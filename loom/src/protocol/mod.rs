//! # Protocol module
//!
//! - **WebSocket** (this file): CLI remote mode request/response types. Aligned with [DESIGN_CLI_REMOTE_MODE]
//!   §2.3 (requests) and §2.4 (responses), and with [EXPORT_SPEC] / [USER_GUIDELINE].
//! - **Stream** ([`stream`]): Streaming output protocol (type + payload, envelope) per [protocol_spec].
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                           protocol (this crate)                              │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                              │
//! │   Request types (client → server)          Response types (server → client)  │
//! │   ─────────────────────────────           ───────────────────────────────   │
//! │   ClientRequest:                           ServerResponse:                   │
//! │     Run(RunRequest)                          RunStreamEvent(RunStreamEventResponse)  │
//! │     ToolsList(ToolsListRequest)              RunEnd(RunEndResponse)          │
//! │     ToolShow(ToolShowRequest)                ToolsList(ToolsListResponse)     │
//! │     Ping(PingRequest)                        ToolShow(ToolShowResponse)       │
//! │                                              Pong(PongResponse)              │
//! │                                              Error(ErrorResponse)             │
//! │                                                                              │
//! │   ┌──────────────┐    JSON (type + payload)    ┌──────────────┐             │
//! │   │    Client    │ ─────────────────────────►  │    Server    │             │
//! │   │  (WebSocket) │  ◄───────────────────────── │  (WebSocket) │             │
//! │   └──────────────┘                              └──────┬───────┘             │
//! │        │                                                │                    │
//! │        ▼                                                ▼                    │
//! │   ┌─────────────────────────────────────────────────────────────────────┐  │
//! │   │  envelope_state    stream (stream_event bridge)                       │  │
//! │   │  EnvelopeState     StreamEvent<S> ──► ProtocolEvent ──► JSON envelope │  │
//! │   │                    ProtocolEventEnvelope, RunStreamEventResponse     │  │
//! │   └─────────────────────────────────────────────────────────────────────┘  │
//! │                                                                              │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! [DESIGN_CLI_REMOTE_MODE]: https://github.com/loom/loom/blob/main/docs/DESIGN_CLI_REMOTE_MODE.md
//! [EXPORT_SPEC]: https://github.com/loom/loom/blob/main/docs/EXPORT_SPEC.md
//! [USER_GUIDELINE]: https://github.com/loom/loom/blob/main/docs/USER_GUIDELINE.md
//! [protocol_spec]: https://github.com/loom/loom/blob/main/docs/protocol_spec.md

pub mod envelope_state;
pub mod stream;

pub use envelope_state::EnvelopeState;
pub use stream_event::ProtocolEvent;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub message: String,
    pub agent: AgentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// When set with thread_id, the run's thread is associated with this workspace (see loom-workspace).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
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

/// Typed protocol stream event payload with optional envelope fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolEventEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<u64>,
    #[serde(flatten)]
    pub event: ProtocolEvent,
}

impl ProtocolEventEnvelope {
    /// Serializes the typed event envelope into a JSON object.
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    /// Parses a typed event envelope from a JSON object.
    pub fn from_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }
}

