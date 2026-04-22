use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, RwLock};

#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub output: String,
    pub truncated: bool,
    pub exit_status: Option<agent_client_protocol::TerminalExitStatus>,
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

    async fn terminal_kill(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String>;

    async fn terminal_release(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String>;
}

type BridgeStore = Arc<RwLock<Option<Arc<dyn ClientBridgeTrait>>>>;

static GLOBAL_BRIDGE: OnceLock<BridgeStore> = OnceLock::new();

fn global_bridge_store() -> &'static BridgeStore {
    GLOBAL_BRIDGE.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub async fn set_client_bridge(bridge: Arc<dyn ClientBridgeTrait>) {
    let store = global_bridge_store();
    *store.write().await = Some(bridge);
}

pub async fn clear_client_bridge() {
    let store = global_bridge_store();
    *store.write().await = None;
}

pub async fn get_client_bridge() -> Result<Arc<dyn ClientBridgeTrait>, String> {
    let store = global_bridge_store();
    let guard = store.read().await;
    guard
        .clone()
        .ok_or_else(|| "No client bridge available".to_string())
}

enum BridgeRequest {
    ReadTextFile {
        path: String,
        line: Option<u32>,
        limit: Option<u32>,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    WriteTextFile {
        path: String,
        content: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    TerminalCreate {
        session_id: String,
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
        output_byte_limit: Option<u64>,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    TerminalOutput {
        session_id: String,
        terminal_id: String,
        reply: tokio::sync::oneshot::Sender<Result<TerminalOutput, String>>,
    },
    TerminalWaitForExit {
        session_id: String,
        terminal_id: String,
        reply: tokio::sync::oneshot::Sender<Result<TerminalExitResult, String>>,
    },
    TerminalKill {
        session_id: String,
        terminal_id: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    TerminalRelease {
        session_id: String,
        terminal_id: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}

pub struct AcpClientBridge {
    tx: mpsc::Sender<BridgeRequest>,
}

impl AcpClientBridge {
    pub fn new<C: agent_client_protocol::Client + 'static>(client: Arc<C>) -> Self {
        let (tx, mut rx) = mpsc::channel::<BridgeRequest>(64);
        tokio::task::spawn_local(async move {
            while let Some(req) = rx.recv().await {
                match req {
                    BridgeRequest::ReadTextFile { path, line, limit, reply } => {
                        let result = crate::client_methods::read_text_file(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new("default"),
                            &path,
                            line,
                            limit,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::WriteTextFile { path, content, reply } => {
                        let result = crate::client_methods::write_text_file(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new("default"),
                            &path,
                            &content,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::TerminalCreate {
                        session_id,
                        command,
                        args,
                        env,
                        cwd,
                        output_byte_limit,
                        reply,
                    } => {
                        let result = crate::client_methods::terminal_create(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new(&*session_id),
                            &command,
                            args,
                            env,
                            cwd,
                            output_byte_limit,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::TerminalOutput {
                        session_id,
                        terminal_id,
                        reply,
                    } => {
                        let result = crate::client_methods::terminal_output(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new(&*session_id),
                            &terminal_id,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::TerminalWaitForExit {
                        session_id,
                        terminal_id,
                        reply,
                    } => {
                        let result = crate::client_methods::terminal_wait_for_exit(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new(&*session_id),
                            &terminal_id,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::TerminalKill {
                        session_id,
                        terminal_id,
                        reply,
                    } => {
                        let result = crate::client_methods::terminal_kill(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new(&*session_id),
                            &terminal_id,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                    BridgeRequest::TerminalRelease {
                        session_id,
                        terminal_id,
                        reply,
                    } => {
                        let result = crate::client_methods::terminal_release(
                            client.as_ref(),
                            &agent_client_protocol::SessionId::new(&*session_id),
                            &terminal_id,
                        )
                        .await;
                        let _ = reply.send(result);
                    }
                }
            }
        });
        Self { tx }
    }
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
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::ReadTextFile {
                path: path.to_string(),
                line,
                limit,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
    }

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::WriteTextFile {
                path: path.to_string(),
                content: content.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
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
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::TerminalCreate {
                session_id: session_id.to_string(),
                command: command.to_string(),
                args,
                env,
                cwd,
                output_byte_limit,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
    }

    async fn terminal_output(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalOutput, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::TerminalOutput {
                session_id: session_id.to_string(),
                terminal_id: terminal_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
    }

    async fn terminal_wait_for_exit(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<TerminalExitResult, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::TerminalWaitForExit {
                session_id: session_id.to_string(),
                terminal_id: terminal_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
    }

    async fn terminal_kill(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::TerminalKill {
                session_id: session_id.to_string(),
                terminal_id: terminal_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
    }

    async fn terminal_release(
        &self,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(BridgeRequest::TerminalRelease {
                session_id: session_id.to_string(),
                terminal_id: terminal_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "bridge channel closed".to_string())?;
        reply_rx.await.map_err(|_| "bridge response dropped".to_string())?
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

    async fn write_text_file(
        &self,
        _path: &str,
        _content: &str,
    ) -> Result<(), String> {
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

    async fn terminal_kill(
        &self,
        _session_id: &str,
        _terminal_id: &str,
    ) -> Result<(), String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_release(
        &self,
        _session_id: &str,
        _terminal_id: &str,
    ) -> Result<(), String> {
        Err("No client bridge available".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_global_bridge_default() {
        let result = get_client_bridge().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_noop_bridge() {
        let bridge = NoOpClientBridge;
        assert!(!bridge.is_available());
    }
}
