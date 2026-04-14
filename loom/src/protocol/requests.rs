//! WebSocket request types (client → server).

use crate::message::UserContent;
use serde::{Deserialize, Serialize};

/// Agent type for run requests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    React,
    Dup,
    Tot,
    Got,
}
/// Agent identifier - can be a builtin AgentType or a custom agent name
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentIdentifier {
    /// Builtin agent type
    Type(AgentType),
    /// Custom agent profile name (dev, assistant, ask, etc.)
    Name(String),
}

impl std::fmt::Display for AgentIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentIdentifier::Type(t) => write!(f, "{:?}", t),
            AgentIdentifier::Name(n) => write!(f, "{}", n),
        }
    }
}

/// Run request: execute one Agent run (streaming events + final RunEnd).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub message: UserContent,
    pub agent: AgentIdentifier,
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
    /// Model to use for this run (e.g. "openai/gpt-4o", "gpt-4o").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl RunRequest {
    /// Validates that the message modalities are supported by the given model.
    pub fn validate_modalities(
        &self,
        model: &model_spec_core::spec::Model,
    ) -> Result<(), crate::AgentError> {
        use crate::message::UserContent;

        let UserContent::Multimodal(parts) = &self.message else {
            return Ok(());
        };

        let modalities = &model.modalities;
        if modalities.input.is_empty() {
            return Ok(()); // 无法验证，保守放行
        }

        for part in parts {
            let required = part.modality();
            if required != model_spec_core::spec::ModalityType::Text
                && !modalities.input.contains(&required)
            {
                return Err(crate::AgentError::ExecutionFailed(format!(
                    "model does not support {:?} input",
                    required
                )));
            }
        }
        Ok(())
    }
}

/// Tool list request: list all available tools.
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

/// List models request: list available models.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListModelsRequest {
    pub id: String,
}

/// Set model request: set model for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetModelRequest {
    pub id: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// User messages list request: list stored messages for a thread (pagination).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessagesRequest {
    pub id: String,
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Filter for agent list by source type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSourceFilter {
    BuiltIn,
    Project,
    User,
}

/// Agent list request: list available agent profiles.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentListRequest {
    pub id: String,
    /// Optional filter by agent source (built-in, project, user).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filter: Option<AgentSourceFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// Cancel run request: cancel a running agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelRunRequest {
    pub id: String,
    pub run_id: String,
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
    UserMessages(UserMessagesRequest),
    AgentList(AgentListRequest),
    WorkspaceList(WorkspaceListRequest),
    WorkspaceCreate(WorkspaceCreateRequest),
    WorkspaceThreadList(WorkspaceThreadListRequest),
    WorkspaceThreadAdd(WorkspaceThreadAddRequest),
    WorkspaceThreadRemove(WorkspaceThreadRemoveRequest),
    Ping(PingRequest),
    ListModels(ListModelsRequest),
    SetModel(SetModelRequest),
    CancelRun(CancelRunRequest),
}
// -----------------------------------------------------------------------------
// Workspace requests
// -----------------------------------------------------------------------------

/// Workspace list request: list all workspaces.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceListRequest {
    pub id: String,
}

/// Workspace create request: create a new workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceCreateRequest {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Workspace thread list request: list threads in a workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadListRequest {
    pub id: String,
    pub workspace_id: String,
}

/// Workspace thread add request: associate a thread with a workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadAddRequest {
    pub id: String,
    pub workspace_id: String,
    pub thread_id: String,
}

/// Workspace thread remove request: disassociate a thread from a workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceThreadRemoveRequest {
    pub id: String,
    pub workspace_id: String,
    pub thread_id: String,
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_run_roundtrip() {
        let req = ClientRequest::Run(RunRequest {
            id: Some("abc-123".to_string()),
            message: crate::message::UserContent::Text("hello".to_string()),
            agent: AgentIdentifier::Type(AgentType::React),
            thread_id: Some("t1".to_string()),
            workspace_id: None,
            working_folder: None,
            got_adaptive: None,
            verbose: Some(true),
            model: None,
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
            thread_id: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"output\":\"yaml\""));
        assert!(json.contains("\"working_folder\":\"/tmp\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        if let ClientRequest::ToolShow(r) = parsed {
            assert_eq!(r.id, "req-ts2");
            assert_eq!(r.output, Some(ToolShowOutput::Yaml));
        } else {
            panic!("expected ToolShow");
        }
    }

    #[test]
    fn request_agent_list_roundtrip() {
        let req = ClientRequest::AgentList(AgentListRequest {
            id: "req-agents".to_string(),
            source_filter: Some(AgentSourceFilter::BuiltIn),
            working_folder: None,
            thread_id: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"agent_list\""));
        assert!(json.contains("\"id\":\"req-agents\""));
        assert!(json.contains("\"source_filter\":\"builtin\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::AgentList(_)));
    }

    #[test]
    fn request_workspace_list_roundtrip() {
        let req = ClientRequest::WorkspaceList(WorkspaceListRequest {
            id: "req-wl".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"workspace_list\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::WorkspaceList(_)));
    }

    #[test]
    fn request_workspace_create_roundtrip() {
        let req = ClientRequest::WorkspaceCreate(WorkspaceCreateRequest {
            id: "req-wc".to_string(),
            name: Some("project-alpha".to_string()),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"workspace_create\""));
        assert!(json.contains("\"name\":\"project-alpha\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::WorkspaceCreate(_)));
    }

    #[test]
    fn request_workspace_thread_list_roundtrip() {
        let req = ClientRequest::WorkspaceThreadList(WorkspaceThreadListRequest {
            id: "req-wtl".to_string(),
            workspace_id: "ws-1".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_list\""));
        assert!(json.contains("\"workspace_id\":\"ws-1\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::WorkspaceThreadList(_)));
    }

    #[test]
    fn request_workspace_thread_add_roundtrip() {
        let req = ClientRequest::WorkspaceThreadAdd(WorkspaceThreadAddRequest {
            id: "req-wta".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: "t-1".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_add\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::WorkspaceThreadAdd(_)));
    }

    #[test]
    fn request_workspace_thread_remove_roundtrip() {
        let req = ClientRequest::WorkspaceThreadRemove(WorkspaceThreadRemoveRequest {
            id: "req-wtr".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: "t-1".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"workspace_thread_remove\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::WorkspaceThreadRemove(_)));
    }

    #[test]
    fn request_list_models_roundtrip() {
        let req = ClientRequest::ListModels(ListModelsRequest {
            id: "req-lm".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"list_models\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::ListModels(_)));
    }

    #[test]
    fn request_set_model_roundtrip() {
        let req = ClientRequest::SetModel(SetModelRequest {
            id: "req-sm".to_string(),
            model_id: "gpt-4".to_string(),
            session_id: Some("session-123".to_string()),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"set_model\""));
        assert!(json.contains("\"model_id\":\"gpt-4\""));
        let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ClientRequest::SetModel(_)));
    }
}
