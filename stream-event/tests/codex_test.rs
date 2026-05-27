use stream_event::codex::*;
use stream_event::CodexEvent;

#[test]
fn codex_usage_zero() {
    let u = CodexUsage::zero();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.cached_input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.reasoning_output_tokens, 0);
}

#[test]
fn codex_usage_sub_saturating() {
    let a = CodexUsage {
        input_tokens: 10,
        cached_input_tokens: 5,
        output_tokens: 20,
        reasoning_output_tokens: 3,
    };
    let b = CodexUsage {
        input_tokens: 15,
        cached_input_tokens: 2,
        output_tokens: 10,
        reasoning_output_tokens: 1,
    };
    let diff = a - b;
    assert_eq!(diff.input_tokens, 0); // saturating
    assert_eq!(diff.cached_input_tokens, 3);
    assert_eq!(diff.output_tokens, 10);
    assert_eq!(diff.reasoning_output_tokens, 2);
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
        input_tokens: 10,
        cached_input_tokens: 5,
        output_tokens: 20,
        reasoning_output_tokens: 3,
    };
    let diff = a - b;
    assert_eq!(diff.input_tokens, 90);
    assert_eq!(diff.cached_input_tokens, 45);
    assert_eq!(diff.output_tokens, 180);
    assert_eq!(diff.reasoning_output_tokens, 27);
}

#[test]
fn codex_event_thread_started_serializes() {
    let ev = CodexEvent::ThreadStarted {
        thread_id: "t-1".to_string(),
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "thread_started");
    assert_eq!(v["thread_id"], "t-1");
}

#[test]
fn codex_event_turn_started_serializes() {
    let ev = CodexEvent::TurnStarted;
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "turn_started");
}

#[test]
fn codex_event_turn_completed_serializes() {
    let ev = CodexEvent::TurnCompleted {
        usage: CodexUsage {
            input_tokens: 100,
            cached_input_tokens: 50,
            output_tokens: 200,
            reasoning_output_tokens: 10,
        },
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "turn_completed");
    assert_eq!(v["usage"]["input_tokens"], 100);
    assert_eq!(v["usage"]["output_tokens"], 200);
}

#[test]
fn codex_event_turn_failed_serializes() {
    let ev = CodexEvent::TurnFailed {
        error: CodexErrorInfo {
            message: "timeout".to_string(),
        },
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "turn_failed");
    assert_eq!(v["error"]["message"], "timeout");
}

#[test]
fn codex_event_item_started_serializes() {
    let ev = CodexEvent::ItemStarted {
        item: serde_json::json!({"id": "i-1"}),
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "item_started");
    assert_eq!(v["item"]["id"], "i-1");
}

#[test]
fn codex_event_item_updated_serializes() {
    let ev = CodexEvent::ItemUpdated {
        item: serde_json::json!({"progress": 50}),
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "item_updated");
}

#[test]
fn codex_event_item_completed_serializes() {
    let ev = CodexEvent::ItemCompleted {
        item: serde_json::json!({"done": true}),
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "item_completed");
}

#[test]
fn codex_event_error_serializes() {
    let ev = CodexEvent::Error {
        message: "something broke".to_string(),
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "something broke");
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
fn agent_message_item_structure() {
    let item = agent_message_item("msg-1", "hello");
    assert_eq!(item["id"], "msg-1");
    assert_eq!(item["type"], "agent_message");
    assert_eq!(item["text"], "hello");
}

#[test]
fn reasoning_item_structure() {
    let item = reasoning_item("r-1", "thinking...");
    assert_eq!(item["id"], "r-1");
    assert_eq!(item["type"], "reasoning");
    assert_eq!(item["text"], "thinking...");
}

#[test]
fn command_execution_item_structure() {
    let item = command_execution_item("cmd-1", "cargo test", "running...", Some(0), "completed");
    assert_eq!(item["id"], "cmd-1");
    assert_eq!(item["type"], "command_execution");
    assert_eq!(item["command"], "cargo test");
    assert_eq!(item["aggregated_output"], "running...");
    assert_eq!(item["exit_code"], 0);
    assert_eq!(item["status"], "completed");
}

#[test]
fn command_execution_item_null_exit_code() {
    let item = command_execution_item("cmd-2", "ls", "file.txt", None, "running");
    assert!(item["exit_code"].is_null());
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
    let item = file_change_item("fc-1", changes.clone(), "completed");
    assert_eq!(item["id"], "fc-1");
    assert_eq!(item["type"], "file_change");
    assert_eq!(item["changes"].as_array().unwrap().len(), 2);
    assert_eq!(item["status"], "completed");
}

#[test]
fn mcp_tool_call_item_with_result() {
    let item = mcp_tool_call_item(
        "mcp-1",
        "my-server",
        "my_tool",
        serde_json::json!({"arg": "val"}),
        Some(serde_json::json!({"output": "ok"})),
        None,
        "completed",
    );
    assert_eq!(item["id"], "mcp-1");
    assert_eq!(item["type"], "mcp_tool_call");
    assert_eq!(item["server"], "my-server");
    assert_eq!(item["tool"], "my_tool");
    assert_eq!(item["arguments"]["arg"], "val");
    assert_eq!(item["result"]["output"], "ok");
    assert!(item["error"].is_null());
}

#[test]
fn mcp_tool_call_item_with_error() {
    let item = mcp_tool_call_item(
        "mcp-2",
        "my-server",
        "fail_tool",
        serde_json::json!({}),
        None,
        Some(McpToolCallItemError {
            message: "crashed".to_string(),
        }),
        "failed",
    );
    assert!(item["result"].is_null());
    assert_eq!(item["error"]["message"], "crashed");
}

#[test]
fn error_item_structure() {
    let item = error_item("e-1", "fatal error");
    assert_eq!(item["id"], "e-1");
    assert_eq!(item["type"], "error");
    assert_eq!(item["message"], "fatal error");
}

#[test]
fn codex_error_info_equality() {
    let a = CodexErrorInfo {
        message: "err".to_string(),
    };
    let b = CodexErrorInfo {
        message: "err".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn file_update_change_serializes() {
    let change = FileUpdateChange {
        path: "foo.rs".to_string(),
        kind: "edit".to_string(),
    };
    let v = serde_json::to_value(&change).unwrap();
    assert_eq!(v["path"], "foo.rs");
    assert_eq!(v["kind"], "edit");
}
