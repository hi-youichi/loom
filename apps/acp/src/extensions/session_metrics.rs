//! Runtime counters for SessionIndex migration observability.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::session_list::SessionListHandler;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

/// Read-only metrics used to decide when the legacy list alias can be retired.
pub struct SessionMetricsHandler {
    session_list: Arc<SessionListHandler>,
}

impl SessionMetricsHandler {
    pub fn new(session_list: Arc<SessionListHandler>) -> Self {
        Self { session_list }
    }

    fn require_principal(ctx: &ExtensionContext) -> Result<(), ExtensionError> {
        if ctx.principal.trim().is_empty() {
            return Err(ExtensionError {
                code: -32002,
                message: "forbidden".into(),
                data: Some(Value::String(
                    "session metrics require authentication".into(),
                )),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl ExtensionHandler for SessionMetricsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        Self::require_principal(ctx)?;
        if method != "status" {
            return Err(ExtensionError::method_not_found());
        }
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        Ok(json!({
            "legacyListGlobalCalls": self.session_list.legacy_alias_call_count(),
        }))
    }

    fn capabilities(&self) -> Value {
        json!({ "status": true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;

    fn context(principal: &str) -> ExtensionContext {
        ExtensionContext {
            session_id: None,
            principal: principal.into(),
            connection_id: "metrics-test".into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn status_exposes_alias_counter_for_authenticated_principal() {
        let list = Arc::new(SessionListHandler::new());
        let handler = SessionMetricsHandler::new(list);
        let result = handler
            .handle("status", json!({}), &context("owner-a"))
            .await
            .expect("metrics status");
        assert_eq!(result["legacyListGlobalCalls"], 0);
    }

    #[tokio::test]
    async fn status_requires_principal() {
        let handler = SessionMetricsHandler::new(Arc::new(SessionListHandler::new()));
        let error = handler
            .handle("status", json!({}), &context(""))
            .await
            .expect_err("unauthenticated metrics must fail");
        assert_eq!(error.code, -32002);
    }
}
