use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub terminal_id: String,
    pub command: String,
    pub status: TerminalStatus,
    pub output_buffer: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalStatus {
    Running,
    Completed { exit_code: i32 },
    Failed { error: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TerminalError {
    #[error("Terminal not found: {0}")]
    NotFound(String),

    #[error("Failed to create terminal: {0}")]
    CreationFailed(String),
}

pub struct TerminalManager {
    terminals: Arc<RwLock<HashMap<String, TerminalSession>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_terminal(
        &self,
        command: String,
        _args: Option<Vec<String>>,
        _cwd: Option<String>,
    ) -> Result<String, TerminalError> {
        let timestamp = Uuid::new_v4();
        let terminal_id = format!("term-{}", timestamp);

        let session = TerminalSession {
            terminal_id: terminal_id.clone(),
            command: command.clone(),
            status: TerminalStatus::Running,
            output_buffer: String::new(),
        };

        self.terminals
            .write()
            .await
            .insert(terminal_id.clone(), session);

        Ok(terminal_id)
    }

    pub async fn append_output(&self, terminal_id: &str, output: &str) {
        if let Some(session) = self.terminals.write().await.get_mut(terminal_id) {
            session.output_buffer.push_str(output);
        }
    }

    pub async fn update_status(&self, terminal_id: &str, status: TerminalStatus) {
        if let Some(session) = self.terminals.write().await.get_mut(terminal_id) {
            session.status = status;
        }
    }

    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalSession> {
        self.terminals.read().await.get(terminal_id).cloned()
    }

    pub async fn get_status(&self, terminal_id: &str) -> Option<TerminalStatus> {
        self.terminals
            .read()
            .await
            .get(terminal_id)
            .map(|s| s.status.clone())
    }

    pub async fn get_output(&self, terminal_id: &str) -> Option<String> {
        self.terminals
            .read()
            .await
            .get(terminal_id)
            .map(|s| s.output_buffer.clone())
    }
}

impl Clone for TerminalManager {
    fn clone(&self) -> Self {
        Self {
            terminals: Arc::clone(&self.terminals),
        }
    }
}
