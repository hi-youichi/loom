//! Bridge for ACP Client calls.
//!
//! This module provides a way to call ACP client methods from tools.
//! Since `agent_client_protocol::Client` futures are not `Send`, we use
//! a thread-local approach to bridge the gap.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Terminal output from ACP client.
#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub output: String,
    pub truncated: bool,
    pub exit_status: Option<agent_client_protocol::TerminalExitStatus>,
}

/// Trait for client bridge operations.
/// This abstracts the ACP client calls to allow for testing and mocking.
#[async_trait::async_trait]
pub trait ClientBridgeTrait: Send + Sync {
    /// Check if the bridge is available (has a valid client connection).
    fn is_available(&self) -> bool;

    /// Read a text file from the client.
    /// Returns the file content, or an error message.
    async fn read_text_file(
        &self,
        path: &str,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String>;

    /// Write a text file to the client.
    /// Returns Ok(()) on success, or an error message.
    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String>;

    /// Create a terminal in the client and execute a command.
    /// Returns the terminal ID, or an error message.
    async fn create_terminal(
        &self,
        command: &str,
        args: Option<&[String]>,
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        name: Option<&str>,
    ) -> Result<String, String>;

    /// Get output from a terminal.
    /// Returns the terminal output, or an error message.
    async fn terminal_output(&self, terminal_id: &str) -> Result<TerminalOutput, String>;
}

/// Global client bridge wrapper.
/// Uses Arc<RwLock> for thread-safe shared access.
pub struct GlobalClientBridge {
    inner: Arc<RwLock<Option<Arc<dyn ClientBridgeTrait>>>>,
}

impl GlobalClientBridge {
    /// Check if a bridge is available.
    #[allow(dead_code)]
    pub fn is_available(&self) -> bool {
        let guard = self.inner.read().unwrap();
        guard.as_ref().map(|b| b.is_available()).unwrap_or(false)
    }

    /// Set the client bridge.
    pub fn set(&self, bridge: Arc<dyn ClientBridgeTrait>) {
        let mut guard = self.inner.write().unwrap();
        *guard = Some(bridge);
    }

    /// Clear the client bridge.
    pub fn clear(&self) {
        let mut guard = self.inner.write().unwrap();
        *guard = None;
    }

    /// Get the inner bridge, returning error if not available.
    pub fn get(&self) -> Result<Arc<dyn ClientBridgeTrait>, String> {
        let guard = self.inner.read().unwrap();
        guard.as_ref()
            .cloned()
            .ok_or_else(|| "Client bridge not initialized".to_string())
    }
}

impl Clone for GlobalClientBridge {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Global client bridge instance.
pub static GLOBAL_CLIENT_BRIDGE: OnceLock<GlobalClientBridge> = OnceLock::new();

/// Get or initialize the global client bridge.
fn get_global_bridge() -> &'static GlobalClientBridge {
    GLOBAL_CLIENT_BRIDGE.get_or_init(|| GlobalClientBridge {
        inner: Arc::new(RwLock::new(None)),
    })
}

/// Set the global client bridge.
pub fn set_client_bridge(bridge: Arc<dyn ClientBridgeTrait>) {
    get_global_bridge().set(bridge);
}

/// Clear the global client bridge.
pub fn clear_client_bridge() {
    get_global_bridge().clear();
}

/// Get the global client bridge.
pub fn get_client_bridge() -> Result<Arc<dyn ClientBridgeTrait>, String> {
    get_global_bridge().get()
}

/// A no-op client bridge that returns placeholder responses.
/// Used when no real client is available.
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

    async fn create_terminal(
        &self,
        _command: &str,
        _args: Option<&[String]>,
        _cwd: Option<&str>,
        _env: Option<&HashMap<String, String>>,
        _name: Option<&str>,
    ) -> Result<String, String> {
        Err("No client bridge available".to_string())
    }

    async fn terminal_output(&self, _terminal_id: &str) -> Result<TerminalOutput, String> {
        Err("No client bridge available".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_bridge_default() {
        assert!(!get_global_bridge().is_available());
    }

    #[test]
    fn test_noop_bridge() {
        let bridge = NoOpClientBridge;
        assert!(!bridge.is_available());
    }
}
