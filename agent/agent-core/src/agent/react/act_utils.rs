//! Pure utility functions for the Act node: truncation, argument parsing,
//! step-progress payload, and error templates.

use serde_json::Value;

use crate::goal_runner::state::ToolError;

/// Event type for Custom stream events emitted after each tool call (step progress).
/// Server or clients can use this to show progress (e.g. "Calling list_dir", "Done: 12 entries").
pub const STEP_PROGRESS_EVENT_TYPE: &str = "step_progress";

/// Default error message template for tool errors.
pub const DEFAULT_TOOL_ERROR_TEMPLATE: &str = "Error: {error}\n Please fix your mistakes.";

/// Default execution error message template with tool name and kwargs.
pub const DEFAULT_EXECUTION_ERROR_TEMPLATE: &str =
    "Error executing tool '{tool_name}' with kwargs {tool_kwargs} with error:\n {error}\n Please fix the error and try again.";

/// Truncates a string for logging, appending "..." if longer than max_len.
pub(crate) fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_len).collect::<String>())
    }
}

/// Parses ToolCall.arguments string to JSON Value.
///
/// Returns `Err(ToolError::InvalidJsonArguments)` for malformed input so the
/// LLM receives actionable feedback instead of a silently-fallbacked `{}`.
pub(crate) fn parse_tool_arguments(tool_name: &str, arguments: &str) -> Result<Value, ToolError> {
    let trimmed = arguments.trim();

    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    let raw = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(v) => v,
        Err(e) => {
            return Err(ToolError::InvalidJsonArguments {
                tool_name: tool_name.to_string(),
                raw_args: arguments.to_string(),
                parse_error: e.to_string(),
            });
        }
    };

    // Handle double-wrapped JSON: "{\"key\": \"val\"}" → {"key": "val"}
    if let Some(s) = raw.as_str() {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
            return Ok(parsed);
        }
    }

    Ok(raw)
}

/// Builds a step_progress Custom event payload for streaming.
pub(crate) fn step_progress_payload(tool_name: &str, call_id: &str, summary: &str) -> Value {
    serde_json::json!({
        "type": STEP_PROGRESS_EVENT_TYPE,
        "node_id": "act",
        "tool_name": tool_name,
        "call_id": call_id,
        "summary": summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate_for_log("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        let long = "a".repeat(50);
        let result = truncate_for_log(&long, 10);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 13);
    }

    #[test]
    fn parse_tool_arguments_valid_json() {
        let v = parse_tool_arguments("grep", r#"{"path": "/tmp"}"#).unwrap();
        assert_eq!(v["path"], "/tmp");
    }

    #[test]
    fn parse_tool_arguments_empty_string() {
        let v = parse_tool_arguments("grep", "").unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn parse_tool_arguments_whitespace_only() {
        let v = parse_tool_arguments("grep", "   ").unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn parse_tool_arguments_invalid_json_returns_error() {
        let result = parse_tool_arguments("grep", "not json {");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tool_arguments_nested_string_json() {
        let v = parse_tool_arguments("grep", r#""{\"key\": \"val\"}""#).unwrap();
        assert_eq!(v["key"], "val");
    }

    #[test]
    fn step_progress_payload_structure() {
        let p = step_progress_payload("bash", "c1", "done");
        assert_eq!(p["type"], STEP_PROGRESS_EVENT_TYPE);
        assert_eq!(p["node_id"], "act");
        assert_eq!(p["tool_name"], "bash");
        assert_eq!(p["call_id"], "c1");
        assert_eq!(p["summary"], "done");
    }
}
