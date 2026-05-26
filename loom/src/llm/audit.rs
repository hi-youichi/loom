//! LLM audit log: persist LLM API call (request + response) to file for debugging,
//! cost analysis, and conversation replay.
//!
//! # Design
//!
//! - Each session writes to a separate JSONL file (`{thread_id}.jsonl`).
//! - A tokio background task writes entries asynchronously via an mpsc channel.
//! - Sanitization: API keys and Authorization headers are stripped.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::memory::uuid6;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single LLM call audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditEntry {
    /// Unique ID (UUID v6).
    pub id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Session identifier (from LlmHeaders.thread_id).
    pub thread_id: String,
    /// Call type: "chat" or "chat_stream".
    #[serde(rename = "type")]
    pub entry_type: String,
    /// Model name.
    pub model: String,
    /// Request URL.
    pub url: String,
    /// Request duration in milliseconds.
    pub duration_ms: u64,
    /// HTTP status code (200 on success, actual value on error).
    pub status: u16,
    /// Request details.
    pub request: LlmAuditRequest,
    /// Response details (None on error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<LlmAuditResponse>,
    /// Error message (None on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Request portion of an audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRequest {
    /// Messages sent to the API (serialized as JSON Value).
    pub messages: serde_json::Value,
    /// Tool definitions, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    /// Request parameters.
    pub parameters: LlmAuditRequestParams,
}

/// Request parameter summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRequestParams {
    pub temperature: Option<f32>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

/// Response portion of an audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditResponse {
    /// Assistant reply content.
    pub content: String,
    /// Reasoning / thinking content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Token usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmAuditUsage>,
    /// Tool calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmAuditToolCall>,
}

/// Token usage (mirrors `LlmUsage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Tool call record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// LLM audit log interface.
pub trait LlmAuditLog: Send + Sync {
    /// Record one audit entry (non-blocking).
    fn log(&self, entry: LlmAuditEntry);
}

/// No-op implementation: does nothing.
pub struct NoOpLlmAuditLog;

impl LlmAuditLog for NoOpLlmAuditLog {
    fn log(&self, _entry: LlmAuditEntry) {}
}

// ---------------------------------------------------------------------------
// File-based implementation
// ---------------------------------------------------------------------------

use tokio::sync::mpsc;
use tracing::warn;

/// Background writer message.
enum AuditMsg {
    Write(LlmAuditEntry),
}

/// File-based audit log.
///
/// Uses an `mpsc::unbounded_channel` for async writes; a background tokio task
/// consumes the queue and appends each record as a JSONL line to the file.
pub struct FileLlmAuditLog {
    tx: mpsc::UnboundedSender<AuditMsg>,
}

impl FileLlmAuditLog {
    /// Create a new file audit log.
    ///
    /// - `base_path`: log directory, e.g. `~/.loom/data/llm_logs/`
    ///
    /// Spawns a background tokio task for writing. When `FileLlmAuditLog` is
    /// dropped (the sender is dropped), the task exits automatically.
    pub fn new(base_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(Self::writer_task(rx, base_path));
        Self { tx }
    }

    async fn writer_task(
        mut rx: mpsc::UnboundedReceiver<AuditMsg>,
        base_path: PathBuf,
    ) {
        while let Some(msg) = rx.recv().await {
            match msg {
                AuditMsg::Write(entry) => {
                    let thread_id = entry.thread_id.clone();
                    let file_path = base_path.join(format!("{}.jsonl", thread_id));
                    if let Err(e) = Self::append_entry(&file_path, &entry) {
                        warn!(
                            path = %file_path.display(),
                            error = %e,
                            "Failed to write LLM audit log"
                        );
                    }
                }
            }
        }
    }

    /// Append one record to a JSONL file.
    fn append_entry(path: &PathBuf, entry: &LlmAuditEntry) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let mut line =
            serde_json::to_string(entry)
                .map_err(std::io::Error::other)?;
        line.push('\n');
        file.write_all(line.as_bytes())
    }
}

