//! Context persistence for LLM and Tool interactions.
//!
//! Records LLM requests/responses and tool call invocations/results to files
//! for debugging and analysis. Context is written in JSON Lines format for easy
//! parsing and querying.
//!
//! `raw_request` / `raw_response` fields may contain credentials or large payloads; handle accordingly.
//!
//! # Storage Location
//!
//! Context is stored under `~/.loom/thread/$session-id/`:
//! - `think-{turn}.jsonl`: LLM requests and responses for each turn
//! - `tool_context.jsonl`: Tool call invocations and results

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fs::OpenOptions, io::Write};
use tracing::{debug, warn};

/// Context writer that writes to a file in JSON Lines format.
struct ContextWriter {
    file: std::fs::File,
    path: PathBuf,
}

impl ContextWriter {
    fn new(path: PathBuf) -> Result<Self, std::io::Error> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self { file, path })
    }

    fn write(&mut self, entry: &ContextEntry) {
        let json = match serde_json::to_string(entry) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize context entry: {}", e);
                return;
            }
        };

        if let Err(e) = writeln!(self.file, "{}", json) {
            warn!(
                "Failed to write context to file {}: {}",
                self.path.display(),
                e
            );
        }
    }
}

/// Context entry for LLM interactions and tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Entry type: "llm_request", "llm_response", "tool_call", "tool_result"
    pub event_type: String,
    /// Node ID (e.g., "think", "act")
    pub node_id: String,
    /// Event-specific data
    pub data: serde_json::Value,
}

impl ContextEntry {
    fn new(event_type: &str, node_id: &str, data: serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            event_type: event_type.to_string(),
            node_id: node_id.to_string(),
            data,
        }
    }
}

/// Create or get LLM context writer for a session and turn.
fn get_llm_context_writer(session_id: &str, turn_count: u32) -> Option<ContextWriter> {
    let filename = format!("think-{}.jsonl", turn_count);
    let path = env_config::home::thread_session_dir(session_id).join(filename);

    match ContextWriter::new(path.clone()) {
        Ok(writer) => {
            debug!(
                "LLM context writer created for session {}: {}",
                session_id,
                path.display()
            );
            Some(writer)
        }
        Err(e) => {
            warn!(
                "Failed to create LLM context writer for session {} at {}: {}",
                session_id,
                path.display(),
                e
            );
            None
        }
    }
}

/// Create or get Tool context writer for a session.
fn get_tool_context_writer(session_id: &str) -> Option<ContextWriter> {
    let path = env_config::home::thread_session_dir(session_id).join("tool_context.jsonl");

    match ContextWriter::new(path.clone()) {
        Ok(writer) => {
            debug!(
                "Tool context writer created for session {}: {}",
                session_id,
                path.display()
            );
            Some(writer)
        }
        Err(e) => {
            warn!(
                "Failed to create tool context writer for session {} at {}: {}",
                session_id,
                path.display(),
                e
            );
            None
        }
    }
}

/// Save LLM request context (messages sent to LLM).
///
/// Called before invoking the LLM with the input messages.
pub fn save_llm_request(
    node_id: &str,
    session_id: Option<&str>,
    turn_count: u32,
    messages: &[crate::message::Message],
    raw_request: Option<&str>,
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_llm_context_writer(session_id, turn_count) {
        let data = serde_json::json!({
            "messages": messages,
            "message_count": messages.len(),
            "raw_request": raw_request,
        });

        let entry = ContextEntry::new("llm_request", node_id, data);
        writer.write(&entry);
    }
}

/// Save LLM response context (content and tool_calls returned by LLM).
///
/// Called after receiving the LLM response.
pub fn save_llm_response(
    node_id: &str,
    session_id: Option<&str>,
    turn_count: u32,
    content: &str,
    reasoning_content: Option<&str>,
    tool_calls: &[crate::state::ToolCall],
    usage: Option<&crate::llm::LlmUsage>,
    raw_request: Option<&str>,
    raw_response: Option<&str>,
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_llm_context_writer(session_id, turn_count) {
        let data = serde_json::json!({
            "content": content,
            "reasoning_content": reasoning_content,
            "tool_calls": tool_calls.iter().map(|tc| serde_json::json!({
                "id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
            })).collect::<Vec<_>>(),
            "tool_call_count": tool_calls.len(),
            "usage": usage.map(|u| serde_json::json!({
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
            })),
            "raw_request": raw_request,
            "raw_response": raw_response,
        });

        let entry = ContextEntry::new("llm_response", node_id, data);
        writer.write(&entry);
    }
}

