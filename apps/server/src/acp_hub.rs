//! Server-owned ACP session hub.
//!
//! One logical CLI client can reconnect its WebSocket without losing the
//! `LoomAcpAgent` and its ACP sessions. Notification delivery is rebound to
//! the most recently attached connection.

use std::{collections::VecDeque, sync::Arc};

use agent_client_protocol::schema::v1::SessionNotification;
use tokio::sync::{mpsc, oneshot, Mutex};

pub struct AcpHub {
    inner: Mutex<Option<HubInner>>,
}

struct HubInner {
    agent: Arc<loom_acp::LoomAcpAgent>,
    recipient: Arc<Mutex<Option<mpsc::Sender<SessionNotification>>>>,
    replay: Arc<Mutex<VecDeque<SessionNotification>>>,
    lease_cancel: Option<oneshot::Sender<()>>,
}

const REPLAY_CAPACITY: usize = 512;

impl Default for AcpHub {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl AcpHub {
    /// Attach the current WebSocket notification sink and return the durable
    /// ACP agent. A later attach replaces only delivery, never session state.
    pub async fn attach(
        &self,
    ) -> Result<
        (
            Arc<loom_acp::LoomAcpAgent>,
            mpsc::Receiver<SessionNotification>,
            oneshot::Receiver<()>,
        ),
        String,
    > {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let (events_tx, mut events_rx) = mpsc::channel::<SessionNotification>(256);
            let recipient = Arc::new(Mutex::new(None::<mpsc::Sender<SessionNotification>>));
            let replay = Arc::new(Mutex::new(VecDeque::<SessionNotification>::new()));
            let recipient_for_task = recipient.clone();
            let replay_for_task = replay.clone();
            tokio::spawn(async move {
                while let Some(event) = events_rx.recv().await {
                    let mut buffer = replay_for_task.lock().await;
                    if buffer.len() == REPLAY_CAPACITY {
                        buffer.pop_front();
                    }
                    buffer.push_back(event.clone());
                    drop(buffer);
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
            });
        }
        let inner = guard.as_mut().expect("hub initialized");
        if let Some(previous) = inner.lease_cancel.take() {
            let _ = previous.send(());
        }
        let (lease_cancel, lease) = oneshot::channel();
        inner.lease_cancel = Some(lease_cancel);
        let (tx, rx) = mpsc::channel(256);
        let buffered: Vec<_> = inner.replay.lock().await.iter().cloned().collect();
        for event in buffered {
            // The bounded buffer and channel have the same capacity; a full
            // receiver means the caller attached too slowly, so preserve the
            // newest state and stop replaying older notifications.
            if tx.try_send(event).is_err() {
                break;
            }
        }
        *inner.recipient.lock().await = Some(tx);
        Ok((inner.agent.clone(), rx, lease))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reconnect_keeps_the_same_agent_and_session_store() {
        let hub = AcpHub::default();
        let (first, _first_updates, first_lease) = hub.attach().await.expect("first attach");
        let (second, _second_updates, _second_lease) = hub.attach().await.expect("second attach");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first_lease.await.is_ok());
    }
}
