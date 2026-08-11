use std::collections::HashSet;
use std::sync::Arc;

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

    /// Release client-side resources that are still owned by this session.
    /// Implementations may use this during session close/delete or prompt
    /// cancellation; the default keeps test and local bridges source
    /// compatible.
    async fn cleanup(&self) {}

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

pub struct AcpClientBridge {
    session_id: agent_client_protocol::schema::v1::SessionId,
    conn: Arc<
        tokio::sync::RwLock<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        >,
    >,
    terminal_ids: tokio::sync::Mutex<HashSet<String>>,
}

impl AcpClientBridge {
    pub fn new(
        session_id: impl Into<String>,
        conn: Arc<
            tokio::sync::RwLock<
                Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
            >,
        >,
    ) -> Self {
        Self {
            session_id: agent_client_protocol::schema::v1::SessionId::new(session_id.into()),
            conn,
            terminal_ids: tokio::sync::Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait::async_trait]
impl ClientBridgeTrait for AcpClientBridge {
    fn is_available(&self) -> bool {
        true
    }

    async fn cleanup(&self) {
        let terminal_ids = std::mem::take(&mut *self.terminal_ids.lock().await);
        for terminal_id in terminal_ids {
            let guard = self.conn.read().await;
            let Some(conn) = guard.as_ref() else {
                continue;
            };
            let _ =
                crate::client_methods::terminal_kill(conn, &self.session_id, &terminal_id).await;
            let _ =
                crate::client_methods::terminal_release(conn, &self.session_id, &terminal_id).await;
        }
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
        crate::client_methods::read_text_file(conn, &self.session_id, path, line, limit).await
    }

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::write_text_file(conn, &self.session_id, path, content).await
    }

    async fn terminal_create(
        &self,
        _session_id: &str,
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
        let terminal_id = crate::client_methods::terminal_create(
            conn,
            &self.session_id,
            command,
            args,
            env,
            cwd,
            output_byte_limit,
        )
        .await?;
        self.terminal_ids.lock().await.insert(terminal_id.clone());
        Ok(terminal_id)
    }

    async fn terminal_output(
        &self,
        _session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalOutput, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_output(conn, &self.session_id, terminal_id).await
    }

    async fn terminal_wait_for_exit(
        &self,
        _session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalExitResult, String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_wait_for_exit(conn, &self.session_id, terminal_id).await
    }

    async fn terminal_kill(&self, _session_id: &str, terminal_id: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_kill(conn, &self.session_id, terminal_id).await
    }

    async fn terminal_release(&self, _session_id: &str, terminal_id: &str) -> Result<(), String> {
        let guard: tokio::sync::RwLockReadGuard<
            Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
        > = self.conn.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        let result =
            crate::client_methods::terminal_release(conn, &self.session_id, terminal_id).await;
        if result.is_ok() {
            self.terminal_ids.lock().await.remove(terminal_id);
        }
        result
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

    #[test]
    fn test_noop_bridge() {
        let bridge = NoOpClientBridge;
        assert!(!bridge.is_available());
    }
}