impl LlmAuditLog for FileLlmAuditLog {
    fn log(&self, entry: LlmAuditEntry) {
        let _ = self.tx.send(AuditMsg::Write(entry));
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// LLM audit log configuration (loaded from environment variables).
#[derive(Debug, Clone)]
pub struct LlmAuditConfig {
    /// Whether audit logging is enabled.
    pub enabled: bool,
    /// Directory path for JSONL log files.
    pub path: PathBuf,
}

impl LlmAuditConfig {
    /// Load from environment variables.
    ///
    /// - `LLM_AUDIT_ENABLED` (default: `false`)
    /// - `LLM_AUDIT_PATH` (default: `~/.loom/data/llm_logs`)
    pub fn from_env() -> Self {
        let enabled = std::env::var("LLM_AUDIT_ENABLED")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let path = std::env::var("LLM_AUDIT_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env_config::home::loom_home()
                    .join("data")
                    .join("llm_logs")
            });

        Self { enabled, path }
    }

    /// Create a `FileLlmAuditLog` if enabled, otherwise return `None`.
    pub fn build(self) -> Option<FileLlmAuditLog> {
        if !self.enabled {
            return None;
        }
        Some(FileLlmAuditLog::new(self.path))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a standard audit entry with the given fields.
#[allow(clippy::too_many_arguments)]
pub fn build_audit_entry(
    thread_id: String,
    entry_type: &str,
    model: String,
    url: &str,
    duration_ms: u64,
    status: u16,
    request: LlmAuditRequest,
    response: Option<LlmAuditResponse>,
    error: Option<String>,
) -> LlmAuditEntry {
    LlmAuditEntry {
        id: uuid6().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        thread_id,
        entry_type: entry_type.to_string(),
        model,
        url: url.to_string(),
        duration_ms,
        status,
        request,
        response,
        error,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_audit_log_does_nothing() {
        let log = NoOpLlmAuditLog;
        let entry = LlmAuditEntry {
            id: "test".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            thread_id: "test-thread".into(),
            entry_type: "chat".into(),
            model: "gpt-4".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            duration_ms: 100,
            status: 200,
            request: LlmAuditRequest {
                messages: serde_json::json!([]),
                tools: None,
                parameters: LlmAuditRequestParams {
                    temperature: None,
                    stream: false,
                    tool_choice: None,
                },
            },
            response: None,
            error: None,
        };
        log.log(entry); // should not panic
    }

    #[tokio::test]
    async fn test_file_audit_log_writes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let log = FileLlmAuditLog::new(dir.path().to_path_buf());

        let entry = LlmAuditEntry {
            id: "entry-1".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            thread_id: "test-thread".into(),
            entry_type: "chat".into(),
            model: "gpt-4".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            duration_ms: 100,
            status: 200,
            request: LlmAuditRequest {
                messages: serde_json::json!([{"role": "user", "content": "hello"}]),
                tools: None,
                parameters: LlmAuditRequestParams {
                    temperature: Some(0.7),
                    stream: false,
                    tool_choice: None,
                },
            },
            response: Some(LlmAuditResponse {
                content: "Hi!".into(),
                reasoning_content: None,
                usage: Some(LlmAuditUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                tool_calls: vec![],
            }),
            error: None,
        };
        log.log(entry);

        // Give the background task time to write
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let file_path = dir.path().join("test-thread.jsonl");
        let content = std::fs::read_to_string(&file_path).unwrap();
        let parsed: LlmAuditEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.id, "entry-1");
        assert_eq!(parsed.thread_id, "test-thread");
    }

    #[tokio::test]
    async fn test_file_audit_log_appends() {
        let dir = tempfile::tempdir().unwrap();
        let log = FileLlmAuditLog::new(dir.path().to_path_buf());

        for i in 0..3 {
            let entry = LlmAuditEntry {
                id: format!("entry-{}", i),
                timestamp: "2025-01-01T00:00:00Z".into(),
                thread_id: "append-test".into(),
                entry_type: "chat".into(),
                model: "gpt-4".into(),
                url: "url".into(),
                duration_ms: 10,
                status: 200,
                request: LlmAuditRequest {
                    messages: serde_json::json!([]),
                    tools: None,
                    parameters: LlmAuditRequestParams {
                        temperature: None,
                        stream: false,
                        tool_choice: None,
                    },
                },
                response: None,
                error: None,
            };
            log.log(entry);
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let file_path = dir.path().join("append-test.jsonl");
        let content = std::fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_file_audit_log_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested").join("logs");
        let log = FileLlmAuditLog::new(nested.clone());

        let entry = LlmAuditEntry {
            id: "dir-test".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            thread_id: "dir-test".into(),
            entry_type: "chat".into(),
            model: "gpt-4".into(),
            url: "url".into(),
            duration_ms: 10,
            status: 200,
            request: LlmAuditRequest {
                messages: serde_json::json!([]),
                tools: None,
                parameters: LlmAuditRequestParams {
                    temperature: None,
                    stream: false,
                    tool_choice: None,
                },
            },
            response: None,
            error: None,
        };
        log.log(entry);

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(nested.join("dir-test.jsonl").exists());
    }

    #[test]
    fn test_audit_entry_serialization_roundtrip() {
        let entry = LlmAuditEntry {
            id: "roundtrip".into(),
            timestamp: "2025-06-01T12:00:00Z".into(),
            thread_id: "rt".into(),
            entry_type: "chat".into(),
            model: "gpt-4".into(),
            url: "url".into(),
            duration_ms: 50,
            status: 200,
            request: LlmAuditRequest {
                messages: serde_json::json!([{"role": "user", "content": "hi"}]),
                tools: None,
                parameters: LlmAuditRequestParams {
                    temperature: Some(0.5),
                    stream: false,
                    tool_choice: None,
                },
            },
            response: Some(LlmAuditResponse {
                content: "hello".into(),
                reasoning_content: None,
                usage: Some(LlmAuditUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                tool_calls: vec![],
            }),
            error: None,
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LlmAuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "roundtrip");
        assert_eq!(parsed.response.as_ref().unwrap().content, "hello");
    }

    #[test]
    fn test_audit_entry_skips_none_fields() {
        let entry = LlmAuditEntry {
            id: "skip-none".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            thread_id: "skip".into(),
            entry_type: "chat".into(),
            model: "gpt-4".into(),
            url: "url".into(),
            duration_ms: 0,
            status: 500,
            request: LlmAuditRequest {
                messages: serde_json::json!([]),
                tools: None,
                parameters: LlmAuditRequestParams {
                    temperature: None,
                    stream: false,
                    tool_choice: None,
                },
            },
            response: None,
            error: Some("Internal Server Error".into()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        // response field should not appear
        assert!(!json.contains(r#""response""#));
        // error field should appear
        assert!(json.contains(r#""error""#));
    }

    #[test]
    fn test_build_audit_entry_helper() {
        let request = LlmAuditRequest {
            messages: serde_json::json!([]),
            tools: None,
            parameters: LlmAuditRequestParams {
                temperature: None,
                stream: false,
                tool_choice: None,
            },
        };
        let entry = build_audit_entry(
            "helper-test".into(),
            "chat",
            "gpt-4".into(),
            "https://api.openai.com/v1/chat/completions",
            100,
            200,
            request,
            None,
            None,
        );
        assert_eq!(entry.thread_id, "helper-test");
        assert_eq!(entry.model, "gpt-4");
        assert!(entry.response.is_none());
        assert!(entry.error.is_none());
        assert!(!entry.id.is_empty());
    }
}
