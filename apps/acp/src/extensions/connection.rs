use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub struct ConnectionHandler;

impl ConnectionHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConnectionHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Stdio,
    #[serde(rename = "websocket")]
    WebSocket,
    Relay,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthIdentity {
    pub method: String,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub transport: TransportType,
    pub local_endpoint: Option<String>,
    pub remote_endpoint: Option<String>,
    pub connected_at: String,
    pub auth_identity: Option<AuthIdentity>,
    pub client_scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCapabilities {
    pub standard: Vec<String>,
    pub extensions: Vec<String>,
    pub limitations: Vec<String>,
}

fn mask_identity(s: &str) -> String {
    if s.is_empty() {
        return "anonymous***".to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 1 {
        return "***".to_string();
    }
    if chars.len() <= 4 {
        let keep: String = chars[..1].iter().collect();
        return format!("{keep}***");
    }
    let keep: String = chars.iter().take(10).collect();
    format!("{keep}***")
}

fn resolve_transport(ctx: &ExtensionContext) -> TransportType {
    if ctx.connection_id.starts_with("relay-") {
        TransportType::Relay
    } else if ctx.connection_id.starts_with("ws-") {
        TransportType::WebSocket
    } else {
        TransportType::Stdio
    }
}

fn derive_client_scope(ctx: &ExtensionContext) -> Vec<String> {
    let detected = ctx.client_capabilities.detected();
    let mut scope = Vec::new();
    if detected.fs_read_text_file {
        scope.push("fs:read".to_string());
    }
    if detected.fs_write_text_file {
        scope.push("fs:write".to_string());
    }
    if detected.terminal_supported {
        scope.push("terminal:create".to_string());
    }
    if detected.session_list {
        scope.push("session:list".to_string());
    }
    if detected.session_fork {
        scope.push("session:fork".to_string());
    }
    if detected.session_resume {
        scope.push("session:resume".to_string());
    }
    if detected.mcp_http || detected.mcp_stdio || detected.mcp_sse {
        scope.push("mcp".to_string());
    }
    scope
}

fn build_limitations(transport: &TransportType) -> Vec<String> {
    match transport {
        TransportType::Relay => {
            vec!["relay transport: max payload size 1MB".to_string()]
        }
        TransportType::Stdio => {
            vec!["stdio transport: no concurrent sessions".to_string()]
        }
        TransportType::WebSocket => vec![],
    }
}

fn handle_info(ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let transport = resolve_transport(ctx);
    let principal = if ctx.principal.is_empty() {
        String::new()
    } else {
        ctx.principal.clone()
    };

    let auth_identity = if principal.is_empty() {
        None
    } else {
        Some(AuthIdentity {
            method: "bearer_token".to_string(),
            identity: mask_identity(&principal),
        })
    };

    let info = ConnectionInfo {
        transport,
        local_endpoint: None,
        remote_endpoint: None,
        connected_at: chrono::Utc::now().to_rfc3339(),
        auth_identity,
        client_scope: derive_client_scope(ctx),
    };

    serde_json::to_value(info).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "connection info serialization failed: {e}"
        ))),
    })
}

fn handle_capabilities_rpc(ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let transport = resolve_transport(ctx);

    let caps = ConnectionCapabilities {
        standard: vec![
            "initialize".to_string(),
            "session/new".to_string(),
            "session/load".to_string(),
            "session/prompt".to_string(),
            "session/update".to_string(),
            "session/cancel".to_string(),
            "fs/read_text_file".to_string(),
            "fs/write_text_file".to_string(),
            "terminal/create".to_string(),
        ],
        extensions: vec![
            "worktree".to_string(),
            "git".to_string(),
            "files".to_string(),
            "mcp".to_string(),
            "goal".to_string(),
            "connection".to_string(),
            "relay".to_string(),
            "pairing".to_string(),
            "client-auth".to_string(),
        ],
        limitations: build_limitations(&transport),
    };

    serde_json::to_value(caps).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "capability serialization failed: {e}"
        ))),
    })
}

#[async_trait]
impl ExtensionHandler for ConnectionHandler {
    async fn handle(
        &self,
        method: &str,
        _params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "info" => handle_info(ctx),
            "capabilities" => handle_capabilities_rpc(ctx),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "info": true,
            "capabilities": true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use serde_json::json;

    fn make_ctx(connection_id: &str, principal: &str) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: principal.into(),
            connection_id: connection_id.into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn test_info_returns_transport_string() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert!(result["transport"].is_string());
        assert_eq!(result["transport"], "stdio");
    }

