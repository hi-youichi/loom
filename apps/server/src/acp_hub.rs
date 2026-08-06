//! Server-owned ACP session hub.
//!
//! One logical CLI client can reconnect its WebSocket without losing the
//! `LoomAcpAgent` and its ACP sessions. Notification delivery is rebound to
//! the most recently attached connection.
//!
//! ## Features
//!
//! - **Durable agent**: `LoomAcpAgent` survives WS disconnect.
//! - **Replay buffer**: last `REPLAY_CAPACITY` notifications retained with
//!   monotonic event cursors; reattach drains from a caller-specified cursor.
//! - **Disconnect policy**: `persist` (default) keeps runs alive on WS close;
//!   `cancel` aborts all active generations.
//! - **Session owner**: Bearer-authenticated identity stamped on attach;
//!   cross-owner session/load is rejected.
//! - **Idle TTL**: background sweeper cancels runs whose connection has been
//!   gone longer than the configured TTL.
//! - **Metrics**: connection count, reconnect count, replay drops.

use std::{collections::VecDeque, sync::Arc, time::Instant};

use agent_client_protocol::schema::v1::SessionNotification;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Monotonic event sequence number for replay cursor tracking.
pub type EventCursor = u64;

/// A notification paired with its cursor value.
#[derive(Clone, Debug)]
pub struct CursorNotification {
    pub cursor: EventCursor,
    pub notification: SessionNotification,
}

/// Disconnect behavior when the WS lease drops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisconnectPolicy {
    /// Keep active runs; only cancel via explicit `session/cancel`. Default.
    #[default]
    Persist,
    /// Cancel all active generations when the connection drops.
    Cancel,
}

impl DisconnectPolicy {
    /// Parse from env var `LOOM_ACP_DISCONNECT_POLICY`.
    pub fn from_env() -> Self {
        match std::env::var("LOOM_ACP_DISCONNECT_POLICY")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "cancel" => DisconnectPolicy::Cancel,
            _ => DisconnectPolicy::Persist,
        }
    }
}

/// Identity of the client that owns a connection / session.
#[derive(Clone, Debug, Default)]
pub struct SessionOwner {
    /// Authenticated principal (e.g. bearer subject); `local-anonymous` when
    /// no auth is configured.
    pub principal: String,
}

impl SessionOwner {
    pub fn anonymous() -> Self {
        Self {
            principal: "local-anonymous".to_string(),
        }
    }

    pub fn from_bearer(subject: String) -> Self {
        if subject.is_empty() {
            Self::anonymous()
        } else {
            Self { principal: subject }
        }
    }
}

/// Lightweight metrics counters for observability.
#[derive(Debug, Default, Clone)]
pub struct AcpHubStats {
    pub total_connections: u64,
    pub total_reconnects: u64,
    pub total_replay_dropped: u64,
    pub total_disconnects: u64,
}

/// Configuration for [`AcpHub`].
#[derive(Debug)]
pub struct AcpHubConfig {
    pub replay_capacity: usize,
    pub disconnect_policy: DisconnectPolicy,
    /// Max idle seconds before cancelling orphaned runs (0 = disabled).
    pub idle_ttl_secs: u64,
}

impl Default for AcpHubConfig {
    fn default() -> Self {
        Self {
            replay_capacity: 512,
            disconnect_policy: DisconnectPolicy::from_env(),
            idle_ttl_secs: 0,
        }
    }
}

pub struct AcpHub {
    inner: Mutex<Option<HubInner>>,
    config: AcpHubConfig,
    stats: Mutex<AcpHubStats>,
}

struct HubInner {
    agent: Arc<loom_acp::LoomAcpAgent>,
    recipient: Arc<Mutex<Option<mpsc::Sender<SessionNotification>>>>,
    replay: Arc<Mutex<VecDeque<CursorNotification>>>,
    lease_cancel: Option<oneshot::Sender<()>>,
    /// Owner of the current connection.
    owner: SessionOwner,
    /// Wall-clock time when the current lease started.
    attached_at: Instant,
    /// Wall-clock time when the last lease was dropped (for idle TTL).
    last_detach_at: Option<Instant>,
    /// Monotonic generation counter — each `attach_with` increments it.
    /// `note_detach` only clears recipient when the generation matches,
    /// preventing stale detach calls from killing a newer connection.
    generation: u64,
    /// Handle to the idle TTL sweeper task.  Replaced on each `attach_with`
    /// so that only one sweeper is active at a time (preventing
    /// accumulation across reconnections).
    ttl_sweeper: Option<tokio::task::JoinHandle<()>>,
}

