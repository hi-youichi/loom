use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodexUsage {
    pub input_tokens: u32,
    pub cached_input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_output_tokens: u32,
}

impl CodexUsage {
    pub fn zero() -> Self {
        Self {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
        }
    }
}

impl std::ops::Sub for CodexUsage {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(rhs.input_tokens),
            cached_input_tokens: self.cached_input_tokens.saturating_sub(rhs.cached_input_tokens),
            output_tokens: self.output_tokens.saturating_sub(rhs.output_tokens),
            reasoning_output_tokens: self.reasoning_output_tokens.saturating_sub(rhs.reasoning_output_tokens),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── CodexUsage ──

    #[test]
    fn codex_usage_zero() {
        let u = CodexUsage::zero();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.cached_input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.reasoning_output_tokens, 0);
    }

    #[test]
    fn codex_usage_sub_normal() {
        let a = CodexUsage {
            input_tokens: 100,
            cached_input_tokens: 50,
            output_tokens: 200,
            reasoning_output_tokens: 30,
        };
        let b = CodexUsage {
            input_tokens: 40,
            cached_input_tokens: 10,
            output_tokens: 80,
            reasoning_output_tokens: 10,
        };
        let diff = a - b;
        assert_eq!(diff.input_tokens, 60);
        assert_eq!(diff.cached_input_tokens, 40);
        assert_eq!(diff.output_tokens, 120);
        assert_eq!(diff.reasoning_output_tokens, 20);
    }

    #[test]
    fn codex_usage_sub_saturating() {
        let a = CodexUsage {
            input_tokens: 10,
            cached_input_tokens: 5,
            output_tokens: 3,
            reasoning_output_tokens: 1,
        };
        let b = CodexUsage {
            input_tokens: 100,
            cached_input_tokens: 50,
            output_tokens: 30,
            reasoning_output_tokens: 10,
        };
        let diff = a - b;
        assert_eq!(diff.input_tokens, 0);
        assert_eq!(diff.cached_input_tokens, 0);
        assert_eq!(diff.output_tokens, 0);
        assert_eq!(diff.reasoning_output_tokens, 0);
    }

    #[test]
    fn codex_usage_equality() {
        let a = CodexUsage {
            input_tokens: 1,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 4,
        };
        let b = CodexUsage {
            input_tokens: 1,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 4,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn codex_usage_inequality() {
        let a = CodexUsage::zero();
        let b = CodexUsage {
            input_tokens: 1,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
        };
        assert_ne!(a, b);
    }

    // ── CodexEvent serialization ──

    #[test]
    fn thread_started_serializes() {
        let ev = CodexEvent::ThreadStarted {
            thread_id: "t-1".to_string(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "thread_started");
        assert_eq!(v["thread_id"], "t-1");
    }

    #[test]
    fn turn_started_serializes() {
        let ev = CodexEvent::TurnStarted;
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "turn_started");
    }

    #[test]
    fn turn_completed_serializes() {
        let ev = CodexEvent::TurnCompleted {
            usage: CodexUsage {
                input_tokens: 10,
                cached_input_tokens: 5,
                output_tokens: 20,
                reasoning_output_tokens: 2,
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "turn_completed");
        assert_eq!(v["usage"]["input_tokens"], 10);
        assert_eq!(v["usage"]["cached_input_tokens"], 5);
        assert_eq!(v["usage"]["output_tokens"], 20);
        assert_eq!(v["usage"]["reasoning_output_tokens"], 2);
    }

    #[test]
    fn turn_failed_serializes() {
        let ev = CodexEvent::TurnFailed {
            error: CodexErrorInfo {
                message: "something went wrong".to_string(),
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "turn_failed");
        assert_eq!(v["error"]["message"], "something went wrong");
    }

    #[test]
    fn item_started_serializes() {
        let ev = CodexEvent::ItemStarted {
            item: json!({"id": "i-1", "type": "message"}),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "item_started");
        assert_eq!(v["item"]["id"], "i-1");
    }

    #[test]
    fn item_updated_serializes() {
        let ev = CodexEvent::ItemUpdated {
            item: json!({"id": "i-1", "status": "completed"}),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "item_updated");
        assert_eq!(v["item"]["status"], "completed");
    }

    #[test]
    fn item_completed_serializes() {
        let ev = CodexEvent::ItemCompleted {
            item: json!({"id": "i-2"}),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "item_completed");
    }

    #[test]
    fn codex_error_serializes() {
        let ev = CodexEvent::Error {
            message: "fatal".to_string(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["message"], "fatal");
    }

    // ── Helper functions ──

    #[test]
    fn agent_message_item_structure() {
        let v = agent_message_item("msg-1", "hello world");
        assert_eq!(v["id"], "msg-1");
        assert_eq!(v["type"], "agent_message");
        assert_eq!(v["text"], "hello world");
    }

    #[test]
    fn reasoning_item_structure() {
        let v = reasoning_item("r-1", "thinking...");
        assert_eq!(v["id"], "r-1");
        assert_eq!(v["type"], "reasoning");
        assert_eq!(v["text"], "thinking...");
    }

    #[test]
    fn command_execution_item_structure() {
        let v = command_execution_item("cmd-1", "cargo test", "passed", Some(0), "completed");
        assert_eq!(v["id"], "cmd-1");
        assert_eq!(v["type"], "command_execution");
        assert_eq!(v["command"], "cargo test");
        assert_eq!(v["aggregated_output"], "passed");
        assert_eq!(v["exit_code"], 0);
        assert_eq!(v["status"], "completed");
    }

    #[test]
    fn command_execution_item_null_exit_code() {
        let v = command_execution_item("cmd-2", "ls", "", None, "running");
        assert!(v["exit_code"].is_null());
    }

    #[test]
    fn file_change_item_structure() {
        let changes = vec![
            FileUpdateChange {
                path: "src/main.rs".to_string(),
                kind: "edit".to_string(),
            },
            FileUpdateChange {
                path: "src/lib.rs".to_string(),
                kind: "create".to_string(),
            },
        ];
        let v = file_change_item("fc-1", changes, "completed");
        assert_eq!(v["id"], "fc-1");
        assert_eq!(v["type"], "file_change");
        assert_eq!(v["changes"][0]["path"], "src/main.rs");
        assert_eq!(v["changes"][0]["kind"], "edit");
        assert_eq!(v["changes"][1]["path"], "src/lib.rs");
        assert_eq!(v["status"], "completed");
    }

    #[test]
    fn mcp_tool_call_item_with_result() {
        let v = mcp_tool_call_item(
            "mcp-1",
            "filesystem",
            "read_file",
            json!({"path": "/tmp/a.txt"}),
            Some(json!("contents")),
            None,
            "completed",
        );
        assert_eq!(v["id"], "mcp-1");
        assert_eq!(v["type"], "mcp_tool_call");
        assert_eq!(v["server"], "filesystem");
        assert_eq!(v["tool"], "read_file");
        assert_eq!(v["result"], "contents");
        assert!(v["error"].is_null());
    }

    #[test]
    fn mcp_tool_call_item_with_error() {
        let v = mcp_tool_call_item(
            "mcp-2",
            "github",
            "create_issue",
            json!({}),
            None,
            Some(McpToolCallItemError {
                message: "unauthorized".to_string(),
            }),
            "failed",
        );
        assert_eq!(v["error"]["message"], "unauthorized");
        assert!(v["result"].is_null());
    }

    #[test]
    fn error_item_structure() {
        let v = error_item("err-1", "panic!");
        assert_eq!(v["id"], "err-1");
        assert_eq!(v["type"], "error");
        assert_eq!(v["message"], "panic!");
    }
}