    #[tokio::test]
    async fn test_info_connected_at_rfc3339() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        let connected_at = result["connectedAt"].as_str().unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(connected_at).is_ok());
    }

    #[tokio::test]
    async fn test_info_auth_identity_structure() {
        let ctx = make_ctx("conn-1", "client-token-abc123");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        let identity = &result["authIdentity"];
        assert!(identity["method"].is_string());
        assert!(identity["identity"].is_string());
        assert_eq!(identity["method"], "bearer_token");
    }

    #[tokio::test]
    async fn test_info_identity_masked() {
        let ctx = make_ctx("conn-1", "super-secret-token-xyz");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        let identity = result["authIdentity"]["identity"].as_str().unwrap();
        assert!(identity.contains("***"));
        assert!(!identity.contains("super-secret-token-xyz"));
        assert!(!result.to_string().contains("super-secret-token-xyz"));
    }

    #[tokio::test]
    async fn test_info_client_scope_array() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert!(result["clientScope"].is_array());
    }

    #[tokio::test]
    async fn test_info_no_params_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_info_empty_principal_succeeds() {
        let ctx = make_ctx("conn-1", "");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await;
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value["authIdentity"].is_null());
    }

    #[tokio::test]
    async fn test_capabilities_standard_non_empty() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result["standard"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_capabilities_extensions_contains_connection() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        let extensions = result["extensions"].as_array().unwrap();
        assert!(extensions.iter().any(|e| e == "connection"));
    }

    #[tokio::test]
    async fn test_capabilities_limitations_array() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result["limitations"].is_array());
    }

    #[tokio::test]
    async fn test_capabilities_no_params_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("capabilities", json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capabilities_no_secrets() {
        let ctx = make_ctx("conn-1", "super-secret-token-xyz");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result.to_string().contains("super-secret-token-xyz"));
    }

    #[tokio::test]
    async fn test_unknown_method_returns_method_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("unknown", json!({}), &ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_capabilities_shape() {
        let handler = ConnectionHandler::new();
        let caps = handler.capabilities();
        assert_eq!(caps["info"], true);
        assert_eq!(caps["capabilities"], true);
        assert_eq!(caps.as_object().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_websocket_transport_serializes_single_word() {
        let ctx = make_ctx("ws-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transport"], "websocket");
    }

    #[tokio::test]
    async fn test_relay_transport() {
        let ctx = make_ctx("relay-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transport"], "relay");
    }

    #[tokio::test]
    async fn test_stdio_transport_default() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transport"], "stdio");
    }

    #[tokio::test]
    async fn test_capabilities_limitations_derived_from_stdio() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        let limitations = result["limitations"].as_array().unwrap();
        assert!(limitations
            .iter()
            .any(|l| l.as_str().unwrap().contains("no concurrent sessions")));
    }

    #[tokio::test]
    async fn test_capabilities_limitations_derived_from_relay() {
        let ctx = make_ctx("relay-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        let limitations = result["limitations"].as_array().unwrap();
        assert!(limitations
            .iter()
            .any(|l| l.as_str().unwrap().contains("max payload size")));
    }

    #[tokio::test]
    async fn test_capabilities_limitations_derived_from_websocket_empty() {
        let ctx = make_ctx("ws-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        let limitations = result["limitations"].as_array().unwrap();
        assert!(limitations.is_empty());
    }

    fn make_ctx_with_caps(
        connection_id: &str,
        principal: &str,
        caps: ClientCapabilitiesInfo,
    ) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: principal.into(),
            connection_id: connection_id.into(),
            working_directory: None,
            client_capabilities: caps,
        }
    }

    #[tokio::test]
    async fn test_client_scope_mapped_from_capabilities() {
        let caps_json = serde_json::json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
            "mcp": { "http": true },
            "session": { "list": {}, "fork": {}, "resume": {} }
        });
        let caps = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let ctx = make_ctx_with_caps("conn-1", "user", caps);
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        let scope = result["clientScope"].as_array().unwrap();
        let scope_strs: Vec<&str> = scope.iter().map(|s| s.as_str().unwrap()).collect();
        assert!(scope_strs.contains(&"fs:read"));
        assert!(scope_strs.contains(&"fs:write"));
        assert!(scope_strs.contains(&"terminal:create"));
        assert!(scope_strs.contains(&"mcp"));
        assert!(scope_strs.contains(&"session:list"));
        assert!(scope_strs.contains(&"session:fork"));
        assert!(scope_strs.contains(&"session:resume"));
    }

    #[tokio::test]
    async fn test_client_scope_empty_with_no_capabilities() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        let scope = result["clientScope"].as_array().unwrap();
        assert!(scope.is_empty());
    }

    #[tokio::test]
    async fn test_info_returns_valid_json_value() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler.handle("info", json!({}), &ctx).await.unwrap();
        assert!(result.is_object());
    }

    #[tokio::test]
    async fn test_capabilities_returns_valid_json_value() {
        let ctx = make_ctx("conn-1", "user");
        let handler = ConnectionHandler::new();
        let result = handler
            .handle("capabilities", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result.is_object());
    }

    #[test]
    fn test_mask_identity_empty() {
        assert_eq!(mask_identity(""), "anonymous***");
    }

    #[test]
    fn test_mask_identity_single_char() {
        assert_eq!(mask_identity("a"), "***");
    }

    #[test]
    fn test_mask_identity_short() {
        assert_eq!(mask_identity("ab"), "a***");
        assert_eq!(mask_identity("abcd"), "a***");
    }

    #[test]
    fn test_mask_identity_exactly_10_chars() {
        let principal = "0123456789";
        let masked = mask_identity(principal);
        assert_eq!(masked, "0123456789***");
    }

    #[test]
    fn test_mask_identity_normal() {
        let masked = mask_identity("client-token-abc123");
        assert!(masked.ends_with("***"));
        assert!(masked.starts_with("client-tok"));
        assert!(!masked.contains("client-token-abc123"));
    }

    #[test]
    fn test_mask_identity_never_leaks_full() {
        let long = "super-long-secret-token-xyz-12345";
        let masked = mask_identity(long);
        assert!(!masked.contains(long));
        assert!(masked.len() < long.len());
    }

    #[test]
    fn test_serde_websocket_transport_enum() {
        let serialized = serde_json::to_string(&TransportType::WebSocket).unwrap();
        assert_eq!(serialized, "\"websocket\"");
        let relay = serde_json::to_string(&TransportType::Relay).unwrap();
        assert_eq!(relay, "\"relay\"");
        let stdio = serde_json::to_string(&TransportType::Stdio).unwrap();
        assert_eq!(stdio, "\"stdio\"");
    }
}
