//! Unit tests for ReAct state and tool types.
//!
//! Tests construction, Clone, Debug, and edge cases for ReActState, ToolCall, ToolResult.

mod init_logging;

use graphweave::{Message, ReActState, ToolCall, ToolResult};

// --- ToolCall ---

#[test]
fn tool_call_default_and_construction() {
    let t = ToolCall::default();
    assert!(t.name.is_empty());
    assert!(t.arguments.is_empty());
    assert!(t.id.is_none());

    let t = ToolCall {
        name: "get_time".into(),
        arguments: r#"{}"#.into(),
        id: Some("call-1".into()),
    };
    assert_eq!(t.name, "get_time");
    assert_eq!(t.arguments, "{}");
    assert_eq!(t.id.as_deref(), Some("call-1"));
}

#[test]
fn tool_call_without_id() {
    let t = ToolCall {
        name: "search".into(),
        arguments: r#"{"q":"rust"}"#.into(),
        id: None,
    };
    assert_eq!(t.name, "search");
    assert_eq!(t.arguments, r#"{"q":"rust"}"#);
    assert!(t.id.is_none());
}

#[test]
fn tool_call_clone() {
    let t = ToolCall {
        name: "get_time".into(),
        arguments: "{}".into(),
        id: Some("call-1".into()),
    };
    let c = t.clone();
    assert_eq!(c.name, t.name);
    assert_eq!(c.arguments, t.arguments);
    assert_eq!(c.id, t.id);
}

#[test]
fn tool_call_debug() {
    let t = ToolCall {
        name: "get_time".into(),
        arguments: "{}".into(),
        id: Some("call-1".into()),
    };
    let s = format!("{:?}", t);
    assert!(s.contains("get_time"));
    assert!(s.contains("call-1"));
}

// --- ToolResult ---

#[test]
fn tool_result_default_and_construction() {
    let r = ToolResult::default();
    assert!(r.call_id.is_none());
    assert!(r.name.is_none());
    assert!(r.content.is_empty());

    let r = ToolResult {
        call_id: Some("call-1".into()),
        name: Some("get_time".into()),
        content: "2025-01-29 12:00:00".into(),
    };
    assert_eq!(r.call_id.as_deref(), Some("call-1"));
    assert_eq!(r.name.as_deref(), Some("get_time"));
    assert_eq!(r.content, "2025-01-29 12:00:00");
}

#[test]
fn tool_result_call_id_only() {
    let r = ToolResult {
        call_id: Some("call-1".into()),
        name: None,
        content: "ok".into(),
    };
    assert_eq!(r.call_id.as_deref(), Some("call-1"));
    assert!(r.name.is_none());
    assert_eq!(r.content, "ok");
}

#[test]
fn tool_result_name_only() {
    let r = ToolResult {
        call_id: None,
        name: Some("get_time".into()),
        content: "12:00".into(),
    };
    assert!(r.call_id.is_none());
    assert_eq!(r.name.as_deref(), Some("get_time"));
    assert_eq!(r.content, "12:00");
}

#[test]
fn tool_result_clone() {
    let r = ToolResult {
        call_id: Some("call-1".into()),
        name: Some("get_time".into()),
        content: "12:00".into(),
    };
    let c = r.clone();
    assert_eq!(c.call_id, r.call_id);
    assert_eq!(c.name, r.name);
    assert_eq!(c.content, r.content);
}

#[test]
fn tool_result_debug() {
    let r = ToolResult {
        call_id: Some("call-1".into()),
        name: Some("get_time".into()),
        content: "12:00".into(),
    };
    let s = format!("{:?}", r);
    assert!(s.contains("12:00"));
}

// --- ReActState ---

#[test]
fn react_state_default() {
    let s = ReActState::default();
    assert!(s.messages.is_empty());
    assert!(s.tool_calls.is_empty());
    assert!(s.tool_results.is_empty());
    assert_eq!(s.turn_count, 0);
}

#[test]
fn react_state_construction_and_clone() {
    let state = ReActState {
        messages: vec![
            Message::user("What time is it?"),
            Message::assistant("I'll check."),
        ],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("call-1".into()),
        }],
        tool_results: vec![ToolResult {
            call_id: Some("call-1".into()),
            name: Some("get_time".into()),
            content: "12:00".into(),
        }],
        turn_count: 0,
        approval_result: None,
    };
    assert_eq!(state.messages.len(), 2);
    assert_eq!(state.tool_calls.len(), 1);
    assert_eq!(state.tool_results.len(), 1);
    assert_eq!(state.tool_calls[0].name, "get_time");
    assert_eq!(state.tool_results[0].content, "12:00");

    let cloned = state.clone();
    assert_eq!(cloned.messages.len(), state.messages.len());
    assert_eq!(cloned.tool_calls[0].id, state.tool_calls[0].id);
}

