//! # Protocol module
//!
//! - **WebSocket** (this file): CLI remote mode request/response types. Aligned with [DESIGN_CLI_REMOTE_MODE]
//!   §2.3 (requests) and §2.4 (responses), and with [EXPORT_SPEC] / [USER_GUIDELINE].
//! - **Stream** ([`stream`]): Streaming output protocol (type + payload, envelope) per [protocol_spec].
//!
//! [DESIGN_CLI_REMOTE_MODE]: https://github.com/loom/loom/blob/main/docs/DESIGN_CLI_REMOTE_MODE.md
//! [EXPORT_SPEC]: https://github.com/loom/loom/blob/main/docs/EXPORT_SPEC.md
//! [USER_GUIDELINE]: https://github.com/loom/loom/blob/main/docs/USER_GUIDELINE.md
//! [protocol_spec]: https://github.com/loom/loom/blob/main/docs/protocol_spec.md

pub mod envelope_state;
pub mod stream;

pub use envelope_state::EnvelopeState;

use crate::llm::LlmUsage;
use crate::tool_source::ToolSpec;
use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------------
// Requests (client → server)
// -----------------------------------------------------------------------------

/// Agent type for run requests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    React,
    Dup,
    Tot,
    Got,
}

/// Run request: execute one Agent run (streaming events + final RunEnd).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRequest {
    pub id: String,
    pub message: String,
    pub agent: AgentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub got_adaptive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
}

/// Tools list request: list all tools.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolsListRequest {
    pub id: String,
}

/// Output format for tool_show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolShowOutput {
    Json,
    Yaml,
}

/// Tool show request: get a single tool definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolShowRequest {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ToolShowOutput>,
}

/// Ping request: health / keepalive.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PingRequest {
    pub id: String,
}

/// Client-to-server request envelope.
///
/// Each variant maps to a JSON object with `"type": "<variant_name>"`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientRequest {
    Run(RunRequest),
    ToolsList(ToolsListRequest),
    ToolShow(ToolShowRequest),
    Ping(PingRequest),
}

// -----------------------------------------------------------------------------
// Responses (server → client)
// -----------------------------------------------------------------------------

/// One stream event (format A) for a run; server sends one per stream event before RunEnd.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunStreamEventResponse {
    pub id: String,
    pub event: serde_json::Value,
}

/// RunEnd: final reply for one run (format B from EXPORT_SPEC).
/// Optional envelope fields align with protocol_spec §5 (reply message).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEndResponse {
    pub id: String,
    pub reply: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_usage: Option<LlmUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<u64>,
}

/// Tools list response (USER_GUIDELINE §4.2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub id: String,
    pub tools: Vec<ToolSpec>,
}

/// Tool show response: either `tool` (JSON) or `tool_yaml` (YAML string).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolShowResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_yaml: Option<String>,
}

/// Pong: response to ping.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PongResponse {
    pub id: String,
}

/// Error response for any failed request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub error: String,
}

/// Server-to-client response envelope.
///
/// Each variant maps to a JSON object with `"type": "<variant_name>"`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerResponse {
    RunStreamEvent(RunStreamEventResponse),
    RunEnd(RunEndResponse),
    ToolsList(ToolsListResponse),
    ToolShow(ToolShowResponse),
    Pong(PongResponse),
    Error(ErrorResponse),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LlmUsage;

    #[test]
    fn request_run_roundtrip() {
        let req = ClientRequest::Run(RunRequest {
            id: "abc-123".to_string(),
            message: "hello".to_string(),
            agent: AgentType::React,
            thread_id: Some("t1".to_string()),
            working_folder: None,
            got_adaptive: None,
            verbose: Some(true),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"run\""));
        assert!(json.contains("\"id\":\"abc-123\""));
        assert!(json.contains("\"message\":\"hello\""));
        assert!(json.contains("\"agent\":\"react\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::Run(_)));
    }

    #[test]
    fn request_tools_list_roundtrip() {
        let req = ClientRequest::ToolsList(ToolsListRequest {
            id: "req-1".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"tools_list","id":"req-1"}"#);
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::ToolsList(_)));
    }

    #[test]
    fn response_run_end_roundtrip() {
        let resp = ServerResponse::RunEnd(RunEndResponse {
            id: "run-1".to_string(),
            reply: "Hi there".to_string(),
            usage: Some(LlmUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            total_usage: None,
            session_id: None,
            node_id: None,
            event_id: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"run_end\""));
        assert!(json.contains("\"reply\":\"Hi there\""));
        assert!(json.contains("\"prompt_tokens\":10"));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::RunEnd(_)));
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = ServerResponse::Error(ErrorResponse {
            id: Some("req-x".to_string()),
            error: "something failed".to_string(),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"error\":\"something failed\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::Error(_)));
    }
}
