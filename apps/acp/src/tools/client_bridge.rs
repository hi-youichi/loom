use std::collections::HashSet;
use std::sync::Arc;

use crate::client_capabilities::ClientCapabilitiesInfo;
use crate::extensions::question::{QuestionHandler, QuestionReply, QuestionRequest};

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

    /// Ask the connected ACP client a structured question and wait for its
    /// answer. Bridges that do not have a client connection reject this path.
    async fn ask_question(&self, _request: QuestionRequest) -> Result<QuestionReply, String> {
        Err("No question bridge available".to_string())
    }
}

pub struct AcpClientBridge {
    session_id: agent_client_protocol::schema::v1::SessionId,
    connections: Arc<crate::connection_registry::ConnectionRegistry>,
    bindings: Arc<crate::session_bindings::SessionBindings>,
    terminal_ids: tokio::sync::Mutex<HashSet<String>>,
    question_handler: Option<Arc<QuestionHandler>>,
}

impl AcpClientBridge {
    pub fn new(
        session_id: impl Into<String>,
        connections: Arc<crate::connection_registry::ConnectionRegistry>,
        bindings: Arc<crate::session_bindings::SessionBindings>,
    ) -> Self {
        Self {
            session_id: agent_client_protocol::schema::v1::SessionId::new(session_id.into()),
            connections,
            bindings,
            terminal_ids: tokio::sync::Mutex::new(HashSet::new()),
            question_handler: None,
        }
    }

    pub fn with_question_handler(mut self, question_handler: Arc<QuestionHandler>) -> Self {
        self.question_handler = Some(question_handler);
        self
    }

    async fn current_connection(
        &self,
    ) -> Result<
        (
            String,
            Arc<
                tokio::sync::RwLock<
                    Option<agent_client_protocol::ConnectionTo<agent_client_protocol::Client>>,
                >,
            >,
            ClientCapabilitiesInfo,
        ),
        String,
    > {
        let session_id = crate::session::SessionId::new(self.session_id.to_string());
        for connection_id in self.bindings.connections_for(&session_id) {
            let Some(connection) = self.connections.get(&connection_id) else {
                continue;
            };
            if !connection.is_active() {
                continue;
            }
            let Ok(capabilities) = connection.require_capabilities().await else {
                continue;
            };
            return Ok((connection_id, connection.sdk_client_slot(), capabilities));
        }
        Err("No active ACP connection available for session".to_string())
    }
}

#[async_trait::async_trait]
impl ClientBridgeTrait for AcpClientBridge {
    fn is_available(&self) -> bool {
        let session_id = crate::session::SessionId::new(self.session_id.to_string());
        self.bindings
            .connections_for(&session_id)
            .into_iter()
            .any(|connection_id| {
                self.connections
                    .get(&connection_id)
                    .is_some_and(|connection| connection.is_active())
            })
    }

    async fn cleanup(&self) {
        let terminal_ids = std::mem::take(&mut *self.terminal_ids.lock().await);
        for terminal_id in terminal_ids {
            let Ok((_, slot, _)) = self.current_connection().await else {
                continue;
            };
            let guard = slot.read().await;
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
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::read_text_file(conn, &self.session_id, path, line, limit).await
    }

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String> {
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
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
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
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
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
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
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_wait_for_exit(conn, &self.session_id, terminal_id).await
    }

    async fn terminal_kill(&self, _session_id: &str, terminal_id: &str) -> Result<(), String> {
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "No connection available".to_string())?;
        crate::client_methods::terminal_kill(conn, &self.session_id, terminal_id).await
    }

    async fn terminal_release(&self, _session_id: &str, terminal_id: &str) -> Result<(), String> {
        let (_, slot, _) = self.current_connection().await?;
        let guard = slot.read().await;
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

    async fn ask_question(&self, request: QuestionRequest) -> Result<QuestionReply, String> {
        let handler = self
            .question_handler
            .as_ref()
            .ok_or_else(|| "Question capability is unavailable".to_string())?;
        let (connection_id, _, capabilities) = self.current_connection().await?;
        let mut request = request;
        request.session_id = Some(self.session_id.to_string());
        handler
            .request_for_agent(
                request,
                connection_id,
                Some(self.session_id.to_string()),
                capabilities,
            )
            .await
            .map_err(|error| error.to_string())
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