#[test]
fn react_state_clone_field_by_field() {
    let state = ReActState {
        messages: vec![
            Message::system("You are helpful."),
            Message::user("Hi"),
            Message::assistant("Hello."),
        ],
        tool_calls: vec![
            ToolCall {
                name: "get_time".into(),
                arguments: "{}".into(),
                id: Some("call-1".into()),
            },
            ToolCall {
                name: "search".into(),
                arguments: r#"{"q":"x"}"#.into(),
                id: Some("call-2".into()),
            },
        ],
        tool_results: vec![
            ToolResult {
                call_id: Some("call-1".into()),
                name: Some("get_time".into()),
                content: "12:00".into(),
            },
            ToolResult {
                call_id: Some("call-2".into()),
                name: Some("search".into()),
                content: "[]".into(),
            },
        ],
        turn_count: 0,
        approval_result: None,
    };
    let cloned = state.clone();
    assert_eq!(cloned.messages.len(), 3);
    assert_eq!(cloned.tool_calls.len(), 2);
    assert_eq!(cloned.tool_results.len(), 2);
    assert_eq!(cloned.tool_calls[0].name, "get_time");
    assert_eq!(cloned.tool_calls[1].name, "search");
    assert_eq!(cloned.tool_results[0].content, "12:00");
    assert_eq!(cloned.tool_results[1].content, "[]");
}

#[test]
fn react_state_with_all_message_variants() {
    let state = ReActState {
        messages: vec![
            Message::system("System prompt"),
            Message::user("User input"),
            Message::assistant("Assistant reply"),
        ],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
    };
    assert_eq!(state.messages.len(), 3);
    match &state.messages[0] {
        Message::System(s) => assert_eq!(s, "System prompt"),
        _ => panic!("expected System"),
    }
    match &state.messages[1] {
        Message::User(s) => assert_eq!(s, "User input"),
        _ => panic!("expected User"),
    }
    match &state.messages[2] {
        Message::Assistant(s) => assert_eq!(s, "Assistant reply"),
        _ => panic!("expected Assistant"),
    }
}

#[test]
fn react_state_empty_tool_calls_non_empty_results() {
    let state = ReActState {
        messages: vec![Message::user("hi")],
        tool_calls: vec![],
        tool_results: vec![ToolResult {
            call_id: None,
            name: Some("get_time".into()),
            content: "12:00".into(),
        }],
        turn_count: 0,
        approval_result: None,
    };
    assert!(state.tool_calls.is_empty());
    assert_eq!(state.tool_results.len(), 1);
    assert_eq!(state.tool_results[0].content, "12:00");
}

#[test]
fn react_state_debug() {
    let state = ReActState {
        messages: vec![Message::user("hi")],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: None,
        }],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
    };
    let s = format!("{:?}", state);
    assert!(s.contains("messages"));
    assert!(s.contains("tool_calls"));
    assert!(s.contains("tool_results"));
}

// --- ReActState::last_assistant_reply ---

/// **Scenario**: No Assistant message yields None.
#[test]
fn last_assistant_reply_none_when_no_assistant() {
    let state = ReActState {
        messages: vec![Message::system("You are helpful."), Message::user("Hi")],
        ..Default::default()
    };
    assert_eq!(state.last_assistant_reply(), None);
}

/// **Scenario**: Chronologically last Assistant message content is returned.
#[test]
fn last_assistant_reply_returns_last_assistant_content() {
    let state = ReActState {
        messages: vec![
            Message::system("S"),
            Message::user("U1"),
            Message::assistant("A1"),
            Message::user("U2"),
            Message::assistant("Final reply."),
        ],
        ..Default::default()
    };
    assert_eq!(
        state.last_assistant_reply().as_deref(),
        Some("Final reply.")
    );
}

/// **Scenario**: Empty Assistant content (e.g. tool_calls only) returns Some("").
#[test]
fn last_assistant_reply_empty_content_returns_some_empty() {
    let state = ReActState {
        messages: vec![
            Message::user("Hi"),
            Message::assistant(""), // e.g. turn with only tool_calls
        ],
        ..Default::default()
    };
    assert_eq!(state.last_assistant_reply().as_deref(), Some(""));
}

/// **Scenario**: Default state (no messages) returns None.
#[test]
fn last_assistant_reply_default_state_none() {
    let state = ReActState::default();
    assert_eq!(state.last_assistant_reply(), None);
}

#[test]
fn react_state_send_sync_compile_time() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ReActState>();
    assert_send_sync::<ToolCall>();
    assert_send_sync::<ToolResult>();
}
