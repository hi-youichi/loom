use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

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
