//! WebSocket response types (server → client).

use serde::{Deserialize, Serialize};

use crate::llm::LlmUsage;
use crate::tool_source::ToolSpec;
use stream_event::ProtocolEvent;

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

    /// Deserializes a JSON object into a typed event envelope.
    pub fn from_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }
}

/// Protocol event stream response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunStreamEventResponse {
    pub id: String,
    pub event: ProtocolEventEnvelope,
}

/// Run end response: final event after a successful run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEndResponse {
    pub id: String,
    pub reply: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
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

/// Tool list response: all available tools.
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

/// One message in user messages list (role + content).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessageItem {
    pub role: String,
    pub content: String,
}

/// User messages list response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessagesResponse {
    pub id: String,
    pub thread_id: String,
    pub messages: Vec<UserMessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// Agent summary information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: AgentSource,
}

/// Agent source type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSource {
    BuiltIn,
    Project,
    User,
}

/// Agent list response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub id: String,
    pub agents: Vec<AgentSummary>,
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
    UserMessages(UserMessagesResponse),
    AgentList(AgentListResponse),
    WorkspaceList(WorkspaceListResponse),
    WorkspaceCreate(WorkspaceCreateResponse),
    WorkspaceThreadList(WorkspaceThreadListResponse),
    WorkspaceThreadAdd(WorkspaceThreadAddResponse),
    WorkspaceThreadRemove(WorkspaceThreadRemoveResponse),
    Pong(PongResponse),
    Error(ErrorResponse),
}
// -----------------------------------------------------------------------------
// Workspace responses
// -----------------------------------------------------------------------------
/// Workspace metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub created_at_ms: i64,
}
/// Workspace list response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceListResponse {
    pub id: String,
    pub workspaces: Vec<WorkspaceMeta>,
}
/// Workspace create response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceCreateResponse {
    pub id: String,
    pub workspace_id: String,
}
/// Thread in workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadInWorkspace {
    pub thread_id: String,
    pub created_at_ms: i64,
}
/// Workspace thread list response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadListResponse {
    pub id: String,
    pub workspace_id: String,
    pub threads: Vec<ThreadInWorkspace>,
}
/// Workspace thread add response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadAddResponse {
    pub id: String,
    pub workspace_id: String,
    pub thread_id: String,
}
/// Workspace thread remove response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadRemoveResponse {
    pub id: String,
    pub workspace_id: String,
    pub thread_id: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_source::ToolSpec;
    use crate::LlmUsage;

    #[test]
    fn response_run_end_roundtrip() {
        let resp = ServerResponse::RunEnd(RunEndResponse {
            id: "req-1".to_string(),
            reply: "hello".to_string(),
            reasoning_content: None,
            usage: Some(LlmUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
            total_usage: None,
            session_id: None,
            node_id: None,
            event_id: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"run_end\""));
        assert!(json.contains("\"id\":\"req-1\""));
        assert!(json.contains("\"reply\":\"hello\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::RunEnd(_)));
    }

    #[test]
    fn response_tools_list_roundtrip() {
        let resp = ServerResponse::ToolsList(ToolsListResponse {
            id: "req-1".to_string(),
            tools: vec![
                ToolSpec {
                    name: "test_tool".to_string(),
                    description: Some("A test tool".to_string()),
                    input_schema: serde_json::json!({}),
                    output_hint: None,
                },
            ],
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"tools_list\""));
        assert!(json.contains("\"id\":\"req-1\""));
        assert!(json.contains("\"name\":\"test_tool\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::ToolsList(_)));
    }

    #[test]
    fn response_tool_show_roundtrip() {
        let resp = ServerResponse::ToolShow(ToolShowResponse {
            id: "req-show".to_string(),
            tool: Some(serde_json::json!({
                "name": "read",
                "description": "Read file",
            })),
            tool_yaml: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"tool_show\""));
        assert!(json.contains("\"id\":\"req-show\""));
        assert!(json.contains("\"name\":\"read\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        if let ServerResponse::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-show");
            assert!(r.tool.is_some());
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
        assert!(json.contains("\"id\":\"req-show-yaml\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        if let ServerResponse::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-show-yaml");
            assert!(r.tool.is_none());
            assert_eq!(
                r.tool_yaml.as_deref(),
                Some("name: read\ndescription: Read file\n")
            );
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

    #[test]
    fn response_agent_list_roundtrip() {
        let resp = ServerResponse::AgentList(AgentListResponse {
            id: "req-agents".to_string(),
            agents: vec![
                AgentSummary {
                    name: "dev".to_string(),
                    description: Some("Developer assistant".to_string()),
                    source: AgentSource::BuiltIn,
                },
                AgentSummary {
                    name: "my-agent".to_string(),
                    description: None,
                    source: AgentSource::Project,
                },
            ],
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"agent_list\""));
        assert!(json.contains("\"id\":\"req-agents\""));
        assert!(json.contains("\"name\":\"dev\""));
        assert!(json.contains("\"source\":\"builtin\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::AgentList(_)));
    }

    #[test]
    fn response_workspace_list_roundtrip() {
        let resp = ServerResponse::WorkspaceList(WorkspaceListResponse {
            id: "req-wl".to_string(),
            workspaces: vec![WorkspaceMeta {
                id: "ws-1".to_string(),
                name: Some("project-alpha".to_string()),
                created_at_ms: 1712649600000,
            }],
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"workspace_list\""));
        assert!(json.contains("\"name\":\"project-alpha\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::WorkspaceList(_)));
    }

    #[test]
    fn response_workspace_create_roundtrip() {
        let resp = ServerResponse::WorkspaceCreate(WorkspaceCreateResponse {
            id: "req-wc".to_string(),
            workspace_id: "ws-2".to_string(),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"workspace_create\""));
        assert!(json.contains("\"workspace_id\":\"ws-2\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::WorkspaceCreate(_)));
    }

    #[test]
    fn response_workspace_thread_list_roundtrip() {
        let resp = ServerResponse::WorkspaceThreadList(WorkspaceThreadListResponse {
            id: "req-wtl".to_string(),
            workspace_id: "ws-1".to_string(),
            threads: vec![ThreadInWorkspace {
                thread_id: "t-1".to_string(),
                created_at_ms: 1712649600000,
            }],
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_list\""));
        assert!(json.contains("\"thread_id\":\"t-1\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::WorkspaceThreadList(_)));
    }

    #[test]
    fn response_workspace_thread_add_roundtrip() {
        let resp = ServerResponse::WorkspaceThreadAdd(WorkspaceThreadAddResponse {
            id: "req-wta".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: "t-1".to_string(),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_add\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::WorkspaceThreadAdd(_)));
    }

    #[test]
    fn response_workspace_thread_remove_roundtrip() {
        let resp = ServerResponse::WorkspaceThreadRemove(WorkspaceThreadRemoveResponse {
            id: "req-wtr".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: "t-1".to_string(),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_remove\""));
        let parsed: ServerResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ServerResponse::WorkspaceThreadRemove(_)));
    }
}
