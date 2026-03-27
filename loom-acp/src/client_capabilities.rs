//! Client capabilities handling for ACP.
//!
//! This module manages the detection and usage of client capabilities
//! declared during initialization. Currently supports:
//! - File system operations (fs/read_text_file, fs/write_text_file)
//! - Terminal operations (terminal/create, terminal/output, etc.)
//! - Future: MCP capabilities, prompt capabilities

use std::sync::Arc;
use serde_json::Value;

/// Client capabilities as detected during initialization.
#[derive(Debug, Clone, Default)]
pub struct DetectedCapabilities {
    /// Whether the client supports fs/read_text_file
    pub fs_read_text_file: bool,
    /// Whether the client supports fs/write_text_file
    pub fs_write_text_file: bool,
    /// Whether the client supports terminal operations
    pub terminal_supported: bool,
}

impl DetectedCapabilities {
    /// Extract capabilities from ACP ClientCapabilities JSON value.
    /// The ClientCapabilities structure from agent_client_protocol is:
    /// {
    ///   "fs": { "readTextFile": bool, "writeTextFile": bool },
    ///   "terminal": bool,
    ///   "promptCapabilities": { ... },
    ///   "mcpCapabilities": { ... }
    /// }
    pub fn from_client_capabilities_json(caps_json: Option<Value>) -> Self {
        let caps_json = caps_json.unwrap_or_else(|| serde_json::json!({}));
        let caps_obj = caps_json.as_object().cloned().unwrap_or_default();
        
        // Extract fs capabilities
        let fs_caps = caps_obj.get("fs")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        
        let fs_read_text_file = fs_caps.get("readTextFile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        let fs_write_text_file = fs_caps.get("writeTextFile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        // Extract terminal capabilities
        let terminal_supported = caps_obj.get("terminal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        Self {
            fs_read_text_file,
            fs_write_text_file,
            terminal_supported,
        }
    }
}

/// Wrapper around detected capabilities with convenient methods.
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilitiesInfo {
    inner: Arc<DetectedCapabilities>,
}

impl ClientCapabilitiesInfo {
    /// Create from detected capabilities.
    pub fn new(detected: DetectedCapabilities) -> Self {
        Self {
            inner: Arc::new(detected),
        }
    }
    
    /// Create from ACP ClientCapabilities JSON.
    pub fn from_json(caps_json: Option<Value>) -> Self {
        let detected = DetectedCapabilities::from_client_capabilities_json(caps_json);
        Self::new(detected)
    }

    /// Check if client supports fs/read_text_file.
    pub fn can_read_text_file(&self) -> bool {
        self.inner.fs_read_text_file
    }

    /// Check if client supports fs/write_text_file.
    pub fn can_write_text_file(&self) -> bool {
        self.inner.fs_write_text_file
    }

    /// Check if client supports terminal operations.
    pub fn supports_terminal(&self) -> bool {
        self.inner.terminal_supported
    }

    /// Check if client supports terminal/create.
    /// Alias for `supports_terminal()`.
    pub fn can_create_terminal(&self) -> bool {
        self.supports_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_capabilities() {
        let caps = DetectedCapabilities::default();
        assert!(!caps.fs_read_text_file);
        assert!(!caps.fs_write_text_file);
        assert!(!caps.terminal_supported);
    }

    #[test]
    fn test_from_none() {
        let caps = DetectedCapabilities::from_client_capabilities_json(None);
        assert!(!caps.fs_read_text_file);
        assert!(!caps.fs_write_text_file);
        assert!(!caps.terminal_supported);
    }

    #[test]
    fn test_from_partial_capabilities() {
        let caps_json = serde_json::json!({
            "fs": {
                "readTextFile": true,
                "writeTextFile": false
            }
        });

        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.fs_read_text_file);
        assert!(!caps.fs_write_text_file);
        assert!(!caps.terminal_supported);
    }
    
    #[test]
    fn test_from_full_capabilities() {
        let caps_json = serde_json::json!({
            "fs": {
                "readTextFile": true,
                "writeTextFile": true
            },
            "terminal": true
        });

        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.fs_read_text_file);
        assert!(caps.fs_write_text_file);
        assert!(caps.terminal_supported);
    }
    
    #[test]
    fn test_client_capabilities_info() {
        let caps_json = serde_json::json!({
            "fs": {
                "readTextFile": true,
                "writeTextFile": false
            },
            "terminal": true
        });
        
        let info = ClientCapabilitiesInfo::from_json(Some(caps_json));
        assert!(info.can_read_text_file());
        assert!(!info.can_write_text_file());
        assert!(info.supports_terminal());
    }
}