impl Default for AcpHub {
    fn default() -> Self {
        Self::new(AcpHubConfig::default())
    }
}

impl AcpHub {
    /// Create with explicit config.
    pub fn new(config: AcpHubConfig) -> Self {
        Self {
            inner: Mutex::new(None),
            config,
            stats: Mutex::new(AcpHubStats::default()),
        }
    }

    /// Attach with default (anonymous) owner.
    pub async fn attach(
        &self,
    ) -> Result<
        (
            Arc<loom_acp::LoomAcpAgent>,
            mpsc::Receiver<SessionNotification>,
            oneshot::Receiver<()>,
            u64,
        ),
        String,
    > {
        self.attach_with(SessionOwner::anonymous(), None).await
    }

    /// Attach with owner identity and optional resume-from cursor.
    ///
    /// On first call the durable agent is created. Subsequent calls rebind
    /// the notification sink to the new WS connection.
    ///
    /// - `owner`: the authenticated principal for this connection.
    /// - `resume_from`: if `Some(cursor)`, only replay notifications whose
    ///   cursor is **greater** than the given value. If `None`, replay all.
    pub async fn attach_with(
        &self,
        owner: SessionOwner,
        resume_from: Option<EventCursor>,
    ) -> Result<
        (
            Arc<loom_acp::LoomAcpAgent>,
            mpsc::Receiver<SessionNotification>,
            oneshot::Receiver<()>,
            u64,
        ),
        String,
    > {
        let mut guard = self.inner.lock().await;
        let is_reconnect = guard.is_some();

        if guard.is_none() {
            let (events_tx, mut events_rx) = mpsc::channel::<SessionNotification>(256);
            let recipient = Arc::new(Mutex::new(None::<mpsc::Sender<SessionNotification>>));
            let replay = Arc::new(Mutex::new(VecDeque::<CursorNotification>::new()));

            let recipient_for_task = recipient.clone();
            let replay_for_task = replay.clone();
            let cursor_for_task = Arc::new(std::sync::atomic::AtomicU64::new(1));
            let cap = self.config.replay_capacity;
            tokio::spawn(async move {
                while let Some(event) = events_rx.recv().await {
                    let cur = cursor_for_task.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    {
                        let mut buffer = replay_for_task.lock().await;
                        if buffer.len() == cap {
                            buffer.pop_front();
                        }
                        buffer.push_back(CursorNotification {
                            cursor: cur,
                            notification: event.clone(),
                        });
                    }
                    let target = recipient_for_task.lock().await.clone();
                    if let Some(tx) = target {
                        let _ = tx.send(event).await;
                    }
                }
            });

            let agent = loom_acp::LoomAcpAgent::with_session_update_tx(events_tx)
                .map_err(|e| e.to_string())?;
            *guard = Some(HubInner {
                agent: Arc::new(agent),
                recipient,
                replay,
                lease_cancel: None,
                owner: owner.clone(),
                attached_at: Instant::now(),
                last_detach_at: None,
                generation: 0,
                ttl_sweeper: None,
            });
        }

        let inner = guard.as_mut().expect("hub initialized");

        // Cross-owner rejection (single-connection model: new attach replaces
        // the old one, but if a different principal tries to attach, reject).
        if is_reconnect && inner.owner.principal != owner.principal {
            tracing::warn!(
                existing_owner = %inner.owner.principal,
                new_owner = %owner.principal,
                "Cross-owner attach rejected"
            );
            return Err(format!(
                "connection owned by '{}'; cross-owner session takeover is not supported",
                inner.owner.principal
            ));
        }

        // Cancel previous lease.
        if let Some(previous) = inner.lease_cancel.take() {
            let _ = previous.send(());
        }

        // Apply disconnect policy for the outgoing connection.
        if is_reconnect && self.config.disconnect_policy == DisconnectPolicy::Cancel {
            tracing::info!("DisconnectPolicy::Cancel — cancelling active generations");
            inner.agent.cancel_all();
        }

        let (lease_cancel, lease) = oneshot::channel();
        inner.lease_cancel = Some(lease_cancel);
        inner.owner = owner.clone();
        inner.attached_at = Instant::now();
        inner.last_detach_at = None;
        inner.generation += 1;
        let generation = inner.generation;

        let (tx, rx) = mpsc::channel(256);

        // Replay buffered notifications.
        let buffered: Vec<_> = inner
            .replay
            .lock()
            .await
            .iter()
            .filter(|cn| {
                resume_from
                    .map(|rf| cn.cursor > rf)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        let replay_count = buffered.len();
        for cn in buffered {
            if tx.try_send(cn.notification).is_err() {
                self.stats.lock().await.total_replay_dropped += 1;
                break;
            }
        }
        *inner.recipient.lock().await = Some(tx);

        // Update metrics.
        {
            let mut s = self.stats.lock().await;
            s.total_connections += 1;
            if is_reconnect {
                s.total_reconnects += 1;
            }
        }

        // Spawn idle TTL sweeper if configured.  Abort the previous sweeper
        // (from an earlier attach) so only one is active per hub at a time.
        if self.config.idle_ttl_secs > 0 {
            if let Some(prev) = inner.ttl_sweeper.take() {
                prev.abort();
            }
            let agent = inner.agent.clone();
            let ttl = std::time::Duration::from_secs(self.config.idle_ttl_secs);
            let recipient = inner.recipient.clone();
            let sweeper = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(ttl).await;
                    let alive = recipient.lock().await.is_some();
                    if !alive {
                        tracing::info!(ttl_secs = ttl.as_secs(), "Idle TTL exceeded, cancelling orphaned runs");
                        agent.cancel_all();
                        break;
                    }
                }
            });
            inner.ttl_sweeper = Some(sweeper);
        }

