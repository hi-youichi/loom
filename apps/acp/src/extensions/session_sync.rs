//! `_loomdesk.dev/session-sync/*` — ordered incremental session recovery.

use std::path::Path;
use std::sync::{Arc, RwLock, Weak};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::LoomAcpAgent;
use crate::extensions::question::QuestionHandler;
use crate::extensions::{ExtensionContext, ExtensionError, ExtensionHandler};
use crate::session::SessionId;
use crate::session_sync::{SessionSyncCursor, SessionSyncPromptState, SessionSyncService};

pub const DOMAIN: &str = "session-sync";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenParams {
    session_id: String,
    cwd: String,
    cursor: Option<SessionSyncCursor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloseParams {
    session_id: String,
}

pub struct SessionSyncHandler {
    agent: RwLock<Option<Weak<LoomAcpAgent>>>,
    service: RwLock<Option<Weak<SessionSyncService>>>,
    question: RwLock<Option<Weak<QuestionHandler>>>,
}

impl SessionSyncHandler {
    pub fn new() -> Self {
        Self {
            agent: RwLock::new(None),
            service: RwLock::new(None),
            question: RwLock::new(None),
        }
    }

    pub fn bind(
        &self,
        agent: &Arc<LoomAcpAgent>,
        service: &Arc<SessionSyncService>,
        question: &Arc<QuestionHandler>,
    ) {
        *self.agent.write().expect("session-sync agent slot") = Some(Arc::downgrade(agent));
        *self.service.write().expect("session-sync service slot") = Some(Arc::downgrade(service));
        *self.question.write().expect("session-sync question slot") =
            Some(Arc::downgrade(question));
    }

    fn resolve(&self) -> Result<(Arc<LoomAcpAgent>, Arc<SessionSyncService>), ExtensionError> {
        let agent = self
            .agent
            .read()
            .expect("session-sync agent slot")
            .as_ref()
            .and_then(Weak::upgrade);
        let service = self
            .service
            .read()
            .expect("session-sync service slot")
            .as_ref()
            .and_then(Weak::upgrade);
        agent.zip(service).ok_or_else(|| ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(
                "session-sync extension is not bound to the runtime yet".into(),
            )),
        })
    }
}

impl Default for SessionSyncHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for SessionSyncHandler {
    fn capabilities(&self) -> Value {
        json!({
            "version": 1,
            "notifications": [crate::session_sync::UPDATE_METHOD],
            "methods": {
                "open": {
                    "description": "Attach to an ordered session stream and replay updates after a cursor.",
                    "params": { "sessionId": "string", "cwd": "absolute path", "cursor": "{ streamId, seq } | null" }
                },
                "close": {
                    "description": "Stop session-sync notifications for this connection.",
                    "params": { "sessionId": "string" }
                }
            }
        })
    }

    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "open" => {
                let params: OpenParams = serde_json::from_value(params)
                    .map_err(|error| ExtensionError::invalid_params(error.to_string()))?;
                let (agent, service) = self.resolve()?;
                let session_id = SessionId::new(params.session_id);
                let session = agent
                    .sessions()
                    .get(&session_id)
                    .ok_or_else(|| ExtensionError::not_found("session was not found"))?;
                if session.owner_principal != ctx.principal {
                    return Err(ExtensionError::not_found("session was not found"));
                }
                ensure_same_cwd(&params.cwd, session.working_directory.as_deref())?;
                let result = service
                    .open(
                        session_id.clone(),
                        ctx.connection_id.clone(),
                        params.cursor,
                        if agent.sessions().has_active_prompt(&session_id) {
                            SessionSyncPromptState::Running
                        } else {
                            SessionSyncPromptState::Idle
                        },
                    )
                    .await
                    .map_err(|error| ExtensionError {
                        code: -32603,
                        message: "internal_error".into(),
                        data: Some(Value::String(error.to_string())),
                    })?;
                let question = {
                    self.question
                        .read()
                        .expect("session-sync question slot")
                        .as_ref()
                        .and_then(Weak::upgrade)
                };
                if let Some(question) = question {
                    question
                        .rebind_session(&result.session_id, &ctx.connection_id)
                        .await?;
                }
                serde_json::to_value(result).map_err(|error| ExtensionError {
                    code: -32603,
                    message: "internal_error".into(),
                    data: Some(Value::String(error.to_string())),
                })
            }
            "close" => {
                let params: CloseParams = serde_json::from_value(params)
                    .map_err(|error| ExtensionError::invalid_params(error.to_string()))?;
                let (_, service) = self.resolve()?;
                service.close_session(&SessionId::new(params.session_id), &ctx.connection_id);
                Ok(json!({ "closed": true }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }
}

fn ensure_same_cwd(requested: &str, session: Option<&Path>) -> Result<(), ExtensionError> {
    let requested = std::fs::canonicalize(requested)
        .map_err(|error| ExtensionError::invalid_params(format!("cwd is invalid: {error}")))?;
    let session =
        session.ok_or_else(|| ExtensionError::conflict("session has no working directory"))?;
    let session = std::fs::canonicalize(session).map_err(|error| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "session working directory is unavailable: {error}"
        ))),
    })?;
    if requested != session {
        return Err(ExtensionError::forbidden(
            "cwd does not match the session working directory",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runtime_handler_opens_owned_session_and_hides_cross_owner_session() {
        let runtime = crate::runtime::AcpRuntime::new().expect("runtime");
        let cwd = std::env::current_dir().expect("cwd");
        let session_id = runtime
            .agent
            .sessions()
            .create_owned(Some(cwd.clone()), "owner-a");
        let opened = runtime.open_connection("owner-a".into());
        let active_prompt = runtime
            .agent
            .sessions()
            .begin_prompt(&session_id)
            .expect("active prompt");
        let context = ExtensionContext {
            session_id: None,
            principal: "owner-a".into(),
            connection_id: opened.connection.id.clone(),
            working_directory: Some(cwd.clone()),
            client_capabilities: crate::client_capabilities::ClientCapabilitiesInfo::default(),
        };

        let result = runtime
            .extensions
            .dispatch(
                "_loomdesk.dev/session-sync/open",
                json!({ "sessionId": session_id.to_string(), "cwd": cwd, "cursor": null }),
                &context,
            )
            .await
            .expect("open");
        assert_eq!(result["mode"], "reset_required");
        assert_eq!(result["promptState"], "running");
        assert!(runtime
            .bindings
            .is_connection_bound_to_session(&session_id, &opened.connection.id));

        let cross_owner = ExtensionContext {
            principal: "owner-b".into(),
            ..context
        };
        let error = runtime
            .extensions
            .dispatch(
                "_loomdesk.dev/session-sync/open",
                json!({ "sessionId": session_id.to_string(), "cwd": cwd, "cursor": null }),
                &cross_owner,
            )
            .await
            .expect_err("cross owner must fail");
        assert_eq!(error.code, -32003);
        runtime
            .agent
            .sessions()
            .finish_prompt(&session_id, active_prompt.generation());
    }
}
