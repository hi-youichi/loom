use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub terminal_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub output_byte_limit: Option<u64>,
    pub status: TerminalStatus,
    pub output_buffer: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalStatus {
    Running,
    Completed { exit_code: Option<u32>, signal: Option<String> },
    Killed,
    Released,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TerminalError {
    #[error("Terminal not found: {0}")]
    NotFound(String),

    #[error("Failed to create terminal: {0}")]
    CreationFailed(String),

    #[error("Terminal already released: {0}")]
    AlreadyReleased(String),

    #[error("Terminal not running: {0}")]
    NotRunning(String),
}

struct TerminalEntry {
    session: TerminalSession,
    child: Option<Child>,
    output_notify: Arc<Notify>,
    exit_notify: Arc<Notify>,
}

pub struct TerminalManager {
    terminals: Arc<RwLock<HashMap<String, TerminalEntry>>>,
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
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: Vec<(String, String)>,
        output_byte_limit: Option<u64>,
    ) -> Result<String, TerminalError> {
        let terminal_id = format!("term-{}", Uuid::new_v4());

        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref dir) = cwd {
            cmd.current_dir(dir);
        }

        for (k, v) in &env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| TerminalError::CreationFailed(e.to_string()))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let session = TerminalSession {
            terminal_id: terminal_id.clone(),
            command,
            args,
            cwd,
            env,
            output_byte_limit,
            status: TerminalStatus::Running,
            output_buffer: String::new(),
            truncated: false,
        };

        let output_notify = Arc::new(Notify::new());
        let exit_notify = Arc::new(Notify::new());

        let entry = TerminalEntry {
            session,
            child: Some(child),
            output_notify: output_notify.clone(),
            exit_notify: exit_notify.clone(),
        };

        self.terminals.write().await.insert(terminal_id.clone(), entry);

        if let Some(stdout) = stdout {
            self.spawn_output_reader(terminal_id.clone(), stdout, output_byte_limit);
        }
        if let Some(stderr) = stderr {
            self.spawn_output_reader(terminal_id.clone(), stderr, output_byte_limit);
        }

        self.spawn_exit_watcher(terminal_id.clone());

        Ok(terminal_id)
    }

    fn spawn_exit_watcher(&self, terminal_id: String) {
        let terminals = self.terminals.clone();
        tokio::spawn(async move {
            let child_process = {
                let mut map = terminals.write().await;
                if let Some(entry) = map.get_mut(&terminal_id) {
                    entry.child.take()
                } else {
                    None
                }
            };

            if let Some(mut child) = child_process {
                let result = child.wait().await;
                let status = match result {
                    Ok(exit_status) => {
                        if exit_status.success() {
                            TerminalStatus::Completed {
                                exit_code: exit_status.code().map(|c| c as u32),
                                signal: None,
                            }
                        } else if exit_status.code().is_some() {
                            TerminalStatus::Completed {
                                exit_code: exit_status.code().map(|c| c as u32),
                                signal: None,
                            }
                        } else {
                            TerminalStatus::Completed {
                                exit_code: None,
                                signal: None,
                            }
                        }
                    }
                    Err(_) => TerminalStatus::Completed {
                        exit_code: None,
                        signal: None,
                    },
                };

                let mut map = terminals.write().await;
                if let Some(entry) = map.get_mut(&terminal_id) {
                    if matches!(entry.session.status, TerminalStatus::Running) {
                        entry.session.status = status;
                        entry.exit_notify.notify_waiters();
                        entry.output_notify.notify_waiters();
                    }
                }
            }
        });
    }

    fn spawn_output_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        &self,
        terminal_id: String,
        mut reader: R,
        _output_byte_limit: Option<u64>,
    ) {
        let terminals = self.terminals.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        let mut map = terminals.write().await;
                        if let Some(entry) = map.get_mut(&terminal_id) {
                            if let Some(limit) = entry.session.output_byte_limit {
                                let new_len = entry.session.output_buffer.len() + text.len();
                                if new_len > limit as usize {
                                    let excess = new_len - limit as usize;
                                    let current_len = entry.session.output_buffer.len();
                                    if excess < current_len {
                                        entry.session.output_buffer =
                                            entry.session.output_buffer[excess..].to_string();
                                    } else {
                                        entry.session.output_buffer.clear();
                                    }
                                    entry.session.truncated = true;
                                }
                            }
                            entry.session.output_buffer.push_str(&text);
                            entry.output_notify.notify_waiters();
                        }
                    }
                    Err(_) => break,
                }
            }

            let mut map = terminals.write().await;
            if let Some(entry) = map.get_mut(&terminal_id) {
                if matches!(entry.session.status, TerminalStatus::Running) {
                    entry.output_notify.notify_waiters();
                }
            }
        });
    }

    pub async fn wait_for_exit(&self, terminal_id: &str) -> Result<TerminalStatus, TerminalError> {
        let exit_notify = {
            let map = self.terminals.read().await;
            let entry = map.get(terminal_id).ok_or_else(|| {
                TerminalError::NotFound(terminal_id.to_string())
            })?;
            if matches!(entry.session.status, TerminalStatus::Released) {
                return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
            }
            entry.exit_notify.clone()
        };

        exit_notify.notified().await;

        let map = self.terminals.read().await;
        let entry = map.get(terminal_id).ok_or_else(|| {
            TerminalError::NotFound(terminal_id.to_string())
        })?;
        Ok(entry.session.status.clone())
    }

    pub async fn kill(&self, terminal_id: &str) -> Result<(), TerminalError> {
        let mut map = self.terminals.write().await;
        let entry = map.get_mut(terminal_id).ok_or_else(|| {
            TerminalError::NotFound(terminal_id.to_string())
        })?;

        if matches!(entry.session.status, TerminalStatus::Released) {
            return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
        }

        if let Some(ref mut child) = entry.child {
            let _ = child.kill().await;
        }
        entry.session.status = TerminalStatus::Killed;
        entry.exit_notify.notify_waiters();
        entry.output_notify.notify_waiters();
        Ok(())
    }

    pub async fn release(&self, terminal_id: &str) -> Result<(), TerminalError> {
        let mut map = self.terminals.write().await;
        let entry = map.get_mut(terminal_id).ok_or_else(|| {
            TerminalError::NotFound(terminal_id.to_string())
        })?;

        if matches!(entry.session.status, TerminalStatus::Released) {
            return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
        }

        if let Some(ref mut child) = entry.child {
            let _ = child.kill().await;
        }
        entry.child = None;
        entry.session.status = TerminalStatus::Released;
        entry.exit_notify.notify_waiters();
        entry.output_notify.notify_waiters();
        Ok(())
    }

    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalSession> {
        self.terminals.read().await.get(terminal_id).map(|e| e.session.clone())
    }

    pub async fn get_status(&self, terminal_id: &str) -> Option<TerminalStatus> {
        self.terminals
            .read()
            .await
            .get(terminal_id)
            .map(|e| e.session.status.clone())
    }

    pub async fn get_output(&self, terminal_id: &str) -> Option<(String, bool, Option<TerminalStatus>)> {
        self.terminals.read().await.get(terminal_id).map(|e| {
            (
                e.session.output_buffer.clone(),
                e.session.truncated,
                if !matches!(e.session.status, TerminalStatus::Running) {
                    Some(e.session.status.clone())
                } else {
                    None
                },
            )
        })
    }

    pub async fn append_output(&self, terminal_id: &str, output: &str) {
        if let Some(entry) = self.terminals.write().await.get_mut(terminal_id) {
            entry.session.output_buffer.push_str(output);
        }
    }

    pub async fn update_status(&self, terminal_id: &str, status: TerminalStatus) {
        if let Some(entry) = self.terminals.write().await.get_mut(terminal_id) {
            entry.session.status = status;
        }
    }
}

impl Clone for TerminalManager {
    fn clone(&self) -> Self {
        Self {
            terminals: Arc::clone(&self.terminals),
        }
    }
}
