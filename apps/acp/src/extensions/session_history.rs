//! `_loomdesk.dev/session-history/*` — backward paging of session history
//! that `session/load` tail-truncated on replay.
//!
//! * `info`: read-only probe (`hasMore` for the "load earlier" UI affordance)
//! * `page`: pull the previous slice of checkpoint messages as ordered ACP
//!   `SessionUpdate` batches (same shapes the client already applies for
//!   `session/update`), advancing the per-session cursor.
//!
//! Spec: docs/acp-spec/extensions/36-session-history.md

use std::sync::{Arc, RwLock, Weak};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::LoomAcpAgent;
use crate::extensions::{ExtensionContext, ExtensionError, ExtensionHandler};

pub const DOMAIN: &str = "session-history";

/// Default and maximum page size; mirrors the server-side clamp in
/// `LoomAcpAgent::session_history_page`.
const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 200;

/// Handler for the `session-history` domain. The agent reference is
/// late-bound: extension registration happens before agent construction
/// (the registry is injected into the agent), so `AcpRuntime` calls
/// [`bind`](Self::bind) once the agent exists.
pub struct SessionHistoryHandler {
    agent: RwLock<Option<Weak<LoomAcpAgent>>>,
}

impl SessionHistoryHandler {
    pub fn new() -> Self {
        Self {
            agent: RwLock::new(None),
        }
    }

    pub fn bind(&self, agent: &Arc<LoomAcpAgent>) {
        *self.agent.write().expect("session-history agent slot") = Some(Arc::downgrade(agent));
    }

    fn resolve(&self) -> Result<Arc<LoomAcpAgent>, ExtensionError> {
        self.agent
            .read()
            .expect("session-history agent slot")
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(
                    "session-history extension is not bound to an agent yet".into(),
                )),
            })
    }
}

impl Default for SessionHistoryHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_session_id(params: &Value, ctx: &ExtensionContext) -> Result<String, ExtensionError> {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| ctx.session_id.clone())
        .ok_or_else(|| ExtensionError::invalid_params("sessionId is required"))
}

#[async_trait]
impl ExtensionHandler for SessionHistoryHandler {
    fn capabilities(&self) -> Value {
        json!({
            "methods": {
                "info": {
                    "description": "Read-only probe: totalMessages / loadedStartIndex / hasMore for the load-earlier affordance.",
                    "params": { "sessionId": "string (optional if bound)" },
                    "returns": { "sessionId": "string", "totalMessages": "number", "loadedStartIndex": "number", "hasMore": "boolean" }
                },
                "page": {
                    "description": "Pull the previous slice of checkpoint history as ordered ACP SessionUpdate batches; advances the per-session cursor.",
                    "params": { "sessionId": "string (optional if bound)", "limit": "number (default 50, max 200)" },
                    "returns": { "sessionId": "string", "totalMessages": "number", "hasMore": "boolean", "messages": "[{ index, role, updates: [SessionUpdate] }]" }
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
        if !matches!(method, "info" | "page") {
            return Err(ExtensionError::method_not_found());
        }
        let session_id = resolve_session_id(&params, ctx)?;
        let agent = self.resolve()?;
        match method {
            "info" => {
                let info = agent
                    .session_history_info(&session_id)
                    .await
                    .map_err(ExtensionError::not_found)?;
                Ok(json!({
                    "sessionId": info.session_id,
                    "totalMessages": info.total_messages,
                    "loadedStartIndex": info.loaded_start_index,
                    "hasMore": info.has_more,
                }))
            }
            "page" => {
                let limit = params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(DEFAULT_PAGE_LIMIT)
                    .clamp(1, MAX_PAGE_LIMIT);
                let page = agent
                    .session_history_page(&session_id, limit)
                    .await
                    .map_err(ExtensionError::not_found)?;
                let mut messages = Vec::with_capacity(page.messages.len());
                for entry in &page.messages {
                    let mut updates = Vec::with_capacity(entry.updates.len());
                    for update in &entry.updates {
                        updates.push(serde_json::to_value(update).map_err(|e| ExtensionError {
                            code: -32603,
                            message: "internal_error".into(),
                            data: Some(Value::String(format!(
                                "failed to serialize session update: {e}"
                            ))),
                        })?);
                    }
                    messages.push(json!({
                        "index": entry.index,
                        "role": entry.role,
                        "updates": updates,
                    }));
                }
                Ok(json!({
                    "sessionId": page.session_id,
                    "totalMessages": page.total_messages,
                    "hasMore": page.has_more,
                    "messages": messages,
                }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_without_session() -> ExtensionContext {
        ExtensionContext {
            session_id: None,
            principal: "test".into(),
            connection_id: "conn-1".into(),
            working_directory: None,
            client_capabilities: crate::extensions::ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn unbound_agent_returns_internal_error() {
        let handler = SessionHistoryHandler::new();
        let err = handler
            .handle("page", json!({ "sessionId": "s1" }), &ctx_without_session())
            .await
            .unwrap_err();
        assert_eq!(err.code, -32603);
    }

    #[tokio::test]
    async fn missing_session_id_is_invalid_params() {
        let handler = SessionHistoryHandler::new();
        let err = handler
            .handle("info", json!({}), &ctx_without_session())
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn unknown_method_is_not_found() {
        let handler = SessionHistoryHandler::new();
        let err = handler
            .handle(
                "bogus",
                json!({ "sessionId": "s1" }),
                &ctx_without_session(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }
}
