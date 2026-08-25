//! Server-owned ACP runtime and multi-connection lifecycle.

use std::sync::Arc;

use tokio::sync::Mutex;

pub type EventCursor = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisconnectPolicy {
    #[default]
    Persist,
    Cancel,
}

impl DisconnectPolicy {
    pub fn from_env() -> Self {
        match std::env::var("ANUREO_ACP_DISCONNECT_POLICY")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "cancel" => Self::Cancel,
            _ => Self::Persist,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionOwner {
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

#[derive(Debug, Default, Clone)]
pub struct AcpHubStats {
    pub total_connections: u64,
    /// Kept for metrics compatibility. A second simultaneous transport is no
    /// longer classified as a reconnect.
    pub total_reconnects: u64,
    pub total_replay_dropped: u64,
    pub total_disconnects: u64,
    pub active_connections: u64,
    pub active_sessions: u64,
    pub active_prompts: u64,
    pub total_prompts: u64,
    pub prompt_busy_rejections: u64,
    pub route_failures: u64,
    pub session_rebinds: u64,
}

#[derive(Debug)]
pub struct AcpHubConfig {
    /// Legacy setting retained for configuration compatibility. Standard
    /// session/load replaces connection-level replay.
    pub replay_capacity: usize,
    pub disconnect_policy: DisconnectPolicy,
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

pub struct AcpConnectionLease {
    pub runtime: Arc<anureo_acp::runtime::AcpRuntime>,
    pub connection: Arc<anureo_acp::connection::AcpConnection>,
    pub outbound_rx: tokio::sync::mpsc::Receiver<anureo_acp::connection::ConnectionOutbound>,
}

pub struct AcpHub {
    runtime: Mutex<Option<Arc<anureo_acp::runtime::AcpRuntime>>>,
    config: AcpHubConfig,
    stats: Mutex<AcpHubStats>,
}

impl Default for AcpHub {
    fn default() -> Self {
        Self::new(AcpHubConfig::default())
    }
}

impl AcpHub {
    pub fn new(config: AcpHubConfig) -> Self {
        Self {
            runtime: Mutex::new(None),
            config,
            stats: Mutex::new(AcpHubStats::default()),
        }
    }

    pub fn with_runtime(config: AcpHubConfig, runtime: Arc<anureo_acp::runtime::AcpRuntime>) -> Self {
        Self {
            runtime: Mutex::new(Some(runtime)),
            config,
            stats: Mutex::new(AcpHubStats::default()),
        }
    }

    async fn shared_runtime(&self) -> Result<Arc<anureo_acp::runtime::AcpRuntime>, String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.is_none() {
            *runtime = Some(anureo_acp::runtime::AcpRuntime::new().map_err(|e| e.to_string())?);
        }
        Ok(runtime.as_ref().expect("runtime initialized").clone())
    }

    pub async fn attach(&self) -> Result<AcpConnectionLease, String> {
        self.attach_with(SessionOwner::anonymous(), None).await
    }

    /// Open an independent connection. `resume_from` is accepted only for
    /// backward API compatibility and is intentionally ignored.
    pub async fn attach_with(
        &self,
        owner: SessionOwner,
        _resume_from: Option<EventCursor>,
    ) -> Result<AcpConnectionLease, String> {
        let runtime = self.shared_runtime().await?;
        let opened = runtime.open_connection(owner.principal.clone());
        let active_connections = runtime.connections.len() as u64;
        {
            let mut stats = self.stats.lock().await;
            stats.total_connections += 1;
            stats.active_connections = active_connections;
        }
        tracing::info!(
            connection_id = %opened.connection.id,
            principal = %owner.principal,
            active_connections,
            "ACP connection opened"
        );
        Ok(AcpConnectionLease {
            runtime,
            connection: opened.connection,
            outbound_rx: opened.outbound_rx,
        })
    }

    pub async fn close_connection(&self, connection_id: &str) {
        let runtime = self.runtime.lock().await.as_ref().cloned();
        let Some(runtime) = runtime else {
            return;
        };
        let cancel_now = self.config.disconnect_policy == DisconnectPolicy::Cancel;
        let sessions = runtime.close_connection(connection_id, cancel_now);
        let bound_session_count = sessions.len();

        if !cancel_now && self.config.idle_ttl_secs > 0 && !sessions.is_empty() {
            let runtime = runtime.clone();
            let ttl = std::time::Duration::from_secs(self.config.idle_ttl_secs);
            tokio::spawn(async move {
                tokio::time::sleep(ttl).await;
                for session_id in sessions {
                    // A successfully loaded/resumed session has a new binding
                    // and is no longer an orphan.
                    if runtime.bindings.connection_for(&session_id).is_none() {
                        runtime
                            .agent
                            .sessions()
                            .cancel_current_generation(&session_id);
                    }
                }
            });
        }

        let mut stats = self.stats.lock().await;
        stats.total_disconnects += 1;
        stats.active_connections = runtime.connections.len() as u64;
        tracing::info!(
            connection_id,
            bound_sessions = bound_session_count,
            active_connections = stats.active_connections,
            "ACP connection closed"
        );
    }

    pub async fn stats(&self) -> AcpHubStats {
        let mut stats = self.stats.lock().await;
        if let Some(runtime) = self.runtime.lock().await.as_ref().cloned() {
            let metrics = runtime.metrics();
            stats.active_connections = metrics.active_connections;
            stats.active_sessions = metrics.active_sessions;
            stats.active_prompts = metrics.active_prompts;
            stats.total_prompts = metrics.total_prompts;
            stats.prompt_busy_rejections = metrics.prompt_busy_rejections;
            stats.route_failures = metrics.route_failures;
            stats.session_rebinds = metrics.session_rebinds;
        }
        stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn two_connections_coexist_and_share_one_runtime() {
        let hub = AcpHub::default();
        let first = hub.attach().await.expect("first attach");
        let second = hub.attach().await.expect("second attach");
        assert!(Arc::ptr_eq(&first.runtime, &second.runtime));
        assert_ne!(first.connection.id, second.connection.id);
        assert!(first.connection.is_active());
        assert!(second.connection.is_active());
        assert_eq!(hub.stats().await.active_connections, 2);
    }

    #[tokio::test]
    async fn closing_one_connection_does_not_close_the_other() {
        let hub = AcpHub::default();
        let first = hub.attach().await.expect("first attach");
        let second = hub.attach().await.expect("second attach");
        hub.close_connection(&second.connection.id).await;
        assert!(first.connection.is_active());
        assert!(!second.connection.is_active());
        assert_eq!(hub.stats().await.active_connections, 1);
    }

    #[tokio::test]
    async fn different_owners_can_hold_independent_connections() {
        let hub = AcpHub::default();
        let first = hub
            .attach_with(SessionOwner::from_bearer("owner-a".into()), None)
            .await
            .unwrap();
        let second = hub
            .attach_with(SessionOwner::from_bearer("owner-b".into()), None)
            .await
            .unwrap();
        assert_eq!(first.connection.principal, "owner-a");
        assert_eq!(second.connection.principal, "owner-b");
    }
}
