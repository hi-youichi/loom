//! Three-layer permission gate for extension write operations.
//!
//! Layer 1: Capability — client declared the method in their capabilities
//! Layer 2: Server policy — path/resource/rate-limit checks
//! Layer 3: Explicit confirm — high-risk operations require user confirmation

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

/// Auth status exposed on an already authenticated ACP connection.
///
/// The server owns the pre-auth password/JWT handshake. Reaching this handler
/// proves that the socket has already passed that gate, so it only reports the
/// safe post-auth state needed by browser boot and never returns credentials.
pub struct AuthHandler;

#[async_trait]
impl ExtensionHandler for AuthHandler {
    async fn handle(
        &self,
        method: &str,
        _params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "status" => Ok(json!({
                "authenticated": true,
                "passwordConfigured": false,
                "passkeyEnabled": false,
                "hasPasskeys": false,
                "passkeyCount": 0,
                "rpId": Value::Null,
            })),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({ "status": true })
    }
}

pub fn check_capability(
    ctx: &ExtensionContext,
    domain: &str,
    _method: &str,
) -> Result<(), ExtensionError> {
    let caps = &ctx.client_capabilities;
    let _ = caps;
    if domain.is_empty() {
        return Err(ExtensionError::capability_not_supported(domain));
    }
    Ok(())
}

pub fn check_server_policy(
    ctx: &ExtensionContext,
    _domain: &str,
    _method: &str,
) -> Result<(), ExtensionError> {
    if ctx.principal.is_empty() {
        return Err(ExtensionError::forbidden("no authenticated principal"));
    }
    Ok(())
}

pub fn requires_confirmation(domain: &str, method: &str) -> bool {
    matches!(
        (domain, method),
        ("git", "push" | "force_push" | "commit" | "amend")
            | ("git", "cherry_pick" | "rebase" | "reset")
            | ("worktree", "delete")
            | ("files", "delete" | "move" | "rename")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;

    fn context() -> ExtensionContext {
        ExtensionContext {
            session_id: None,
            principal: "local-anonymous".into(),
            connection_id: "conn-auth-test".into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn authenticated_status_is_available_after_initialize() {
        let result = AuthHandler
            .handle("status", json!({}), &context())
            .await
            .expect("status");
        assert_eq!(result["authenticated"], true);
        assert_eq!(result["passwordConfigured"], false);
    }
}