/// Save tool call invocation context (before execution).
///
/// Called before invoking a tool.
pub fn save_tool_call(
    node_id: &str,
    session_id: Option<&str>,
    call_id: Option<&str>,
    tool_name: &str,
    arguments: &serde_json::Value,
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_tool_context_writer(session_id) {
        let data = serde_json::json!({
            "call_id": call_id,
            "tool_name": tool_name,
            "arguments": arguments,
        });

        let entry = ContextEntry::new("tool_call", node_id, data);
        writer.write(&entry);
    }
}

/// Save tool result context (after execution).
///
/// Called after a tool execution completes (success or error).
pub fn save_tool_result(
    node_id: &str,
    session_id: Option<&str>,
    call_id: Option<&str>,
    tool_name: &str,
    result: &str,
    is_error: bool,
) {
    save_tool_result_value(
        node_id,
        session_id,
        call_id,
        tool_name,
        serde_json::json!(result),
        is_error,
    );
}

pub fn save_tool_result_value(
    node_id: &str,
    session_id: Option<&str>,
    call_id: Option<&str>,
    tool_name: &str,
    result: serde_json::Value,
    is_error: bool,
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_tool_context_writer(session_id) {
        let data = serde_json::json!({
            "call_id": call_id,
            "tool_name": tool_name,
            "result": result,
            "is_error": is_error,
        });

        let entry = ContextEntry::new("tool_result", node_id, data);
        writer.write(&entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_loom_home<F: FnOnce()>(f: F) {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());
        f();
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn test_timestamp_format() {
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        assert!(ts.ends_with('Z'));
        assert!(ts.chars().nth(4).unwrap() == '-');
        assert!(ts.chars().nth(7).unwrap() == '-');
        assert!(ts.chars().nth(10).unwrap() == 'T');
    }

    #[test]
    fn test_context_entry_serialization() {
        let entry = ContextEntry::new("llm_request", "think", serde_json::json!({"test": "data"}));

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("llm_request"));
        assert!(json.contains("think"));
    }

    #[test]
    fn context_entry_new_sets_timestamp() {
        let entry = ContextEntry::new("tool_call", "act", serde_json::json!({}));
        assert!(!entry.timestamp.is_empty());
        assert!(entry.timestamp.ends_with('Z'));
        assert_eq!(entry.event_type, "tool_call");
        assert_eq!(entry.node_id, "act");
    }

    #[test]
    fn context_entry_deserialize_roundtrip() {
        let entry = ContextEntry::new("llm_response", "node", serde_json::json!({"key": "val"}));
        let json = serde_json::to_string(&entry).unwrap();
        let back: ContextEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "llm_response");
        assert_eq!(back.node_id, "node");
        assert_eq!(back.data["key"], "val");
    }

    #[test]
    fn thread_session_dir_matches_loom_home_layout() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());
        let sess = env_config::home::thread_session_dir("my-session");
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
        assert_eq!(
            sess,
            dir.path().join(env_config::home::THREAD_DIR).join("my-session")
        );
    }

    #[test]
    fn context_writer_creates_file_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("test.jsonl");
        let mut writer = ContextWriter::new(path.clone()).unwrap();
        let entry = ContextEntry::new("test", "node", serde_json::json!({"x": 1}));
        writer.write(&entry);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"event_type\":\"test\""));
        assert!(content.contains("\"x\":1"));
    }

    #[test]
    fn context_writer_appends_multiple_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.jsonl");
        let mut writer = ContextWriter::new(path.clone()).unwrap();
        writer.write(&ContextEntry::new("a", "n1", serde_json::json!({})));
        writer.write(&ContextEntry::new("b", "n2", serde_json::json!({})));
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn get_llm_context_writer_creates_writer() {
        with_loom_home(|| {
            let writer = get_llm_context_writer("test-session", 0);
            assert!(writer.is_some());
        });
    }

    #[test]
    fn get_tool_context_writer_creates_writer() {
        with_loom_home(|| {
            let writer = get_tool_context_writer("test-session");
            assert!(writer.is_some());
        });
    }

    #[test]
    fn save_llm_request_writes_to_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let messages = vec![crate::message::Message::user("hello")];
        save_llm_request("think", Some("sess1"), 0, &messages, Some("{\"test\": \"request\"}"));

        let path = dir
            .path()
            .join("thread")
            .join("sess1")
            .join("think-0.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("llm_request"));
        assert!(content.contains("message_count"));
        assert!(content.contains("raw_request"));
        assert!(
            content.contains("\\\"test\\\": \\\"request\\\"")
                || content.contains("\\\"test\\\":\\\"request\\\"")
        );
        assert!(content.contains("\"User\""));
        assert!(content.contains("hello"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_request_uses_default_session() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_llm_request("think", None, 0, &[], None);

        let path = dir
            .path()
            .join("thread")
            .join("default")
            .join("think-0.jsonl");
        assert!(path.exists());

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_response_writes_to_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let usage = crate::llm::LlmUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            ..Default::default()
        };
        save_llm_response(
            "act",
            Some("sess2"),
            0,
            "response text",
            None,
            &[],
            Some(&usage),
            Some("{\"test\": \"request\"}"),
            Some("{\"test\": \"response\"}"),
        );

        let path = dir
            .path()
            .join("thread")
            .join("sess2")
            .join("think-0.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("llm_response"));
        assert!(content.contains("response text"));
        assert!(content.contains("prompt_tokens"));
        assert!(content.contains("raw_request"));
        assert!(content.contains("raw_response"));
        assert!(
            content.contains("\\\"test\\\": \\\"response\\\"")
                || content.contains("\\\"test\\\":\\\"response\\\"")
        );

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_response_without_usage() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_llm_response("act", Some("sess3"), 0, "hi", None, &[], None, None, None);

        let path = dir
            .path()
            .join("thread")
            .join("sess3")
            .join("think-0.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("llm_response"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_response_with_tool_calls() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let tool_calls = vec![crate::state::ToolCall {
            id: Some("tc1".to_string()),
            name: "read_file".to_string(),
            arguments: "{\"path\": \"/tmp\"}".to_string(),
        }];
        save_llm_response("act", Some("sess_tc"), 0, "", None, &tool_calls, None, None, None);

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        let path = dir
            .path()
            .join("thread")
            .join("sess_tc")
            .join("think-0.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("read_file"));
        assert!(content.contains("tc1"));
    }

    #[test]
    fn save_tool_call_writes_to_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_tool_call(
            "act",
            Some("sess4"),
            Some("call-1"),
            "bash",
            &serde_json::json!({"command": "ls"}),
        );

        let path = dir
            .path()
            .join("thread")
            .join("sess4")
            .join("tool_context.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("tool_call"));
        assert!(content.contains("bash"));
        assert!(content.contains("call-1"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_tool_call_uses_default_session() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_tool_call("act", None, None, "test_tool", &serde_json::json!({}));

        let path = dir
            .path()
            .join("thread")
            .join("default")
            .join("tool_context.jsonl");
        assert!(path.exists());

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_tool_result_writes_to_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_tool_result(
            "act",
            Some("sess5"),
            Some("call-2"),
            "bash",
            "output text",
            false,
        );

        let path = dir
            .path()
            .join("thread")
            .join("sess5")
            .join("tool_context.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("tool_result"));
        assert!(content.contains("output text"));
        assert!(content.contains("\"is_error\":false"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_tool_result_error_flag() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        save_tool_result("act", Some("sess6"), None, "bash", "error msg", true);

        let path = dir
            .path()
            .join("thread")
            .join("sess6")
            .join("tool_context.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"is_error\":true"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_request_multiple_turns() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let messages = vec![crate::message::Message::user("hello")];

        save_llm_request("think", Some("sess"), 0, &messages, None);
        let path0 = dir.path().join("thread/sess/think-0.jsonl");
        assert!(path0.exists());

        save_llm_request("think", Some("sess"), 1, &messages, None);
        let path1 = dir.path().join("thread/sess/think-1.jsonl");
        assert!(path1.exists());

        save_llm_request("think", Some("sess"), 2, &messages, None);
        let path2 = dir.path().join("thread/sess/think-2.jsonl");
        assert!(path2.exists());

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn save_llm_request_serializes_messages_as_json() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let messages = vec![crate::message::Message::user("test message")];
        save_llm_request("think", Some("sess"), 0, &messages, None);

        let path = dir.path().join("thread/sess/think-0.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("\"User\""));
        assert!(content.contains("test message"));
        assert!(!content.contains("Message {"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }
}
