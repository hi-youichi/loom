//! Tool types for LLM function calling.
//!
//! This module defines the tool call and tool specification types
//! that are used by LLM clients and the agent runtime.

use serde::{Deserialize, Serialize};

/// A tool call produced by the LLM (ThinkNode output, ActNode input).
///
/// Aligned with OpenAI `tool_calls` format.
/// The `id` field is optional for backward compatibility with older code.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    /// Unique identifier for correlating with ToolResult.
    /// Set by the LLM provider when emitting tool calls.
    pub id: Option<String>,
    /// Tool name as registered in ToolSource (e.g. "bash", "read_file").
    pub name: String,
    /// Arguments as JSON string; parsed in Act when calling the tool.
    pub arguments: String,
}

impl ToolCall {
    /// Creates a new tool call without an id.
    pub fn new(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    /// Creates a new tool call with an id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Creates a new tool call with a generated id.
    pub fn with_generated_id(mut self) -> Self {
        self.id = Some(format!("call_{}", uuid::Uuid::new_v4()));
        self
    }

    /// Creates a ToolCall from an AssistantToolCall (OpenAI format).
    pub fn from_assistant_tool_call(tc: &crate::message::AssistantToolCall) -> Self {
        Self {
            id: Some(tc.id.clone()),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        }
    }
}

impl From<&crate::message::AssistantToolCall> for ToolCall {
    fn from(tc: &crate::message::AssistantToolCall) -> Self {
        Self::from_assistant_tool_call(tc)
    }
}

/// Tool specification advertised to the LLM.
///
/// Used by the agent to describe available tools when calling the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Tool type (always "function" for now).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Function definition.
    pub function: FunctionSpec,
}

impl ToolSpec {
    /// Creates a new function tool specification.
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Function specification for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSpec {
    /// Function name.
    pub name: String,
    /// Function description.
    pub description: String,
    /// JSON schema for function parameters.
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_new() {
        let tc = ToolCall::new("bash", r#"{"command": "ls"}"#);
        assert_eq!(tc.name, "bash");
        assert_eq!(tc.arguments, r#"{"command": "ls"}"#);
        assert!(tc.id.is_none());
    }

    #[test]
    fn tool_call_with_id() {
        let tc = ToolCall::new("bash", "{}").with_id("call_123");
        assert_eq!(tc.id, Some("call_123".to_string()));
    }

    #[test]
    fn tool_spec_function() {
        let spec = ToolSpec::function(
            "get_weather",
            "Get the weather for a city",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        );
        assert_eq!(spec.tool_type, "function");
        assert_eq!(spec.function.name, "get_weather");
    }

    #[test]
    fn tool_call_serialize() {
        let tc = ToolCall::new("bash", "{}");
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("bash"));
    }

    #[test]
    fn tool_call_deserialize() {
        let json = r#"{"name":"bash","arguments":"{}"}"#;
        let tc: ToolCall = serde_json::from_str(json).unwrap();
        assert_eq!(tc.name, "bash");
    }
}