/// One stream event (type-safe protocol event + optional envelope) for a run;
/// server sends one per stream event before RunEnd.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunStreamEventResponse {
    pub id: String,
    pub event: ProtocolEventEnvelope,
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
    use crate::tool_source::ToolSpec;
    use crate::LlmUsage;

    #[test]
    fn request_run_roundtrip() {
        let req = ClientRequest::Run(RunRequest {
            id: Some("abc-123".to_string()),
            message: "hello".to_string(),
            agent: AgentType::React,
            thread_id: Some("t1".to_string()),
            workspace_id: None,
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
            working_folder: None,
            thread_id: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"tools_list","id":"req-1"}"#);
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::ToolsList(_)));
    }

    #[test]
    fn request_tools_list_with_context_roundtrip() {
        let req = ClientRequest::ToolsList(ToolsListRequest {
            id: "req-2".to_string(),
            working_folder: Some("/tmp/proj".to_string()),
            thread_id: Some("t1".to_string()),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"working_folder\":\"/tmp/proj\""));
        assert!(json.contains("\"thread_id\":\"t1\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        if let ClientRequest::ToolsList(r) = parsed {
            assert_eq!(r.working_folder.as_deref(), Some("/tmp/proj"));
            assert_eq!(r.thread_id.as_deref(), Some("t1"));
        } else {
            panic!("expected ToolsList");
        }
    }

    #[test]
    fn request_tools_list_backward_compat_parses_legacy_json() {
        // Old clients send JSON without working_folder/thread_id; server must parse.
        let json = r#"{"type":"tools_list","id":"req-legacy"}"#;
        let parsed: ClientRequest = serde_json::from_str(json).unwrap();
        if let ClientRequest::ToolsList(r) = parsed {
            assert_eq!(r.id, "req-legacy");
            assert_eq!(r.working_folder, None);
            assert_eq!(r.thread_id, None);
        } else {
            panic!("expected ToolsList");
        }
    }

    #[test]
    fn request_tool_show_roundtrip() {
        let req = ClientRequest::ToolShow(ToolShowRequest {
            id: "req-ts".to_string(),
            name: "read".to_string(),
            output: None,
            working_folder: None,
            thread_id: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"tool_show\""));
        assert!(json.contains("\"id\":\"req-ts\""));
        assert!(json.contains("\"name\":\"read\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        if let ClientRequest::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-ts");
            assert_eq!(r.name, "read");
        } else {
            panic!("expected ToolShow");
        }
    }

    #[test]
    fn request_tool_show_with_options_roundtrip() {
        let req = ClientRequest::ToolShow(ToolShowRequest {
            id: "req-ts2".to_string(),
            name: "write".to_string(),
            output: Some(ToolShowOutput::Yaml),
            working_folder: Some("/tmp".to_string()),
            thread_id: Some("t1".to_string()),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"output\":\"yaml\""));
        assert!(json.contains("\"working_folder\":\"/tmp\""));
        assert!(json.contains("\"thread_id\":\"t1\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        if let ClientRequest::ToolShow(r) = parsed {
            assert_eq!(r.output, Some(ToolShowOutput::Yaml));
            assert_eq!(r.working_folder.as_deref(), Some("/tmp"));
        } else {
            panic!("expected ToolShow");
        }
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
    fn response_run_stream_event_roundtrip() {
        let resp = ServerResponse::RunStreamEvent(RunStreamEventResponse {
            id: "run-1".to_string(),
            event: ProtocolEventEnvelope {
                session_id: Some("sess-1".to_string()),
                node_id: Some("run-think-0".to_string()),
                event_id: Some(1),
                event: ProtocolEvent::NodeEnter {
                    id: "think".to_string(),
                },
            },
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"run_stream_event\""));
        assert!(json.contains("\"session_id\":\"sess-1\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            ServerResponse::RunStreamEvent(r) => {
                assert_eq!(r.event.session_id.as_deref(), Some("sess-1"));
                match r.event.event {
                    ProtocolEvent::NodeEnter { id } => assert_eq!(id, "think"),
                    _ => panic!("expected node_enter"),
                }
            }
            _ => panic!("expected RunStreamEvent"),
        }
    }

    #[test]
    fn response_tools_list_roundtrip() {
        let resp = ServerResponse::ToolsList(ToolsListResponse {
            id: "req-list".to_string(),
            tools: vec![
                ToolSpec {
                    name: "read".to_string(),
                    description: Some("Read a file".to_string()),
                    input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}}),
                },
                ToolSpec {
                    name: "write".to_string(),
                    description: None,
                    input_schema: serde_json::json!({"type":"object"}),
                },
            ],
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"tools_list\""));
        assert!(json.contains("\"id\":\"req-list\""));
        assert!(json.contains("\"name\":\"read\""));
        assert!(json.contains("\"name\":\"write\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        if let ServerResponse::ToolsList(r) = parsed {
            assert_eq!(r.id, "req-list");
            assert_eq!(r.tools.len(), 2);
            assert_eq!(r.tools[0].name, "read");
            assert_eq!(r.tools[1].name, "write");
        } else {
            panic!("expected ToolsList");
        }
    }

    #[test]
    fn response_tool_show_roundtrip() {
        let resp = ServerResponse::ToolShow(ToolShowResponse {
            id: "req-show".to_string(),
            tool: Some(serde_json::json!({"name":"read","description":"Read file","input_schema":{"type":"object"}})),
            tool_yaml: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"tool_show\""));
        assert!(json.contains("\"id\":\"req-show\""));
        assert!(json.contains("\"tool\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        if let ServerResponse::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-show");
            assert!(r.tool.is_some());
            assert!(r.tool_yaml.is_none());
        } else {
            panic!("expected ToolShow");
        }
    }

    #[test]
    fn response_tool_show_yaml_roundtrip() {
        let resp = ServerResponse::ToolShow(ToolShowResponse {
            id: "req-show-yaml".to_string(),
            tool: None,
            tool_yaml: Some("name: read\ndescription: Read file\n".to_string()),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"tool_show\""));
        assert!(json.contains("\"tool_yaml\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        if let ServerResponse::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-show-yaml");
            assert!(r.tool.is_none());
            assert_eq!(r.tool_yaml.as_deref(), Some("name: read\ndescription: Read file\n"));
        } else {
            panic!("expected ToolShow");
        }
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