        tracing::info!(
            reconnect = is_reconnect,
            replay_count,
            owner = %owner.principal,
            "AcpHub attached"
        );

        Ok((inner.agent.clone(), rx, lease, generation))
    }

    /// Get a snapshot of hub metrics.
    pub async fn stats(&self) -> AcpHubStats {
        self.stats.lock().await.clone()
    }

    /// Mark the connection identified by `generation` as detached.
    ///
    /// Only clears the notification recipient when `generation` matches the
    /// current connection — a stale detach from an older connection is a no-op
    /// (besides incrementing the disconnect counter), preventing it from
    /// killing the newer connection's notification delivery.
    pub async fn note_detach(&self, generation: u64) {
        let mut guard = self.inner.lock().await;
        if let Some(inner) = guard.as_mut() {
            let is_current = inner.generation == generation;
            if is_current {
                inner.last_detach_at = Some(Instant::now());
                *inner.recipient.lock().await = None;
                self.stats.lock().await.total_disconnects += 1;
            }
            tracing::info!(generation, is_current, "AcpHub detached");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reconnect_keeps_the_same_agent_and_session_store() {
        let hub = AcpHub::default();
        let (first, _first_updates, first_lease, _gen1) = hub.attach().await.expect("first attach");
        let (second, _second_updates, _second_lease, _gen2) = hub.attach().await.expect("second attach");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first_lease.await.is_ok());
    }

    #[tokio::test]
    async fn cross_owner_attach_is_rejected() {
        let hub = AcpHub::default();
        let _ = hub.attach().await.expect("first attach");
        let result = hub
            .attach_with(
                SessionOwner::from_bearer("someone-else".to_string()),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn disconnect_policy_cancel_aborts_on_reconnect() {
        let hub = AcpHub::new(AcpHubConfig {
            disconnect_policy: DisconnectPolicy::Cancel,
            ..AcpHubConfig::default()
        });
        let (_agent, _rx, _lease, _gen) = hub.attach().await.expect("attach");
        // Reconnect with cancel policy should not panic.
        let (_agent2, _rx2, _lease2, _gen2) = hub.attach().await.expect("reconnect");
    }

    #[tokio::test]
    async fn stats_track_connections_and_reconnects() {
        let hub = AcpHub::default();
        hub.attach().await.unwrap();
        hub.attach().await.unwrap();
        let s = hub.stats().await;
        assert_eq!(s.total_connections, 2);
        assert_eq!(s.total_reconnects, 1);
    }

    #[tokio::test]
    async fn note_detach_with_stale_generation_does_not_clear_new_recipient() {
        let hub = AcpHub::default();
        // First connection
        let (_, _, _, gen1) = hub.attach().await.expect("first attach");
        // Second connection (reconnect) — should get gen2
        let (_, _, _, gen2) = hub.attach().await.expect("second attach");
        assert_ne!(gen1, gen2);
        // Stale detach from first connection — should NOT clear recipient
        // and should NOT increment total_disconnects (Bug 10 fix).
        hub.note_detach(gen1).await;
        let s = hub.stats().await;
        assert_eq!(s.total_disconnects, 0, "stale detach should not increment total_disconnects");
        // Now detach the current generation
        hub.note_detach(gen2).await;
        let s = hub.stats().await;
        assert_eq!(s.total_disconnects, 1);
    }
}
