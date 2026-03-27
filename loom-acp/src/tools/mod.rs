//! ACP Client Tools — Tools that call Client methods via ACP protocol.
//!
//! These tools allow Loom to leverage IDE capabilities when running as an ACP agent:
//! - File system operations (fs/read_text_file, fs/write_text_file)
//! - Terminal operations (terminal/create, etc.)
//!
//! Tools are only available when the Client declares support in initialize request.
//! If a capability is not available, tools fall back to local execution or return errors.

mod client_bridge;
mod fs_tools;
mod terminal_tools;

pub use client_bridge::{
    ClientBridgeTrait, NoOpClientBridge, TerminalOutput,
    set_client_bridge, clear_client_bridge, get_client_bridge,
};
pub use fs_tools::{ReadTextFileTool, WriteTextFileTool};
pub use terminal_tools::{CreateTerminalTool, TerminalOutputTool};

use crate::client_capabilities::ClientCapabilitiesInfo;
use loom::tools::Tool;

/// Helper function to create a tool spec with common fields.
pub(crate) fn create_tool_spec(name: &str, description: &str, input_schema: serde_json::Value) -> loom::tool_source::ToolSpec {
    loom::tool_source::ToolSpec {
        name: name.to_string(),
        description: Some(description.to_string()),
        input_schema,
        output_hint: None,
    }
}

/// Create all available ACP client tools based on capabilities.
///
/// This function returns tools that the client supports based on capabilities.
/// Tools that require capabilities the client doesn't have are not included.
///
/// Note: Before using these tools, you must call `set_global_client_bridge()`
/// to set up the client bridge for ACP communication.
pub fn create_acp_tools(capabilities: &ClientCapabilitiesInfo) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    // File system tools
    if capabilities.can_read_text_file() {
        tools.push(Box::new(ReadTextFileTool::new()));
    }

    if capabilities.can_write_text_file() {
        tools.push(Box::new(WriteTextFileTool::new()));
    }

    // Terminal tools
    if capabilities.can_create_terminal() {
        tools.push(Box::new(CreateTerminalTool::new()));
        tools.push(Box::new(TerminalOutputTool::new()));
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_create_acp_tools_empty() {
        // Default capabilities should not create any tools
        let caps = ClientCapabilitiesInfo::default();
        let tools = create_acp_tools(&caps);
        assert!(tools.is_empty());
    }

    #[test]
    fn test_create_acp_tools_fs_read() {
        let caps_json = json!({
            "fs": {
                "readTextFile": true
            }
        });
        let caps = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let tools = create_acp_tools(&caps);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "fs/read_text_file");
    }

    #[test]
    fn test_create_acp_tools_fs_all() {
        let caps_json = json!({
            "fs": {
                "readTextFile": true,
                "writeTextFile": true
            }
        });
        let caps = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let tools = create_acp_tools(&caps);
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_create_acp_tools_all() {
        let caps_json = json!({
            "fs": {
                "readTextFile": true,
                "writeTextFile": true
            },
            "terminal": true
        });
        let caps = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let tools = create_acp_tools(&caps);
        assert_eq!(tools.len(), 4); // 2 fs tools + 2 terminal tools
    }

    #[test]
    fn test_tool_specs() {
        let read_tool = ReadTextFileTool::new();
        let spec = read_tool.spec();
        assert_eq!(spec.name, "fs/read_text_file");
        assert!(spec.description.is_some());

        let write_tool = WriteTextFileTool::new();
        let spec = write_tool.spec();
        assert_eq!(spec.name, "fs/write_text_file");
        assert!(spec.description.is_some());

        let create_terminal_tool = CreateTerminalTool::new();
        let spec = create_terminal_tool.spec();
        assert_eq!(spec.name, "terminal_create");
        assert!(spec.description.is_some());

        let output_tool = TerminalOutputTool::new();
        let spec = output_tool.spec();
        assert_eq!(spec.name, "terminal_output");
        assert!(spec.description.is_some());
    }
}
