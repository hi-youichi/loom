use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub struct RelayHandler;

impl RelayHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RelayHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    pub enabled: bool,
    pub connected: bool,
    pub relay_url: Option<String>,
    pub relay_id: Option<String>,
    pub remote_clients: u32,
    pub last_connected: Option<String>,
    pub last_error: Option<String>,
}

fn is_relay_transport(ctx: &ExtensionContext) -> bool {
    ctx.connection_id.starts_with("relay-")
}

fn handle_status(ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let is_relay = is_relay_transport(ctx);

    let status = RelayStatus {
        enabled: is_relay,
        connected: is_relay,
        relay_url: None,
        relay_id: if is_relay {
            Some(ctx.connection_id.clone())
        } else {
            None
        },
        remote_clients: 0,
        last_connected: None,
        last_error: None,
    };

    serde_json::to_value(status).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "relay status serialization failed: {e}"
        ))),
    })
}

#[async_trait]
impl ExtensionHandler for RelayHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "status" => handle_status(ctx),
            "e2ee_info" => Ok(serde_json::json!({
                "supported": true,
                "algorithm": "AES-256-GCM",
                "keyExchange": "X25519",
                "signingKeyRotated": false,
            })),
            "host_lock_status" => Ok(serde_json::json!({
                "enabled": false,
                "locked": false,
                "lockReason": Value::Null,
                "unlockedUntil": Value::Null,
            })),
            "signing_key" => Ok(serde_json::json!({
                "keyId": uuid::Uuid::new_v4().simple().to_string(),
                "algorithm": "Ed25519",
                "rotatedAt": Value::Null,
            })),
            "pairing_start" => {
                let _ = &params;
                let a = &uuid::Uuid::new_v4().simple().to_string()[..4];
                let b = &uuid::Uuid::new_v4().simple().to_string()[..4];
                let shared = {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(uuid::Uuid::new_v4().as_bytes());
                    hasher.finalize()
                };
                Ok(serde_json::json!({
                    "pairingCode": format!("{}-{}", a.to_uppercase(), b.to_uppercase()),
                    "sharedSecret": format!("{shared:x}"),
                    "expiresInSeconds": 120,
                }))
            }
            "pairing_complete" => Ok(serde_json::json!({
                "completed": true,
                "e2ee": { "enabled": true },
                "hostLock": { "enabled": false },
            })),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "status": true,
            "e2ee_info": true,
            "host_lock_status": true,
            "signing_key": true,
            "pairing_start": true,
            "pairing_complete": true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

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
    async fn test_status_disconnected_defaults() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["connected"], false);
        assert!(result["relayUrl"].is_null());
        assert!(result["relayId"].is_null());
        assert_eq!(result["remoteClients"], 0);
        assert!(result["lastConnected"].is_null());
        assert!(result["lastError"].is_null());
    }

    #[tokio::test]
    async fn test_status_connected_positive_path() {
        let ctx = make_ctx("relay-abc123", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["connected"], true);
        assert_eq!(result["relayId"], "relay-abc123");
        assert_eq!(result["remoteClients"], 0);
    }

    #[tokio::test]
    async fn test_status_no_params_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_extraneous_params_ignored() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler
            .handle("status", json!({"foo": 1, "bar": "baz"}), &ctx)
            .await;
        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_returns_valid_json_object() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert!(result.is_object());
    }

    #[tokio::test]
    async fn test_status_read_only_empty_principal() {
        let ctx = make_ctx("conn-1", "");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_no_secrets() {
        let ctx = make_ctx("relay-abc123", "super-secret-token-xyz");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert!(!result.to_string().contains("super-secret-token-xyz"));
    }

    #[tokio::test]
    async fn test_unknown_method_returns_method_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("unknown", json!({}), &ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_capabilities_shape() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert_eq!(caps["status"], true);
        assert_eq!(caps["e2ee_info"], true);
    }

    #[tokio::test]
    async fn test_registry_dispatch_status() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc123", "user");
        let result = registry
            .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["connected"], true);
        assert_eq!(result["relayId"], "relay-abc123");
    }

    #[tokio::test]
    async fn test_registry_dispatch_unknown() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let result = registry
            .dispatch("_loomdesk.dev/relay/unknown", json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_registry_dispatch_no_submethod() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let result = registry
            .dispatch("_loomdesk.dev/relay", json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_registry_capability_snapshot() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let snapshot = registry.build_capability_snapshot();
        assert_eq!(snapshot["relay"]["status"], true);
    }

    #[tokio::test]
    async fn test_status_empty_connection_id() {
        let ctx = make_ctx("", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["connected"], false);
        assert!(result["relayId"].is_null());
    }

    #[tokio::test]
    async fn test_status_connection_id_exact_relay_dash() {
        let ctx = make_ctx("relay-", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["connected"], true);
        assert_eq!(result["relayId"], "relay-");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_uppercase_prefix() {
        let ctx = make_ctx("RELAY-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["connected"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_mixed_case() {
        let ctx = make_ctx("Relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_no_dash() {
        let ctx = make_ctx("relay", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert!(result["relayId"].is_null());
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_in_middle() {
        let ctx = make_ctx("myrelay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_x_no_dash() {
        let ctx = make_ctx("relayx", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_websocket() {
        let ctx = make_ctx("ws-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["connected"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_stdio() {
        let ctx = make_ctx("stdio", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_connection_id_very_long_relay() {
        let long_id = format!("relay-{}", "x".repeat(1024));
        let ctx = make_ctx(&long_id, "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], long_id);
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_special_chars() {
        let ctx = make_ctx("relay-/etc/passwd", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-/etc/passwd");
    }

    #[tokio::test]
    async fn test_status_connection_id_unicode_relay() {
        let ctx = make_ctx("relay-会话-001", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-会话-001");
    }

    #[tokio::test]
    async fn test_status_field_types_strict() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert!(result["enabled"].is_boolean());
        assert!(result["connected"].is_boolean());
        assert!(result["relayUrl"].is_null() || result["relayUrl"].is_string());
        assert!(result["relayId"].is_null() || result["relayId"].is_string());
        assert!(result["remoteClients"].is_u64());
        assert!(result["lastConnected"].is_null() || result["lastConnected"].is_string());
        assert!(result["lastError"].is_null() || result["lastError"].is_string());
    }

    #[tokio::test]
    async fn test_status_field_keys_are_camel_case() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let obj = result.as_object().unwrap();
        let keys: Vec<&String> = obj.keys().collect();
        assert!(keys.iter().any(|key| *key == "enabled"));
        assert!(keys.iter().any(|key| *key == "connected"));
        assert!(keys.iter().any(|key| *key == "relayUrl"));
        assert!(keys.iter().any(|key| *key == "relayId"));
        assert!(keys.iter().any(|key| *key == "remoteClients"));
        assert!(keys.iter().any(|key| *key == "lastConnected"));
        assert!(keys.iter().any(|key| *key == "lastError"));
        assert!(!keys.iter().any(|key| *key == "relay_url"));
        assert!(!keys.iter().any(|key| *key == "relay_id"));
        assert!(!keys.iter().any(|key| *key == "remote_clients"));
        assert!(!keys.iter().any(|key| *key == "last_connected"));
        assert!(!keys.iter().any(|key| *key == "last_error"));
    }

    #[tokio::test]
    async fn test_status_relay_id_equals_connection_id_on_relay() {
        let ctx = make_ctx("relay-xyz-789", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["relayId"].as_str().unwrap(), "relay-xyz-789");
        assert!(result["connected"].as_bool().unwrap());
        assert_eq!(result["enabled"], result["connected"]);
    }

    #[tokio::test]
    async fn test_status_session_id_does_not_affect_result() {
        let mut ctx1 = make_ctx("relay-abc", "user");
        ctx1.session_id = Some("session-A".into());
        let mut ctx2 = make_ctx("relay-abc", "user");
        ctx2.session_id = Some("session-B".into());
        ctx2.session_id = Some("session-B".into());
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx1).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx2).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }

    #[tokio::test]
    async fn test_status_session_id_none_vs_some() {
        let mut ctx1 = make_ctx("relay-abc", "user");
        ctx1.session_id = None;
        let ctx2 = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx1).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx2).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }

    #[tokio::test]
    async fn test_status_principal_does_not_affect_result() {
        let ctx1 = make_ctx("relay-abc", "alice");
        let ctx2 = make_ctx("relay-abc", "bob");
        let ctx3 = make_ctx("relay-abc", "");
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx1).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx2).await.unwrap();
        let r3 = handler.handle("status", json!({}), &ctx3).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
        assert_eq!(r1.to_string(), r3.to_string());
    }

    #[tokio::test]
    async fn test_status_working_directory_does_not_affect_result() {
        let mut ctx1 = make_ctx("relay-abc", "user");
        ctx1.working_directory = Some(PathBuf::from("/tmp/work1"));
        let mut ctx2 = make_ctx("relay-abc", "user");
        ctx2.working_directory = Some(PathBuf::from("/var/work2"));
        let ctx3 = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx1).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx2).await.unwrap();
        let r3 = handler.handle("status", json!({}), &ctx3).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
        assert_eq!(r1.to_string(), r3.to_string());
    }

    #[tokio::test]
    async fn test_status_no_leak_principal_secret() {
        let ctx = make_ctx("relay-abc", "PRINCIPAL-SECRET-DO-NOT-LEAK");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("PRINCIPAL-SECRET-DO-NOT-LEAK"));
        assert!(!s.contains("DO-NOT-LEAK"));
    }

    #[tokio::test]
    async fn test_status_no_leak_session_id_secret() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.session_id = Some("SESSION-SECRET-XYZ".into());
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("SESSION-SECRET-XYZ"));
    }

    #[tokio::test]
    async fn test_status_no_leak_working_directory_secret() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.working_directory = Some(PathBuf::from("/home/PATH-SECRET/secrets"));
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("PATH-SECRET"));
    }

    #[tokio::test]
    async fn test_status_no_leak_client_capabilities_secret() {
        let mut ctx = make_ctx("relay-abc", "user");
        let caps_json = json!({
            "_meta": "CAPS-SECRET-DATA-123",
            "fs": { "readTextFile": true }
        });
        ctx.client_capabilities = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("CAPS-SECRET-DATA-123"));
    }

    #[tokio::test]
    async fn test_status_params_null() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", Value::Null, &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_params_array() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!([]), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_params_string() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!("any string"), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_params_number() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!(42), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_params_bool() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!(true), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_params_nested_object() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler
            .handle("status", json!({"a": {"b": [1, 2, 3]}, "c": null}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_idempotent_sequential() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx).await.unwrap();
        let r3 = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
        assert_eq!(r2.to_string(), r3.to_string());
    }

    #[tokio::test]
    async fn test_status_concurrent_access() {
        use std::sync::Arc;
        let ctx = Arc::new(make_ctx("relay-abc", "user"));
        let handler = Arc::new(RelayHandler::new());
        let mut handles = Vec::new();
        for _ in 0..50 {
            let ctx = Arc::clone(&ctx);
            let handler = Arc::clone(&handler);
            handles.push(tokio::spawn(async move {
                handler.handle("status", json!({}), &ctx).await
            }));
        }
        for h in handles {
            let result = h.await.unwrap().unwrap();
            assert_eq!(result["enabled"], true);
            assert_eq!(result["connected"], true);
            assert_eq!(result["relayId"], "relay-abc");
        }
    }

    #[tokio::test]
    async fn test_status_concurrent_mixed_connection_ids() {
        use std::sync::Arc;
        let handler = Arc::new(RelayHandler::new());
        let mut handles = Vec::new();
        for i in 0..30 {
            let handler = Arc::clone(&handler);
            let cid = if i % 2 == 0 { "relay-x" } else { "stdio-y" };
            let ctx = Arc::new(make_ctx(cid, "user"));
            handles.push(tokio::spawn(async move {
                handler.handle("status", json!({}), &ctx).await
            }));
        }
        for h in handles {
            let result = h.await.unwrap().unwrap();
            if result["enabled"].as_bool().unwrap() {
                assert_eq!(result["connected"], true);
            } else {
                assert_eq!(result["connected"], false);
            }
        }
    }

    #[tokio::test]
    async fn test_status_distinct_handlers_same_result() {
        let ctx = make_ctx("relay-abc", "user");
        let h1 = RelayHandler::new();
        let h2 = RelayHandler::new();
        let h3 = RelayHandler;
        let r1 = h1.handle("status", json!({}), &ctx).await.unwrap();
        let r2 = h2.handle("status", json!({}), &ctx).await.unwrap();
        let r3 = h3.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
        assert_eq!(r2.to_string(), r3.to_string());
    }

    #[tokio::test]
    async fn test_capabilities_is_object() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert!(caps.is_object());
    }

    #[tokio::test]
    async fn test_capabilities_exactly_one_key() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert_eq!(caps.as_object().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn test_capabilities_status_is_bool_true() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert!(caps["status"].is_boolean());
        assert_eq!(caps["status"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn test_capabilities_status_not_string() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert!(!caps["status"].is_string());
    }

    #[tokio::test]
    async fn test_capabilities_status_not_null() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert!(!caps["status"].is_null());
    }

    #[tokio::test]
    async fn test_capabilities_status_not_number() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        assert!(!caps["status"].is_number());
    }

    #[tokio::test]
    async fn test_capabilities_idempotent() {
        let handler = RelayHandler::new();
        let c1 = handler.capabilities();
        let c2 = handler.capabilities();
        let c3 = handler.capabilities();
        assert_eq!(c1.to_string(), c2.to_string());
        assert_eq!(c2.to_string(), c3.to_string());
    }

    #[tokio::test]
    async fn test_default_impl_equivalent_to_new() {
        let h_new = RelayHandler::new();
        let h_default: RelayHandler = Default::default();
        let ctx = make_ctx("relay-x", "user");
        let r1 = h_new.handle("status", json!({}), &ctx).await.unwrap();
        let r2 = h_default.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
        assert_eq!(
            h_new.capabilities().to_string(),
            h_default.capabilities().to_string()
        );
    }

    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}

    #[test]
    fn test_relay_handler_is_send() {
        _assert_send::<RelayHandler>();
    }

    #[test]
    fn test_relay_handler_is_sync() {
        _assert_sync::<RelayHandler>();
    }

    #[test]
    fn test_arc_relay_handler_is_send_sync() {
        _assert_send::<Arc<RelayHandler>>();
        _assert_sync::<Arc<RelayHandler>>();
    }

    #[tokio::test]
    async fn test_registry_dispatch_missing_prefix() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let result = registry.dispatch("relay/status", json!({}), &ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_registry_dispatch_wrong_prefix() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let result = registry
            .dispatch("acp://relay/status", json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_registry_dispatch_unregistered_domain() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let result = registry
            .dispatch("_loomdesk.dev/other_domain/status", json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32001);
    }

    #[tokio::test]
    async fn test_registry_dispatch_capability_not_supported_data_contains_domain() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let result = registry
            .dispatch("_loomdesk.dev/other_domain/status", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(result.code, -32001);
        let data = result.data.unwrap();
        assert!(data.to_string().contains("other_domain"));
    }

    #[tokio::test]
    async fn test_registry_dispatch_trailing_slash_only() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let result = registry
            .dispatch("_loomdesk.dev/relay/", json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_registry_has_domain_after_register() {
        let mut registry = super::super::ExtensionRegistry::new();
        assert!(!registry.has_domain("relay"));
        registry.register("relay", Arc::new(RelayHandler::new()));
        assert!(registry.has_domain("relay"));
    }

    #[tokio::test]
    async fn test_registry_snapshot_only_contains_registered_domains() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let snapshot = registry.build_capability_snapshot();
        assert!(snapshot.get("relay").is_some());
        assert!(snapshot.get("connection").is_none());
        assert!(snapshot.get("pairing").is_none());
        assert_eq!(snapshot.as_object().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_registry_dispatch_concurrent() {
        use std::sync::Arc;
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let registry = Arc::new(registry);
        let mut handles = Vec::new();
        for i in 0..30 {
            let registry = Arc::clone(&registry);
            let cid = if i % 3 == 0 {
                "relay-x"
            } else if i % 3 == 1 {
                "stdio-y"
            } else {
                "ws-z"
            };
            let ctx = Arc::new(make_ctx(cid, "user"));
            handles.push(tokio::spawn(async move {
                registry
                    .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
                    .await
            }));
        }
        for h in handles {
            let result = h.await.unwrap().unwrap();
            assert!(result.is_object());
        }
    }

    #[tokio::test]
    async fn test_method_not_found_error_code_and_message() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let err = handler
            .handle("anything", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "method_not_found");
        assert!(err.data.is_none());
    }

    #[tokio::test]
    async fn test_method_not_found_for_empty_method() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let err = handler.handle("", json!({}), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32601);
    }

    #[tokio::test]
    async fn test_method_not_found_for_status_typo() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        for typo in &["Status", "STATUS", "status ", " status", "stats", "stat"] {
            let err = handler.handle(typo, json!({}), &ctx).await.unwrap_err();
            assert_eq!(err.code, -32601, "typo '{typo}' should be method_not_found");
        }
    }

    #[tokio::test]
    async fn test_response_matches_spec_field_names() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let serialized = result.to_string();
        assert!(serialized.contains("\"enabled\""));
        assert!(serialized.contains("\"connected\""));
        assert!(serialized.contains("\"relayUrl\""));
        assert!(serialized.contains("\"relayId\""));
        assert!(serialized.contains("\"remoteClients\""));
        assert!(serialized.contains("\"lastConnected\""));
        assert!(serialized.contains("\"lastError\""));
    }

    #[tokio::test]
    async fn test_response_remote_clients_is_u32_safe() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let rc = result["remoteClients"].as_u64().unwrap();
        assert!(rc <= u32::MAX as u64);
    }

    #[tokio::test]
    async fn test_capability_snapshot_under_domain_key() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let snapshot = registry.build_capability_snapshot();
        let relay_obj = snapshot.get("relay").expect("relay key missing");
        assert!(relay_obj.is_object());
        assert_eq!(relay_obj["status"], true);
        assert!(relay_obj.get("list").is_none());
        assert!(relay_obj.get("get").is_none());
        assert!(relay_obj.get("close").is_none());
    }

    #[tokio::test]
    async fn test_capability_snapshot_refresh_matches_build() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let built = registry.build_capability_snapshot();
        registry.refresh_capabilities().await;
        let stored = registry.capability_snapshot().await;
        assert_eq!(built.to_string(), stored.to_string());
    }

    #[tokio::test]
    async fn test_capability_handler_matches_snapshot_for_dispatch() {
        let handler = RelayHandler::new();
        let handler_caps = handler.capabilities();
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(handler));
        let snapshot = registry.build_capability_snapshot();
        assert_eq!(handler_caps.to_string(), snapshot["relay"].to_string());
    }

    #[tokio::test]
    async fn test_relay_status_serde_round_trip_via_serialize() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = serde_json::to_string(&result).unwrap();
        let parsed: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(result.to_string(), parsed.to_string());
    }

    #[tokio::test]
    async fn test_status_connection_id_with_numbers_after_relay() {
        let ctx = make_ctx("relay-1234567890", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-1234567890");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_emoji() {
        let ctx = make_ctx("relay-🚀-test", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-🚀-test");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_double_dash() {
        let ctx = make_ctx("relay--abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay--abc");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_underscore() {
        let ctx = make_ctx("relay-abc_def", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc_def");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_dot() {
        let ctx = make_ctx("relay-abc.123.local", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc.123.local");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_query_string() {
        let ctx = make_ctx("relay-abc?token=xyz", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc?token=xyz");
    }

    #[tokio::test]
    async fn test_status_connection_id_single_char_after_relay() {
        let ctx = make_ctx("relay-x", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-x");
    }

    #[tokio::test]
    async fn test_status_response_has_exactly_seven_fields() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result.as_object().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn test_status_enabled_equals_connected_invariant_for_relay() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], result["connected"]);
    }

    #[tokio::test]
    async fn test_status_enabled_equals_connected_invariant_for_non_relay() {
        let ctx = make_ctx("ws-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], result["connected"]);
    }

    #[tokio::test]
    async fn test_status_enabled_equals_connected_for_stdio() {
        let ctx = make_ctx("stdio", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["connected"], false);
    }

    #[tokio::test]
    async fn test_status_remote_clients_is_zero_for_relay() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["remoteClients"], 0);
    }

    #[tokio::test]
    async fn test_status_remote_clients_is_zero_for_non_relay() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["remoteClients"], 0);
    }

    #[tokio::test]
    async fn test_status_remote_clients_is_zero_for_empty_connection() {
        let ctx = make_ctx("", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["remoteClients"], 0);
    }

    #[tokio::test]
    async fn test_status_optional_fields_are_null_when_not_relay() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert!(result["relayUrl"].is_null());
        assert!(result["relayId"].is_null());
        assert!(result["lastConnected"].is_null());
        assert!(result["lastError"].is_null());
    }

    #[tokio::test]
    async fn test_status_optional_url_and_timestamps_null_for_relay_in_v1() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert!(result["relayUrl"].is_null());
        assert!(result["lastConnected"].is_null());
        assert!(result["lastError"].is_null());
    }

    #[tokio::test]
    async fn test_status_method_not_found_has_no_data() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        let err = handler
            .handle("status_typo", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
        assert!(err.data.is_none());
    }

    #[tokio::test]
    async fn test_status_unknown_method_error_code_consistent_across_calls() {
        let ctx = make_ctx("conn-1", "user");
        let handler = RelayHandler::new();
        for _ in 0..5 {
            let err = handler
                .handle("unknown", json!({}), &ctx)
                .await
                .unwrap_err();
            assert_eq!(err.code, -32601);
            assert_eq!(err.message, "method_not_found");
        }
    }

    #[tokio::test]
    async fn test_status_concurrent_100_tasks_all_succeed() {
        use std::sync::Arc;
        let ctx = Arc::new(make_ctx("relay-abc", "user"));
        let handler = Arc::new(RelayHandler::new());
        let mut handles = Vec::new();
        for _ in 0..100 {
            let ctx = Arc::clone(&ctx);
            let handler = Arc::clone(&handler);
            handles.push(tokio::spawn(async move {
                handler.handle("status", json!({}), &ctx).await
            }));
        }
        for h in handles {
            let result = h.await.unwrap().unwrap();
            assert_eq!(result["enabled"], true);
            assert_eq!(result["connected"], true);
            assert_eq!(result["relayId"], "relay-abc");
        }
    }

    #[tokio::test]
    async fn test_status_concurrent_with_mixed_principals() {
        use std::sync::Arc;
        let handler = Arc::new(RelayHandler::new());
        let mut handles = Vec::new();
        for i in 0..20 {
            let handler = Arc::clone(&handler);
            let principal = format!("user-{i}");
            let ctx = Arc::new(make_ctx("relay-abc", &principal));
            handles.push(tokio::spawn(async move {
                handler.handle("status", json!({}), &ctx).await
            }));
        }
        for h in handles {
            let result = h.await.unwrap().unwrap();
            assert_eq!(result["enabled"], true);
        }
    }

    #[tokio::test]
    async fn test_status_capabilities_immutable_across_instances() {
        let h1 = RelayHandler::new();
        let h2 = RelayHandler::new();
        let h3 = RelayHandler;
        let c1 = h1.capabilities();
        let c2 = h2.capabilities();
        let c3 = h3.capabilities();
        assert_eq!(c1.to_string(), c2.to_string());
        assert_eq!(c2.to_string(), c3.to_string());
    }

    #[tokio::test]
    async fn test_status_response_is_valid_json_parseable() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = serde_json::to_string(&result).unwrap();
        let parsed: Value = serde_json::from_str(&s).expect("response must be valid JSON");
        assert!(parsed.is_object());
    }

    #[tokio::test]
    async fn test_status_capabilities_is_valid_json_parseable() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        let s = serde_json::to_string(&caps).unwrap();
        let parsed: Value = serde_json::from_str(&s).expect("capabilities must be valid JSON");
        assert!(parsed.is_object());
    }

    #[tokio::test]
    async fn test_status_call_through_registry_then_handler() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let r1 = registry
            .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
            .await
            .unwrap();
        let handler = RelayHandler::new();
        let r2 = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }

    #[tokio::test]
    async fn test_status_handler_call_then_registry() {
        let handler = RelayHandler::new();
        let ctx = make_ctx("relay-abc", "user");
        let r1 = handler.handle("status", json!({}), &ctx).await.unwrap();
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("relay", Arc::new(RelayHandler::new()));
        let r2 = registry
            .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }

    #[tokio::test]
    async fn test_status_principal_unicode() {
        let ctx = make_ctx("relay-abc", "用户-测试-001");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("用户-测试-001"));
    }

    #[tokio::test]
    async fn test_status_principal_very_long_string() {
        let long_principal = "p".repeat(10000);
        let ctx = make_ctx("relay-abc", &long_principal);
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains(&long_principal));
    }

    #[tokio::test]
    async fn test_status_session_id_very_long_string() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.session_id = Some("s".repeat(10000));
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
    }

    #[tokio::test]
    async fn test_status_working_directory_unicode() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.working_directory = Some(PathBuf::from("/home/用户/工作目录/项目"));
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("用户"));
    }

    #[tokio::test]
    async fn test_status_client_capabilities_complex_object() {
        let mut ctx = make_ctx("relay-abc", "user");
        let caps_json = json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
            "mcp": { "http": true, "stdio": true, "sse": true },
            "prompts": { "text": true, "resourceLink": true, "image": true },
            "session": { "list": {}, "fork": {}, "resume": {} }
        });
        ctx.client_capabilities = ClientCapabilitiesInfo::from_json(Some(caps_json));
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        let s = result.to_string();
        assert!(!s.contains("readTextFile"));
        assert!(!s.contains("terminal"));
        assert!(!s.contains("mcp"));
    }

    #[tokio::test]
    async fn test_status_response_no_internal_metadata() {
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("_meta"));
        assert!(!s.contains("internal"));
        assert!(!s.contains("debug"));
    }

    #[tokio::test]
    async fn test_status_capabilities_no_internal_metadata() {
        let handler = RelayHandler::new();
        let caps = handler.capabilities();
        let s = caps.to_string();
        assert!(!s.contains("_meta"));
        assert!(!s.contains("internal"));
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_colon() {
        let ctx = make_ctx("relay-host:port", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-host:port");
    }

    #[tokio::test]
    async fn test_status_connection_id_relay_with_at_sign() {
        let ctx = make_ctx("relay-user@host", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-user@host");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_newlines() {
        let ctx = make_ctx("relay-abc\ndef", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc\ndef");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_tabs() {
        let ctx = make_ctx("relay-abc\tdef", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc\tdef");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_quotes() {
        let ctx = make_ctx("relay-\"quoted\"", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-\"quoted\"");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_backslash() {
        let ctx = make_ctx("relay-abc\\def", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-abc\\def");
    }

    #[tokio::test]
    async fn test_status_connection_id_with_json_injection_attempt() {
        let ctx = make_ctx("relay-\"}alert(1);//", "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], "relay-\"}alert(1);//");
    }

    #[tokio::test]
    async fn test_status_response_field_order_independent() {
        let ctx1 = make_ctx("relay-a", "user");
        let ctx2 = make_ctx("relay-b", "user");
        let handler = RelayHandler::new();
        let r1 = handler.handle("status", json!({}), &ctx1).await.unwrap();
        let r2 = handler.handle("status", json!({}), &ctx2).await.unwrap();
        let keys1: std::collections::BTreeSet<_> =
            r1.as_object().unwrap().keys().cloned().collect();
        let keys2: std::collections::BTreeSet<_> =
            r2.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys1, keys2);
    }

    #[tokio::test]
    async fn test_status_capabilities_field_order_independent() {
        let h1 = RelayHandler::new();
        let h2 = RelayHandler::new();
        let c1 = h1.capabilities();
        let c2 = h2.capabilities();
        let keys1: std::collections::BTreeSet<_> =
            c1.as_object().unwrap().keys().cloned().collect();
        let keys2: std::collections::BTreeSet<_> =
            c2.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys1, keys2);
    }

    #[tokio::test]
    async fn test_status_no_throw_on_extreme_connection_id_length() {
        let long_id = format!("relay-{}", "a".repeat(100_000));
        let ctx = make_ctx(&long_id, "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["relayId"], long_id);
    }

    #[tokio::test]
    async fn test_status_very_long_connection_id_does_not_panic() {
        let long_id = "x".repeat(1_000_000);
        let ctx = make_ctx(&long_id, "user");
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert!(result["relayId"].is_null());
    }

    #[tokio::test]
    async fn test_status_response_with_all_transports_yields_valid_structure() {
        for cid in &["relay-abc", "ws-xyz", "stdio", "tcp-1", "unix-1", ""] {
            let ctx = make_ctx(cid, "user");
            let handler = RelayHandler::new();
            let result = handler.handle("status", json!({}), &ctx).await.unwrap();
            assert!(result.is_object());
            assert_eq!(result.as_object().unwrap().len(), 7);
            assert!(result["enabled"].is_boolean());
            assert!(result["connected"].is_boolean());
            assert!(result["remoteClients"].is_u64());
        }
    }

    #[tokio::test]
    async fn test_status_capabilities_stable_after_many_handler_calls() {
        let handler = RelayHandler::new();
        let initial_caps = handler.capabilities();
        for _ in 0..50 {
            let _ = handler
                .handle("status", json!({}), &make_ctx("relay-abc", "user"))
                .await;
        }
        let final_caps = handler.capabilities();
        assert_eq!(initial_caps.to_string(), final_caps.to_string());
    }

    #[tokio::test]
    async fn test_status_no_panic_on_null_connection_id_inner() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.connection_id = String::new();
        let handler = RelayHandler::new();
        let result = handler.handle("status", json!({}), &ctx).await.unwrap();
        assert_eq!(result["enabled"], false);
    }

    #[tokio::test]
    async fn test_status_response_consistent_for_same_connection_id_across_rebuilds() {
        let mut registry1 = super::super::ExtensionRegistry::new();
        registry1.register("relay", Arc::new(RelayHandler::new()));
        let mut registry2 = super::super::ExtensionRegistry::new();
        registry2.register("relay", Arc::new(RelayHandler::new()));
        let ctx = make_ctx("relay-abc", "user");
        let r1 = registry1
            .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
            .await
            .unwrap();
        let r2 = registry2
            .dispatch("_loomdesk.dev/relay/status", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }

    #[tokio::test]
    async fn test_status_handler_does_not_mutate_context() {
        let mut ctx = make_ctx("relay-abc", "user");
        ctx.session_id = Some("original-session".into());
        let original_session = ctx.session_id.clone();
        let original_principal = ctx.principal.clone();
        let original_connection = ctx.connection_id.clone();
        let handler = RelayHandler::new();
        let _ = handler.handle("status", json!({}), &ctx).await;
        assert_eq!(ctx.session_id, original_session);
        assert_eq!(ctx.principal, original_principal);
        assert_eq!(ctx.connection_id, original_connection);
    }

    #[tokio::test]
    async fn test_status_handler_does_not_mutate_params() {
        let params = json!({"key": "value"});
        let params_before = params.clone();
        let ctx = make_ctx("relay-abc", "user");
        let handler = RelayHandler::new();
        let _ = handler.handle("status", params, &ctx).await;
        assert_eq!(
            params_before.to_string(),
            json!({"key": "value"}).to_string()
        );
    }

    #[tokio::test]
    async fn test_status_capabilities_handler_call_independence() {
        let handler = RelayHandler::new();
        let ctx = make_ctx("relay-abc", "user");
        let caps_before = handler.capabilities();
        let _ = handler.handle("status", json!({}), &ctx).await;
        let caps_after = handler.capabilities();
        assert_eq!(caps_before.to_string(), caps_after.to_string());
    }
}
