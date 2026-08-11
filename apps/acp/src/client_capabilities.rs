//! Client capabilities handling for ACP.
//!
//! This module manages the detection and usage of client capabilities
//! declared during initialization. Supports:
//! - File system operations (fs/read_text_file, fs/write_text_file)
//! - Terminal operations (terminal/create, terminal/output, etc.)
//! - MCP capabilities (mcp.http, mcp.stdio, mcp.sse)
//! - Prompt capabilities (text, resource_link, image, audio, embedded_context)
//! - Session capabilities (list, fork, resume)

use serde_json::Value;
use std::sync::Arc;

/// Client capabilities as detected during initialization.
#[derive(Debug, Clone, Default)]
pub struct DetectedCapabilities {
    // File system capabilities
    pub fs_read_text_file: bool,
    pub fs_write_text_file: bool,
    // Terminal capability
    pub terminal_supported: bool,
    // MCP transport capabilities (Gap 1 fix)
    pub mcp_http: bool,
    pub mcp_stdio: bool,
    pub mcp_sse: bool,
    // Prompt capabilities (Gap 2 fix: text + resourceLink baseline)
    pub prompt_text: bool,
    pub prompt_resource_link: bool,
    pub prompt_image: bool,
    pub prompt_audio: bool,
    pub prompt_embedded_context: bool,
    // Session lifecycle capabilities (Gap 3 fix)
    pub session_list: bool,
    pub session_fork: bool,
    pub session_resume: bool,
}

