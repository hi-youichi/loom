use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexEvent {
    ThreadStarted {
        thread_id: String,
    },
    TurnStarted,
    TurnCompleted {
        usage: CodexUsage,
    },
    TurnFailed {
        error: CodexErrorInfo,
    },
    ItemStarted {
        item: Value,
    },
    ItemUpdated {
        item: Value,
    },
    ItemCompleted {
        item: Value,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexUsage {
    pub input_tokens: u32,
    pub cached_input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_output_tokens: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexErrorInfo {
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileUpdateChange {
    pub path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct McpToolCallItemError {
    pub message: String,
}

fn item_base(id: &str, item_type: &str) -> Value {
    serde_json::json!({
        "id": id,
        "type": item_type,
    })
}

pub fn agent_message_item(id: &str, text: &str) -> Value {
    let mut v = item_base(id, "agent_message");
    v["text"] = Value::String(text.to_string());
    v
}

pub fn reasoning_item(id: &str, text: &str) -> Value {
    let mut v = item_base(id, "reasoning");
    v["text"] = Value::String(text.to_string());
    v
}

pub fn command_execution_item(
    id: &str,
    command: &str,
    aggregated_output: &str,
    exit_code: Option<i32>,
    status: &str,
) -> Value {
    serde_json::json!({
        "id": id,
        "type": "command_execution",
        "command": command,
        "aggregated_output": aggregated_output,
        "exit_code": exit_code,
        "status": status,
    })
}

pub fn file_change_item(id: &str, changes: Vec<FileUpdateChange>, status: &str) -> Value {
    serde_json::json!({
        "id": id,
        "type": "file_change",
        "changes": changes,
        "status": status,
    })
}

pub fn mcp_tool_call_item(
    id: &str,
    server: &str,
    tool: &str,
    arguments: Value,
    result: Option<Value>,
    error: Option<McpToolCallItemError>,
    status: &str,
) -> Value {
    serde_json::json!({
        "id": id,
        "type": "mcp_tool_call",
        "server": server,
        "tool": tool,
        "arguments": arguments,
        "result": result,
        "error": error,
        "status": status,
    })
}

pub fn error_item(id: &str, message: &str) -> Value {
    let mut v = item_base(id, "error");
    v["message"] = Value::String(message.to_string());
    v
}
