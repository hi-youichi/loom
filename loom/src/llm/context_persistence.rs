//! Context persistence for LLM and Tool interactions.
//!
//! Records LLM requests/responses and tool call invocations/results to files
//! for debugging and analysis. Context is written in JSON Lines format for easy
//! parsing and querying.
//!
//! # Storage Location
//!
//! Context is stored in the XDG data directory, organized by session:
//! - Linux/macOS: `~/.local/share/loom/context/{session_id}/`
//! - Windows: `%LOCALAPPDATA%\loom\context\{session_id}\`
//!
//! Context files per session:
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

/// Get the XDG data directory for context storage.
/// Uses ~/.local/share/loom/context on all Unix-like systems (Linux and macOS).
fn get_base_context_dir() -> PathBuf {
    // On Unix-like systems (Linux, macOS), use ~/.local/share/loom/context
    // On Windows, use %LOCALAPPDATA%/loom/context
    #[cfg(unix)]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".local")
                    .join("share")
            })
            .join("loom")
            .join("context")
    }
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loom")
            .join("context")
    }
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
}