impl DetectedCapabilities {
    /// Extract capabilities from ACP ClientCapabilities JSON value.
    /// The ClientCapabilities structure from agent_client_protocol is:
    /// {
    ///   "fs": { "readTextFile": bool, "writeTextFile": bool },
    ///   "terminal": bool,
    ///   "mcp": { "http": bool, "stdio": bool, "sse": bool },
    ///   "prompts": { "text": bool, "resourceLink": bool, "image": bool, "audio": bool, "embeddedContext": bool },
    ///   "session": { "list": {}, "fork": {}, "resume": {} }
    /// }
    pub fn from_client_capabilities_json(caps_json: Option<Value>) -> Self {
        let caps_json = caps_json.unwrap_or_else(|| serde_json::json!({}));
        let caps_obj = caps_json.as_object().cloned().unwrap_or_default();

        // Extract fs capabilities
        let fs_caps = caps_obj
            .get("fs")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let fs_read_text_file = fs_caps
            .get("readTextFile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let fs_write_text_file = fs_caps
            .get("writeTextFile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Extract terminal capability
        let terminal_supported = caps_obj
            .get("terminal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Extract MCP capabilities
        let mcp_caps = caps_obj
            .get("mcp")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let mcp_http = mcp_caps
            .get("http")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mcp_stdio = mcp_caps
            .get("stdio")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mcp_sse = mcp_caps
            .get("sse")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Extract prompt capabilities
        let prompts_caps = caps_obj
            .get("prompts")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let prompt_text = prompts_caps
            .get("text")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let prompt_resource_link = prompts_caps
            .get("resourceLink")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let prompt_image = prompts_caps
            .get("image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let prompt_audio = prompts_caps
            .get("audio")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let prompt_embedded_context = prompts_caps
            .get("embeddedContext")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Extract session capabilities (presence of key indicates support)
        let session_caps = caps_obj
            .get("session")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let session_list = session_caps.contains_key("list");
        let session_fork = session_caps.contains_key("fork");
        let session_resume = session_caps.contains_key("resume");

        Self {
            fs_read_text_file,
            fs_write_text_file,
            terminal_supported,
            mcp_http,
            mcp_stdio,
            mcp_sse,
            prompt_text,
            prompt_resource_link,
            prompt_image,
            prompt_audio,
            prompt_embedded_context,
            session_list,
            session_fork,
            session_resume,
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

    /// Access the underlying detected capabilities.
    pub fn detected(&self) -> &DetectedCapabilities {
        &self.inner
    }

    // File system checks
    pub fn can_read_text_file(&self) -> bool {
        self.inner.fs_read_text_file
    }
    pub fn can_write_text_file(&self) -> bool {
        self.inner.fs_write_text_file
    }

    // Terminal checks
    pub fn supports_terminal(&self) -> bool {
        self.inner.terminal_supported
    }
    pub fn can_create_terminal(&self) -> bool {
        self.supports_terminal()
    }

    // MCP transport checks
    pub fn supports_mcp_http(&self) -> bool {
        self.inner.mcp_http
    }
    pub fn supports_mcp_stdio(&self) -> bool {
        self.inner.mcp_stdio
    }
    pub fn supports_mcp_sse(&self) -> bool {
        self.inner.mcp_sse
    }
    pub fn supports_mcp(&self) -> bool {
        self.inner.mcp_http || self.inner.mcp_stdio || self.inner.mcp_sse
    }

    // Prompt capability checks
    pub fn supports_prompt_text(&self) -> bool {
        self.inner.prompt_text
    }
    pub fn supports_prompt_resource_link(&self) -> bool {
        self.inner.prompt_resource_link
    }
    pub fn supports_prompt_image(&self) -> bool {
        self.inner.prompt_image
    }
    pub fn supports_prompt_audio(&self) -> bool {
        self.inner.prompt_audio
    }
    pub fn supports_prompt_embedded_context(&self) -> bool {
        self.inner.prompt_embedded_context
    }
    pub fn supports_multimodal_prompts(&self) -> bool {
        self.inner.prompt_image || self.inner.prompt_audio
    }

    // Session capability checks
    pub fn supports_session_list(&self) -> bool {
        self.inner.session_list
    }
    pub fn supports_session_fork(&self) -> bool {
        self.inner.session_fork
    }
    pub fn supports_session_resume(&self) -> bool {
        self.inner.session_resume
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
        assert!(!caps.mcp_http);
        assert!(!caps.mcp_stdio);
        assert!(!caps.mcp_sse);
        assert!(!caps.prompt_text);
        assert!(!caps.prompt_resource_link);
        assert!(!caps.prompt_image);
        assert!(!caps.prompt_audio);
        assert!(!caps.prompt_embedded_context);
        assert!(!caps.session_list);
        assert!(!caps.session_fork);
        assert!(!caps.session_resume);
    }

    #[test]
    fn test_from_none() {
        let caps = DetectedCapabilities::from_client_capabilities_json(None);
        assert!(!caps.fs_read_text_file);
        assert!(!caps.fs_write_text_file);
        assert!(!caps.terminal_supported);
        assert!(!caps.mcp_http);
        assert!(!caps.prompt_text);
        assert!(!caps.session_list);
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

    // ── Gap 1: MCP capability parsing ──────────────────────────────────

    #[test]
    fn test_from_mcp_http_only() {
        let caps_json = serde_json::json!({
            "mcp": { "http": true }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.mcp_http);
        assert!(!caps.mcp_stdio);
        assert!(!caps.mcp_sse);
    }

    #[test]
    fn test_from_mcp_all_transports() {
        let caps_json = serde_json::json!({
            "mcp": { "http": true, "stdio": true, "sse": true }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.mcp_http);
        assert!(caps.mcp_stdio);
        assert!(caps.mcp_sse);
        assert!(ClientCapabilitiesInfo::new(caps.clone()).supports_mcp());
    }

    #[test]
    fn test_from_mcp_defaults_when_absent() {
        let caps = DetectedCapabilities::from_client_capabilities_json(None);
        assert!(!caps.mcp_http);
        assert!(!caps.mcp_stdio);
        assert!(!caps.mcp_sse);
        assert!(!ClientCapabilitiesInfo::new(caps).supports_mcp());
    }

    // ── Gap 2: prompt capability baseline ─────────────────────────────

    #[test]
    fn test_from_prompt_baseline_text_resource_link() {
        let caps_json = serde_json::json!({
            "prompts": { "text": true, "resourceLink": true }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.prompt_text, "text must be true (baseline)");
        assert!(
            caps.prompt_resource_link,
            "resourceLink must be true (baseline)"
        );
    }

    #[test]
    fn test_from_prompt_multimodal() {
        let caps_json = serde_json::json!({
            "prompts": {
                "text": true,
                "resourceLink": true,
                "image": true,
                "audio": true,
                "embeddedContext": true
            }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.prompt_text);
        assert!(caps.prompt_resource_link);
        assert!(caps.prompt_image);
        assert!(caps.prompt_audio);
        assert!(caps.prompt_embedded_context);

        let info = ClientCapabilitiesInfo::new(caps);
        assert!(info.supports_multimodal_prompts());
    }

    #[test]
    fn test_from_prompt_partial_undeclared_default_false() {
        let caps_json = serde_json::json!({
            "prompts": { "image": true }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.prompt_image);
        // Undeclared fields default to false
        assert!(!caps.prompt_text);
        assert!(!caps.prompt_resource_link);
        assert!(!caps.prompt_audio);
    }

    // ── Gap 3: session capability parsing ─────────────────────────────

    #[test]
    fn test_from_session_capabilities_list() {
        let caps_json = serde_json::json!({
            "session": { "list": {} }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.session_list);
        assert!(!caps.session_fork);
        assert!(!caps.session_resume);
    }

    #[test]
    fn test_from_session_capabilities_resume() {
        let caps_json = serde_json::json!({
            "session": { "list": {}, "fork": {}, "resume": {} }
        });
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(caps.session_list);
        assert!(caps.session_fork);
        assert!(caps.session_resume);
    }

    #[test]
    fn test_from_session_empty_object_means_none() {
        let caps_json = serde_json::json!({});
        let caps = DetectedCapabilities::from_client_capabilities_json(Some(caps_json));
        assert!(!caps.session_list);
        assert!(!caps.session_fork);
        assert!(!caps.session_resume);
    }

    // ── Integration: full round-trip ──────────────────────────────────

    #[test]
    fn test_full_capabilities_round_trip() {
        let caps_json = serde_json::json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
            "mcp": { "http": true, "stdio": true },
            "prompts": { "text": true, "resourceLink": true, "image": true },
            "session": { "list": {}, "resume": {} }
        });

        let info = ClientCapabilitiesInfo::from_json(Some(caps_json));

        // Original capabilities
        assert!(info.can_read_text_file());
        assert!(info.can_write_text_file());
        assert!(info.supports_terminal());

        // Gap 1
        assert!(info.supports_mcp_http());
        assert!(info.supports_mcp_stdio());
        assert!(!info.supports_mcp_sse());

        // Gap 2
        assert!(info.supports_prompt_text());
        assert!(info.supports_prompt_resource_link());
        assert!(info.supports_prompt_image());
        assert!(!info.supports_prompt_audio());

        // Gap 3
        assert!(info.supports_session_list());
        assert!(!info.supports_session_fork());
        assert!(info.supports_session_resume());
    }
}
