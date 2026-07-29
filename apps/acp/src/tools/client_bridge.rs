use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub output: String,
    pub truncated: bool,
    pub exit_status: Option<agent_client_protocol::schema::v1::TerminalExitStatus>,
}

#[derive(Debug, Clone)]
pub struct TerminalExitResult {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

#[async_trait::async_trait]
pub trait ClientBridgeTrait: Send + Sync {
    fn is_available(&self) -> bool;

    async fn read_text_file(
        &self,
        path: &str,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String>;

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String>;

    async fn terminal_create(
        &self,
        session_id: &str,
        command: &str,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
        output_byte_limit: Option<u64>,
    ) -> Result<String, String>;

    async fn terminal_output(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalOutput, String>;

    async fn terminal_wait_for_exit(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalExitResult, String>;

    async fn terminal_kill(&self, session_id: &str, terminal_id: &str) -> Result<(), String>;

    async fn terminal_release(&self, session_id: &str, terminal_id: &str) -> Result<(), String>;
}

// Per-session bridge registry: session_id → bridge.
// Replaces the old single GLOBAL_BRIDGE for multi-connection isolation.
type SessionBridgeMap = Arc<RwLock<HashMap<String, Arc<dyn ClientBridgeTrait>>>>;

static SESSION_BRIDGES: OnceLock<SessionBridgeMap> = OnceLock::new();

fn session_bridges() -> &'static SessionBridgeMap {
    SESSION_BRIDGES.get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
}

/// Register a bridge for a specific session.
pub fn set_session_bridge(session_id: &str, bridge: Arc<dyn ClientBridgeTrait>) {
    let map = session_bridges();
    map.write().unwrap().insert(session_id.to_string(), bridge);
}

/// Look up the bridge for a specific session.
pub async fn get_session_bridge(session_id: &str) -> Result<Arc<dyn ClientBridgeTrait>, String> {
    let map = session_bridges();
    let guard = map.read().unwrap();
    guard
        .get(session_id)
        .cloned()
        .ok_or_else(|| format!("No client bridge for session {session_id}"))
}

/// Remove the bridge for a session (on disconnect).
pub fn remove_session_bridge(session_id: &str) {
    let map = session_bridges();
    map.write().unwrap().remove(session_id);
}

pub struct AcpClientBridge {
    conn: Arc<
        tokio::sync::RwLock<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        >,
    >,
}

impl AcpClientBridge {
    pub fn new(
        conn: Arc<
            tokio::sync::RwLock<
                Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
            >,
        >,
    ) -> Self {
        Self { conn }
    }
}

/// Set connection for a specific session.
pub fn set_connection_for_session(
    session_id: &str,
    conn: Arc<
        tokio::sync::RwLock<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        >,
    >,
) {
    let bridge: Arc<dyn ClientBridgeTrait> = Arc::new(AcpClientBridge::new(conn));
    set_session_bridge(session_id, bridge);
    tracing::info!(session_id, "set_connection_for_session: bridge stored");
}

#[async_trait::async_trait]
impl ClientBridgeTrait for AcpClientBridge {
    fn is_available(&self) -> bool {
        true
    }

    async fn read_text_file(
        &self,
        path: &str,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::read_text_file(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new("default"),
            path,
            line,
            limit,
        )
        .await
    }

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::write_text_file(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new("default"),
            path,
            content,
        )
        .await
    }

    async fn terminal_create(
        &self,
        session_id: &str,
        command: &str,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
        output_byte_limit: Option<u64>,
    ) -> Result<String, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_create(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new(session_id),
            command,
            args,
            env,
            cwd,
            output_byte_limit,
        )
        .await
    }

    async fn terminal_output(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalOutput, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_output(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new(session_id),
            terminal_id,
        )
        .await
    }

    async fn terminal_wait_for_exit(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalExitResult, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_wait_for_exit(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new(session_id),
            terminal_id,
        )
        .await
    }

    async fn terminal_kill(&self, session_id: &str, terminal_id: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_kill(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new(session_id),
            terminal_id,
        )
        .await
    }

    async fn terminal_release(&self, session_id: &str, terminal_id: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_release(
            conn,
            &agent_client_protocol::schema::v1::SessionId::new(session_id),
            terminal_id,
        )
        .await
    }
}

pub struct NoOpClientBridge;

#[async_trait::async_trait]
impl ClientBridgeTrait for NoOpClientBridge {
    fn is_available(&self) -> bool {
        false
    }

    async fn read_text_file(
        &self,
        _path: &str,
        _line: Option<u32>,
        _limit: Option<u32>,
    ) -> Result<String, String> {
        Err("No client bridge available".to_string())
    }

    async fn write_text_file(&self, _path: &str, _content: &str) -> Result<(), String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_create(
        &self,
        _session_id: &str,
        _command: &str,
        _args: Vec<String>,
        _env: Vec<(String, String)>,
        _cwd: Option<String>,
        _output_byte_limit: Option<u64>,
    ) -> Result<String, String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_output(
        &self,
        _session_id: &str,
        _terminal_id: &str,
    ) -> Result<TerminalOutput, String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_wait_for_exit(
        &self,
        _session_id: &str,
        _terminal_id: &str,
    ) -> Result<TerminalExitResult, String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_kill(&self, _session_id: &str, _terminal_id: &str) -> Result<(), String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_release(&self, _session_id: &str, _terminal_id: &str) -> Result<(), String> {
        Err("No client bridge available".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_bridge_default() {
        let result = get_session_bridge("nonexistent").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_noop_bridge() {
        let bridge = NoOpClientBridge;
        assert!(!bridge.is_available());
    }
}
