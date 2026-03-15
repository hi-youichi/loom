//! Context persistence for LLM and Tool interactions.
//!
//! Records LLM requests/responses and tool call invocations/results to files
//! for debugging and analysis. Context is written in JSON Lines format for easy
//! parsing and querying.
//!
//! # Storage Location
//!
//! Context is stored under `~/.loom/context/{session_id}/`:
//! - `llm_context.jsonl`: LLM requests and responses
//! - `tool_context.jsonl`: Tool call invocations and results

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{
    fs::OpenOptions,
    io::Write,
};
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

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        Ok(Self {
            file,
            path,
        })
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
            warn!("Failed to write context to file {}: {}", self.path.display(), e);
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

/// Returns `~/.loom/context` (via `loom_home()`).
fn get_base_context_dir() -> PathBuf {
    env_config::home::loom_home().join("context")
}

/// Create or get LLM context writer for a session.
fn get_llm_context_writer(session_id: &str) -> Option<ContextWriter> {
    let path = get_base_context_dir()
        .join(session_id)
        .join("llm_context.jsonl");
    
    match ContextWriter::new(path.clone()) {
        Ok(writer) => {
            debug!("LLM context writer created for session {}: {}", session_id, path.display());
            Some(writer)
        }
        Err(e) => {
            warn!("Failed to create LLM context writer for session {} at {}: {}", session_id, path.display(), e);
            None
        }
    }
}

/// Create or get Tool context writer for a session.
fn get_tool_context_writer(session_id: &str) -> Option<ContextWriter> {
    let path = get_base_context_dir()
        .join(session_id)
        .join("tool_context.jsonl");
    
    match ContextWriter::new(path.clone()) {
        Ok(writer) => {
            debug!("Tool context writer created for session {}: {}", session_id, path.display());
            Some(writer)
        }
        Err(e) => {
            warn!("Failed to create tool context writer for session {} at {}: {}", session_id, path.display(), e);
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
    messages: &[crate::message::Message],
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_llm_context_writer(session_id) {
        let data = serde_json::json!({
            "messages": messages.iter().map(|m| format!("{:?}", m)).collect::<Vec<_>>(),
            "message_count": messages.len(),
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
    content: &str,
    tool_calls: &[crate::state::ToolCall],
    usage: Option<&crate::llm::LlmUsage>,
) {
    let session_id = session_id.unwrap_or("default");
    if let Some(mut writer) = get_llm_context_writer(session_id) {
        let data = serde_json::json!({
            "content": content,
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
        let entry = ContextEntry::new(
            "llm_request",
            "think",
            serde_json::json!({"test": "data"}),
        );

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
    fn get_base_context_dir_uses_loom_home() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());
        let ctx_dir = get_base_context_dir();
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
        assert_eq!(ctx_dir, dir.path().join("context"));
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
            let writer = get_llm_context_writer("test-session");
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
        save_llm_request("think", Some("sess1"), &messages);

        let path = dir.path().join("context").join("sess1").join("llm_context.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("llm_request"));
        assert!(content.contains("message_count"));

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

        save_llm_request("think", None, &[]);

        let path = dir.path().join("context").join("default").join("llm_context.jsonl");
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
        };
        save_llm_response("act", Some("sess2"), "response text", &[], Some(&usage));

        let path = dir.path().join("context").join("sess2").join("llm_context.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("llm_response"));
        assert!(content.contains("response text"));
        assert!(content.contains("prompt_tokens"));

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

        save_llm_response("act", Some("sess3"), "hi", &[], None);

        let path = dir.path().join("context").join("sess3").join("llm_context.jsonl");
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
        save_llm_response("act", Some("sess_tc"), "", &tool_calls, None);

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        let path = dir.path().join("context").join("sess_tc").join("llm_context.jsonl");
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

        let path = dir.path().join("context").join("sess4").join("tool_context.jsonl");
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

        let path = dir.path().join("context").join("default").join("tool_context.jsonl");
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

        save_tool_result("act", Some("sess5"), Some("call-2"), "bash", "output text", false);

        let path = dir.path().join("context").join("sess5").join("tool_context.jsonl");
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

        let path = dir.path().join("context").join("sess6").join("tool_context.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"is_error\":true"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }
}
