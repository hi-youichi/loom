use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::auth;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const REDEEM_MAX_ATTEMPTS: u32 = 5;
const DEFAULT_TTL_SECONDS: u64 = 300;
const MIN_TTL_SECONDS: u64 = 1;
const MAX_TTL_SECONDS: u64 = 86400;
const DEFAULT_LIST_LIMIT: usize = 20;
const MAX_LIST_LIMIT: usize = 100;

const VALID_TRANSPORT_DIRECT: &str = "direct";
const VALID_TRANSPORT_RELAY: &str = "relay";

pub struct PairingHandler {
    store: Arc<PairingStore>,
}

impl PairingHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(PairingStore::default()),
        }
    }

    pub fn with_store(store: Arc<PairingStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<PairingStore> {
        &self.store
    }
}

impl Default for PairingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct PairingStore {
    pending: Mutex<HashMap<String, PairingRecord>>,
    clients: Mutex<HashMap<String, ClientAuthEntry>>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .len()
    }

    pub fn insert(&self, record: PairingRecord) {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .insert(record.pairing_id.clone(), record);
    }

    pub fn get(&self, pairing_id: &str) -> Option<PairingRecord> {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .get(pairing_id)
            .cloned()
    }

    pub fn remove(&self, pairing_id: &str) -> Option<PairingRecord> {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .remove(pairing_id)
    }

    pub fn list_pending(&self) -> Vec<PairingRecord> {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .values()
            .filter(|r| !r.redeemed && !is_expired(r))
            .cloned()
            .collect()
    }

    pub fn list_all(&self) -> Vec<PairingRecord> {
        self.pending
            .lock()
            .expect("pairing store mutex poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn try_redeem(&self, secret_hash: &str) -> RedeemResult {
        let mut pending = self.pending.lock().expect("pairing store mutex poisoned");

        let matching_pid: Option<String> = pending
            .iter()
            .find(|(_, r)| r.secret_hash == secret_hash)
            .map(|(pid, _)| pid.clone());

        if let Some(pid) = matching_pid {
            let (attempts, redeemed, expired) = if let Some(r) = pending.get(&pid) {
                (r.attempts, r.redeemed, is_expired(r))
            } else {
                (0, false, false)
            };

            if attempts >= REDEEM_MAX_ATTEMPTS {
                pending.remove(&pid);
                return RedeemResult::AttemptsExceeded { pairing_id: pid };
            }
            if redeemed {
                return RedeemResult::AlreadyRedeemed;
            }
            if expired {
                return RedeemResult::Expired;
            }
            pending.get_mut(&pid).unwrap().redeemed = true;
            return RedeemResult::Success { pairing_id: pid };
        }

        let recent_pid: Option<String> = pending
            .iter()
            .filter(|(_, r)| !r.redeemed && !is_expired(r))
            .max_by_key(|(_, r)| r.created_at.clone())
            .map(|(pid, _)| pid.clone());

        if let Some(pid) = recent_pid {
            let new_attempts = {
                let rec = pending.get_mut(&pid).unwrap();
                rec.attempts = rec.attempts.saturating_add(1);
                rec.attempts
            };
            if new_attempts >= REDEEM_MAX_ATTEMPTS {
                pending.remove(&pid);
                return RedeemResult::AttemptsExceeded { pairing_id: pid };
            }
            return RedeemResult::NoMatch {
                last_pairing_id: Some(pid),
            };
        }

        RedeemResult::NotFound
    }

    pub fn cancel(&self, pairing_id: &str) -> CancelResult {
        let mut pending = self.pending.lock().expect("pairing store mutex poisoned");
        match pending.get(pairing_id) {
            None => CancelResult::NotFound,
            Some(r) if r.redeemed => CancelResult::NotFound,
            Some(r) if is_expired(r) => CancelResult::NotFound,
            Some(_) => {
                pending.remove(pairing_id);
                CancelResult::Cancelled
            }
        }
    }

    pub fn store_client_token(&self, entry: ClientAuthEntry) {
        self.clients
            .lock()
            .expect("pairing store mutex poisoned")
            .insert(entry.client_id.clone(), entry);
    }

    pub fn get_client_by_token_hash(&self, token_hash: &str) -> Option<ClientAuthEntry> {
        self.clients
            .lock()
            .expect("pairing store mutex poisoned")
            .values()
            .find(|e| e.token_hash == token_hash)
            .cloned()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingRecord {
    pub pairing_id: String,
    pub secret_hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub attempts: u32,
    pub redeemed: bool,
    pub allowed_transports: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuthEntry {
    pub client_id: String,
    pub name: String,
    pub platform: String,
    pub auth_method: String,
    pub created_at: String,
    pub last_active: Option<String>,
    pub revoked: bool,
    pub revoked_at: Option<String>,
    pub scope: Vec<String>,
    pub token_hash: String,
}

#[derive(Debug)]
pub enum RedeemResult {
    Success { pairing_id: String },
    AlreadyRedeemed,
    Expired,
    AttemptsExceeded { pairing_id: String },
    NoMatch { last_pairing_id: Option<String> },
    NotFound,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CancelResult {
    Cancelled,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPayload {
    pub secret: String,
    pub pairing_id: String,
    pub expires_at: String,
    pub transports: Vec<PairingTransport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingTransport {
    #[serde(rename = "type")]
    pub r#type: PairingTransportType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PairingTransportType {
    Direct,
    Relay,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairing {
    pub pairing_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub attempts: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCreateParams {
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub allowed_transports: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRedeemParams {
    pub secret: String,
    pub client_info: ClientInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCancelParams {
    pub pairing_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRedeemResponse {
    pub pairing_id: String,
    pub redeemed: bool,
    pub client_token: String,
    pub transport: PairingTransport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCancelResponse {
    pub pairing_id: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingTransportsResponse {
    pub transports: Vec<AvailableTransport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableTransport {
    #[serde(rename = "type")]
    pub r#type: PairingTransportType,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_id: Option<String>,
    pub label: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex_encode(&hasher.finalize())
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn generate_pairing_id() -> String {
    format!("pair-{}", uuid::Uuid::new_v4())
}

fn generate_secret() -> String {
    format!(
        "pair-secret-{}-{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn generate_client_id() -> String {
    format!("ct-{}", uuid::Uuid::new_v4())
}

fn generate_client_token() -> String {
    format!("ct-{}", uuid::Uuid::new_v4())
}

fn is_expired(record: &PairingRecord) -> bool {
    match chrono::DateTime::parse_from_rfc3339(&record.expires_at) {
        Ok(expires) => {
            let now = chrono::Utc::now();
            now > expires.with_timezone(&chrono::Utc)
        }
        Err(_) => false,
    }
}

fn is_relay_connection(ctx: &ExtensionContext) -> bool {
    ctx.connection_id.starts_with("relay-")
}

fn build_create_transports(ctx: &ExtensionContext, allowed: &[String]) -> Vec<PairingTransport> {
    let is_relay = is_relay_connection(ctx);
    let mut transports = Vec::new();
    for t in allowed {
        match t.as_str() {
            VALID_TRANSPORT_DIRECT => transports.push(PairingTransport {
                r#type: PairingTransportType::Direct,
                url: None,
                relay_id: None,
            }),
            VALID_TRANSPORT_RELAY if is_relay => transports.push(PairingTransport {
                r#type: PairingTransportType::Relay,
                url: None,
                relay_id: Some(ctx.connection_id.clone()),
            }),
            _ => {}
        }
    }
    transports
}

fn build_redeem_transport(ctx: &ExtensionContext) -> PairingTransport {
    if is_relay_connection(ctx) {
        PairingTransport {
            r#type: PairingTransportType::Relay,
            url: None,
            relay_id: Some(ctx.connection_id.clone()),
        }
    } else {
        PairingTransport {
            r#type: PairingTransportType::Direct,
            url: None,
            relay_id: None,
        }
    }
}

fn build_available_transports(ctx: &ExtensionContext) -> Vec<AvailableTransport> {
    let is_relay = is_relay_connection(ctx);
    vec![
        AvailableTransport {
            r#type: PairingTransportType::Direct,
            available: true,
            url: None,
            relay_id: None,
            label: "Local Network".to_string(),
        },
        AvailableTransport {
            r#type: PairingTransportType::Relay,
            available: is_relay,
            url: None,
            relay_id: if is_relay {
                Some(ctx.connection_id.clone())
            } else {
                None
            },
            label: "Cloud Relay".to_string(),
        },
    ]
}

fn ensure_object_params(method: &str, params: &Value) -> Result<(), ExtensionError> {
    if !params.is_object() {
        return Err(ExtensionError::invalid_params(format!(
            "{method} params must be a JSON object"
        )));
    }
    Ok(())
}

fn internal_error(context: &str, err: impl std::fmt::Display) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("{context}: {err}"))),
    }
}

#[async_trait]
impl ExtensionHandler for PairingHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "create" => handle_create(params, ctx, &self.store),
            "redeem" => handle_redeem(params, ctx, &self.store),
            "pending_list" => handle_pending_list(params, ctx, &self.store),
            "cancel" => handle_cancel(params, ctx, &self.store),
            "transports" => handle_transports(params, ctx),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "create": true,
            "redeem": true,
            "pending_list": true,
            "cancel": true,
            "transports": true
        })
    }
}

fn handle_create(
    params: Value,
    ctx: &ExtensionContext,
    store: &PairingStore,
) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "pairing", "create")?;

    ensure_object_params("create", &params)?;
    let p: PairingCreateParams = serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid create params: {e}")))?;

    let ttl = p.ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS);
    if !(MIN_TTL_SECONDS..=MAX_TTL_SECONDS).contains(&ttl) {
        return Err(ExtensionError::invalid_params(format!(
            "ttlSeconds must be between {MIN_TTL_SECONDS} and {MAX_TTL_SECONDS}"
        )));
    }

    let allowed = p.allowed_transports.unwrap_or_else(|| {
        vec![
            VALID_TRANSPORT_DIRECT.to_string(),
            VALID_TRANSPORT_RELAY.to_string(),
        ]
    });

    for t in &allowed {
        if t != VALID_TRANSPORT_DIRECT && t != VALID_TRANSPORT_RELAY {
            return Err(ExtensionError::invalid_params(format!(
                "invalid transport type '{t}'; must be '{VALID_TRANSPORT_DIRECT}' or '{VALID_TRANSPORT_RELAY}'"
            )));
        }
    }

    let now = chrono::Utc::now();
    let created_at = now.to_rfc3339();
    let expires_at = (now + chrono::Duration::seconds(ttl as i64)).to_rfc3339();
    let pairing_id = generate_pairing_id();
    let secret = generate_secret();
    let secret_hash = sha256_hex(&secret);

    let transports = build_create_transports(ctx, &allowed);

    let record = PairingRecord {
        pairing_id: pairing_id.clone(),
        secret_hash,
        created_at,
        expires_at: expires_at.clone(),
        attempts: 0,
        redeemed: false,
        allowed_transports: allowed,
    };
    store.insert(record);

    let payload = PairingPayload {
        secret,
        pairing_id,
        expires_at,
        transports,
    };
    serde_json::to_value(payload).map_err(|e| internal_error("create serialization failed", e))
}

fn handle_redeem(
    params: Value,
    ctx: &ExtensionContext,
    store: &PairingStore,
) -> Result<Value, ExtensionError> {
    ensure_object_params("redeem", &params)?;
    let p: PairingRedeemParams = serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid redeem params: {e}")))?;

    if p.secret.trim().is_empty() {
        return Err(ExtensionError::forbidden("invalid or unknown secret"));
    }
    if p.client_info.name.trim().is_empty() {
        return Err(ExtensionError::invalid_params(
            "clientInfo.name must not be empty",
        ));
    }
    if p.client_info.version.trim().is_empty() {
        return Err(ExtensionError::invalid_params(
            "clientInfo.version must not be empty",
        ));
    }
    if p.client_info.platform.trim().is_empty() {
        return Err(ExtensionError::invalid_params(
            "clientInfo.platform must not be empty",
        ));
    }

    let secret_hash = sha256_hex(&p.secret);
    let result = store.try_redeem(&secret_hash);

    match result {
        RedeemResult::Success { pairing_id } => {
            let client_token = generate_client_token();
            let client_token_hash = sha256_hex(&client_token);
            let client_id = generate_client_id();
            let created_at = now_iso();
            let entry = ClientAuthEntry {
                client_id: client_id.clone(),
                name: p.client_info.name.clone(),
                platform: p.client_info.platform.clone(),
                auth_method: "pairing".to_string(),
                created_at: created_at.clone(),
                last_active: Some(created_at),
                revoked: false,
                revoked_at: None,
                scope: vec!["session:read".to_string()],
                token_hash: client_token_hash,
            };
            store.store_client_token(entry);
            let transport = build_redeem_transport(ctx);
            let response = PairingRedeemResponse {
                pairing_id,
                redeemed: true,
                client_token,
                transport,
            };
            serde_json::to_value(response)
                .map_err(|e| internal_error("redeem serialization failed", e))
        }
        RedeemResult::AlreadyRedeemed => Err(ExtensionError::forbidden("pairing already redeemed")),
        RedeemResult::Expired => Err(ExtensionError::forbidden("pairing has expired")),
        RedeemResult::AttemptsExceeded { pairing_id } => Err(ExtensionError::not_found(format!(
            "pairing '{pairing_id}' was removed due to too many failed attempts"
        ))),
        RedeemResult::NoMatch { .. } => Err(ExtensionError::forbidden("invalid or unknown secret")),
        RedeemResult::NotFound => Err(ExtensionError::not_found(
            "pairing not found (it may have been canceled, removed, or expired)",
        )),
    }
}

fn handle_pending_list(
    params: Value,
    ctx: &ExtensionContext,
    store: &PairingStore,
) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "pairing", "pending_list")?;

    ensure_object_params("pending_list", &params)?;
    let pagination: PaginationParams = serde_json::from_value(params.clone())
        .map_err(|e| ExtensionError::invalid_params(format!("invalid pending_list params: {e}")))?;

    if let Some(limit) = pagination.limit {
        if limit == 0 {
            return Err(ExtensionError::invalid_params("limit must be >= 1"));
        }
    }

    let pending = store.list_pending();
    let mut items: Vec<PendingPairing> = pending
        .iter()
        .map(|r| PendingPairing {
            pairing_id: r.pairing_id.clone(),
            created_at: r.created_at.clone(),
            expires_at: r.expires_at.clone(),
            attempts: r.attempts,
        })
        .collect();
    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let limit = pagination.limit_or_default(DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
    let offset = pagination
        .decode_cursor::<serde_json::Value>()
        .map_err(|e| ExtensionError::invalid_params(format!("invalid cursor: {e}")))?
        .and_then(|v| v.get("offset").and_then(|o| o.as_u64()))
        .map(|n| n as usize)
        .unwrap_or(0);

    let items_json: Vec<Value> = items
        .into_iter()
        .map(|i| serde_json::to_value(&i).unwrap_or(Value::Null))
        .collect();

    let result = PaginatedResult::from_slice(items_json, offset, limit);
    Ok(result.to_json())
}

fn handle_cancel(
    params: Value,
    ctx: &ExtensionContext,
    store: &PairingStore,
) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "pairing", "cancel")?;

    ensure_object_params("cancel", &params)?;
    let p: PairingCancelParams = serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid cancel params: {e}")))?;

    if p.pairing_id.trim().is_empty() {
        return Err(ExtensionError::invalid_params(
            "pairingId must not be empty",
        ));
    }

    match store.cancel(&p.pairing_id) {
        CancelResult::Cancelled => {
            let response = PairingCancelResponse {
                pairing_id: p.pairing_id,
                cancelled: true,
            };
            serde_json::to_value(response)
                .map_err(|e| internal_error("cancel serialization failed", e))
        }
        CancelResult::NotFound => Err(ExtensionError::not_found(format!(
            "pairing '{}' is not pending",
            p.pairing_id
        ))),
    }
}

fn handle_transports(_params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let transports = build_available_transports(ctx);
    let response = PairingTransportsResponse { transports };
    serde_json::to_value(response).map_err(|e| internal_error("transports serialization failed", e))
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

    fn default_client_info() -> Value {
        json!({
            "name": "TestClient",
            "version": "1.0.0",
            "platform": "ios"
        })
    }

    async fn create_pairing(handler: &PairingHandler, ctx: &ExtensionContext) -> Value {
        handler
            .handle("create", json!({}), ctx)
            .await
            .expect("create failed")
    }

    async fn redeem(
        handler: &PairingHandler,
        ctx: &ExtensionContext,
        secret: &str,
    ) -> Result<Value, ExtensionError> {
        handler
            .handle(
                "redeem",
                json!({
                    "secret": secret,
                    "clientInfo": default_client_info()
                }),
                ctx,
            )
            .await
    }

    fn sha256_hex_pub(s: &str) -> String {
        sha256_hex(s)
    }

    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}

    #[tokio::test]
    async fn test_create_returns_required_fields() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        assert!(result["secret"].is_string());
        assert!(result["pairingId"].is_string());
        assert!(result["expiresAt"].is_string());
        assert!(result["transports"].is_array());
        assert_eq!(result.as_object().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_create_secret_format() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        let secret = result["secret"].as_str().unwrap();
        assert!(secret.starts_with("pair-secret-"));
        assert!(secret.len() > 20);
    }

    #[tokio::test]
    async fn test_create_pairing_id_format() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        let pid = result["pairingId"].as_str().unwrap();
        assert!(pid.starts_with("pair-"));
    }

    #[tokio::test]
    async fn test_create_default_ttl_is_300() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        let pid = result["pairingId"].as_str().unwrap().to_string();
        let record = handler.store().get(&pid).unwrap();
        let created = chrono::DateTime::parse_from_rfc3339(&record.created_at).unwrap();
        let expires = chrono::DateTime::parse_from_rfc3339(&record.expires_at).unwrap();
        let diff = (expires - created).num_seconds();
        assert_eq!(diff, 300);
    }

    #[tokio::test]
    async fn test_create_explicit_ttl() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("create", json!({"ttlSeconds": 60}), &ctx)
            .await
            .unwrap();
        let pid = result["pairingId"].as_str().unwrap().to_string();
        let record = handler.store().get(&pid).unwrap();
        let created = chrono::DateTime::parse_from_rfc3339(&record.created_at).unwrap();
        let expires = chrono::DateTime::parse_from_rfc3339(&record.expires_at).unwrap();
        assert_eq!((expires - created).num_seconds(), 60);
    }

    #[tokio::test]
    async fn test_create_ttl_zero_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("create", json!({"ttlSeconds": 0}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_create_ttl_above_max_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("create", json!({"ttlSeconds": 86401}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_create_ttl_max_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("create", json!({"ttlSeconds": 86400}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_ttl_min_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("create", json!({"ttlSeconds": 1}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_allowed_transports_unknown_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("create", json!({"allowedTransports": ["unknown"]}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_create_allowed_transports_direct_only() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("create", json!({"allowedTransports": ["direct"]}), &ctx)
            .await
            .unwrap();
        let transports = result["transports"].as_array().unwrap();
        assert_eq!(transports.len(), 1);
        assert_eq!(transports[0]["type"], "direct");
    }

    #[tokio::test]
    async fn test_create_allowed_transports_relay_included_on_relay_conn() {
        let ctx = make_ctx("relay-xyz", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle(
                "create",
                json!({"allowedTransports": ["direct", "relay"]}),
                &ctx,
            )
            .await
            .unwrap();
        let transports = result["transports"].as_array().unwrap();
        assert_eq!(transports.len(), 2);
        let types: Vec<&str> = transports
            .iter()
            .map(|t| t["type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"direct"));
        assert!(types.contains(&"relay"));
        let relay = transports.iter().find(|t| t["type"] == "relay").unwrap();
        assert_eq!(relay["relayId"], "relay-xyz");
    }

    #[tokio::test]
    async fn test_create_allowed_transports_relay_excluded_off_relay() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle(
                "create",
                json!({"allowedTransports": ["direct", "relay"]}),
                &ctx,
            )
            .await
            .unwrap();
        let transports = result["transports"].as_array().unwrap();
        assert_eq!(transports.len(), 1);
        assert_eq!(transports[0]["type"], "direct");
    }

    #[tokio::test]
    async fn test_create_empty_principal_returns_forbidden() {
        let ctx = make_ctx("conn-1", "");
        let handler = PairingHandler::new();
        let err = handler.handle("create", json!({}), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_create_secret_hash_stored_not_plaintext() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        let secret = result["secret"].as_str().unwrap();
        let pid = result["pairingId"].as_str().unwrap();
        let record = handler.store().get(pid).unwrap();
        assert_ne!(record.secret_hash, secret);
        assert_eq!(record.secret_hash, sha256_hex_pub(secret));
        assert_eq!(record.secret_hash.len(), 64);
    }

    #[tokio::test]
    async fn test_redeem_valid_secret_succeeds() {
        let ctx = make_ctx("relay-xyz", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let result = redeem(&handler, &ctx, &secret).await.unwrap();
        assert!(result["pairingId"].is_string());
        assert_eq!(result["redeemed"], true);
        let token = result["clientToken"].as_str().unwrap();
        assert!(token.starts_with("ct-"));
        assert_eq!(result["transport"]["type"], "relay");
        assert_eq!(result["transport"]["relayId"], "relay-xyz");
    }

    #[tokio::test]
    async fn test_redeem_direct_transport() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let result = redeem(&handler, &ctx, &secret).await.unwrap();
        assert_eq!(result["transport"]["type"], "direct");
    }

    #[tokio::test]
    async fn test_redeem_marks_pairing_redeemed() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let pid = created["pairingId"].as_str().unwrap().to_string();
        redeem(&handler, &ctx, &secret).await.unwrap();
        let record = handler.store().get(&pid).unwrap();
        assert!(record.redeemed);
        assert_eq!(record.attempts, 0);
    }

    #[tokio::test]
    async fn test_redeem_second_time_returns_forbidden() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        redeem(&handler, &ctx, &secret).await.unwrap();
        let err = redeem(&handler, &ctx, &secret).await.unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_redeem_empty_secret_returns_forbidden() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        create_pairing(&handler, &ctx).await;
        let err = redeem(&handler, &ctx, "").await.unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_redeem_whitespace_secret_returns_forbidden() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        create_pairing(&handler, &ctx).await;
        let err = redeem(&handler, &ctx, "   \t\n").await.unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_redeem_wrong_secret_with_pending_returns_forbidden() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        create_pairing(&handler, &ctx).await;
        let err = redeem(&handler, &ctx, "pair-secret-wrong-wrong")
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_redeem_no_pending_records_returns_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = redeem(&handler, &ctx, "pair-secret-nothing-nothing")
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn test_redeem_after_cancel_returns_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let pid = created["pairingId"].as_str().unwrap().to_string();
        let cancel_result = handler
            .handle("cancel", json!({"pairingId": pid}), &ctx)
            .await
            .unwrap();
        assert_eq!(cancel_result["cancelled"], true);
        let err = redeem(&handler, &ctx, &secret).await.unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn test_redeem_missing_client_info_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("redeem", json!({"secret": "pair-secret-x-y"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_missing_name_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"version": "1.0.0", "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_missing_version_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "X", "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_missing_platform_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "X", "version": "1.0.0"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_null_version_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "X", "version": null, "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_empty_name_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "", "version": "1.0.0", "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_empty_version_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "X", "version": "", "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_empty_platform_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "X", "version": "1.0.0", "platform": ""}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_whitespace_name_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle(
                "redeem",
                json!({
                    "secret": "pair-secret-x-y",
                    "clientInfo": {"name": "   ", "version": "1.0.0", "platform": "ios"}
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_redeem_five_wrong_attempts_returns_not_found_and_removes_pairing() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap().to_string();
        for i in 0..4 {
            let err = redeem(&handler, &ctx, &format!("pair-secret-wrong-{i}"))
                .await
                .unwrap_err();
            assert_eq!(err.code, -32002, "attempt {i} should be forbidden");
        }
        let err = redeem(&handler, &ctx, "pair-secret-wrong-final")
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
        assert!(handler.store().get(&pid).is_none());
    }

    #[tokio::test]
    async fn test_redeem_attempts_increment_on_record() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap().to_string();
        redeem(&handler, &ctx, "pair-secret-wrong-1")
            .await
            .unwrap_err();
        redeem(&handler, &ctx, "pair-secret-wrong-2")
            .await
            .unwrap_err();
        let record = handler.store().get(&pid).unwrap();
        assert_eq!(record.attempts, 2);
        assert!(!record.redeemed);
    }

    #[tokio::test]
    async fn test_pending_list_empty_store_returns_empty() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result["items"].is_array());
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
        assert_eq!(result["hasMore"], false);
    }

    #[tokio::test]
    async fn test_pending_list_returns_created_pairing() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["pairingId"], pid);
    }

    #[tokio::test]
    async fn test_pending_list_excludes_redeemed() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        redeem(&handler, &ctx, &secret).await.unwrap();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_pending_list_excludes_canceled() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap().to_string();
        handler
            .handle("cancel", json!({"pairingId": pid}), &ctx)
            .await
            .unwrap();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_pending_list_default_limit_20() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        for _ in 0..5 {
            create_pairing(&handler, &ctx).await;
        }
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 5);
        assert_eq!(result["hasMore"], false);
    }

    #[tokio::test]
    async fn test_pending_list_with_limit() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        for _ in 0..3 {
            create_pairing(&handler, &ctx).await;
        }
        let result = handler
            .handle("pending_list", json!({"limit": 2}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["hasMore"], true);
        assert!(result["nextCursor"].is_string());
    }

    #[tokio::test]
    async fn test_pending_list_limit_clamps_to_max() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler
            .handle("pending_list", json!({"limit": 9999}), &ctx)
            .await
            .unwrap();
        assert!(result["items"].is_array());
    }

    #[tokio::test]
    async fn test_pending_list_limit_zero_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("pending_list", json!({"limit": 0}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_pending_list_invalid_hex_cursor_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("pending_list", json!({"cursor": "not-valid-hex!@#"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_pending_list_cursor_round_trip() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        for _ in 0..3 {
            create_pairing(&handler, &ctx).await;
        }
        let page1 = handler
            .handle("pending_list", json!({"limit": 2}), &ctx)
            .await
            .unwrap();
        assert_eq!(page1["items"].as_array().unwrap().len(), 2);
        assert_eq!(page1["hasMore"], true);
        let cursor = page1["nextCursor"].as_str().unwrap().to_string();
        let page2 = handler
            .handle("pending_list", json!({"limit": 2, "cursor": cursor}), &ctx)
            .await
            .unwrap();
        assert_eq!(page2["items"].as_array().unwrap().len(), 1);
        assert_eq!(page2["hasMore"], false);
    }

    #[tokio::test]
    async fn test_pending_list_requires_principal() {
        let ctx = make_ctx("conn-1", "");
        let handler = PairingHandler::new();
        let err = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_pending_list_no_secrets() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        let s = result.to_string();
        assert!(!s.contains(secret));
        assert!(!s.contains("pair-secret-"));
    }

    #[tokio::test]
    async fn test_pending_list_items_have_no_attempts_secret_field() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        create_pairing(&handler, &ctx).await;
        redeem(&handler, &ctx, "pair-secret-wrong")
            .await
            .unwrap_err();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        let item = &result["items"][0];
        assert!(item["pairingId"].is_string());
        assert!(item["createdAt"].is_string());
        assert!(item["expiresAt"].is_string());
        assert!(item["attempts"].is_number());
        let item_obj = item.as_object().unwrap();
        assert!(!item_obj.contains_key("secret"));
        assert!(!item_obj.contains_key("secretHash"));
        assert!(!item_obj.contains_key("clientToken"));
    }

    #[tokio::test]
    async fn test_cancel_pending_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap().to_string();
        let result = handler
            .handle("cancel", json!({"pairingId": pid}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["pairingId"], pid);
        assert_eq!(result["cancelled"], true);
        assert!(handler.store().get(&pid).is_none());
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_pairing_returns_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("cancel", json!({"pairingId": "pair-nonexistent-id"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn test_cancel_empty_pairing_id_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("cancel", json!({"pairingId": ""}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_cancel_whitespace_pairing_id_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("cancel", json!({"pairingId": "   "}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_cancel_missing_pairing_id_returns_invalid_params() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler.handle("cancel", json!({}), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_cancel_redeemed_pairing_returns_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let pid = created["pairingId"].as_str().unwrap().to_string();
        redeem(&handler, &ctx, &secret).await.unwrap();
        let err = handler
            .handle("cancel", json!({"pairingId": pid}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn test_cancel_requires_principal() {
        let ctx = make_ctx("conn-1", "");
        let handler = PairingHandler::new();
        let err = handler
            .handle("cancel", json!({"pairingId": "pair-anything"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn test_cancel_response_no_secrets() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap();
        let pid = created["pairingId"].as_str().unwrap().to_string();
        let result = handler
            .handle("cancel", json!({"pairingId": pid}), &ctx)
            .await
            .unwrap();
        let s = result.to_string();
        assert!(!s.contains(secret));
        assert!(!s.contains("pair-secret-"));
        assert!(!s.contains("ct-"));
    }

    #[tokio::test]
    async fn test_transports_returns_two_items() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        let transports = result["transports"].as_array().unwrap();
        assert_eq!(transports.len(), 2);
    }

    #[tokio::test]
    async fn test_transports_direct_first_then_relay() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        let transports = result["transports"].as_array().unwrap();
        assert_eq!(transports[0]["type"], "direct");
        assert_eq!(transports[1]["type"], "relay");
    }

    #[tokio::test]
    async fn test_transports_direct_always_available() {
        let ctx = make_ctx("conn-1", "");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transports"][0]["available"], true);
    }

    #[tokio::test]
    async fn test_transports_relay_available_on_relay() {
        let ctx = make_ctx("relay-xyz", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transports"][1]["available"], true);
        assert_eq!(result["transports"][1]["relayId"], "relay-xyz");
    }

    #[tokio::test]
    async fn test_transports_relay_unavailable_off_relay() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        assert_eq!(result["transports"][1]["available"], false);
        assert!(result["transports"][1]["relayId"].is_null());
    }

    #[tokio::test]
    async fn test_transports_labels_set() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        assert!(result["transports"][0]["label"].is_string());
        assert!(result["transports"][1]["label"].is_string());
    }

    #[tokio::test]
    async fn test_transports_field_types() {
        let ctx = make_ctx("relay-xyz", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        assert!(result["transports"][0]["type"].is_string());
        assert!(result["transports"][0]["available"].is_boolean());
        assert!(result["transports"][0]["label"].is_string());
        assert!(result["transports"][1]["type"].is_string());
        assert!(result["transports"][1]["available"].is_boolean());
        assert!(result["transports"][1]["label"].is_string());
        assert!(result["transports"][1]["relayId"].is_string());
    }

    #[tokio::test]
    async fn test_transports_no_secrets() {
        let ctx = make_ctx("relay-xyz", "super-secret-principal-xyz");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(!s.contains("super-secret-principal-xyz"));
        assert!(!s.contains("pair-secret-"));
        assert!(!s.contains("ct-"));
    }

    #[tokio::test]
    async fn test_transports_with_null_params_succeeds() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", Value::Null, &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unknown_method_returns_method_not_found() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("unknown_method", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }

    #[tokio::test]
    async fn test_capabilities_has_exactly_five_keys() {
        let handler = PairingHandler::new();
        let caps = handler.capabilities();
        let obj = caps.as_object().unwrap();
        assert_eq!(obj.len(), 5);
        assert_eq!(caps["create"], true);
        assert_eq!(caps["redeem"], true);
        assert_eq!(caps["pending_list"], true);
        assert_eq!(caps["cancel"], true);
        assert_eq!(caps["transports"], true);
    }

    #[tokio::test]
    async fn test_capabilities_keys_are_snake_case() {
        let handler = PairingHandler::new();
        let caps = handler.capabilities();
        let keys: Vec<&String> = caps.as_object().unwrap().keys().collect();
        assert!(keys.iter().any(|key| *key == "create"));
        assert!(keys.iter().any(|key| *key == "redeem"));
        assert!(keys.iter().any(|key| *key == "pending_list"));
        assert!(keys.iter().any(|key| *key == "cancel"));
        assert!(keys.iter().any(|key| *key == "transports"));
    }

    #[tokio::test]
    async fn test_params_null_returns_invalid_params_for_create() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("create", Value::Null, &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_array_returns_invalid_params_for_create() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler.handle("create", json!([]), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_string_returns_invalid_params_for_create() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("create", json!("string"), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_number_returns_invalid_params_for_create() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler.handle("create", json!(42), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_null_returns_invalid_params_for_redeem() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("redeem", Value::Null, &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_array_returns_invalid_params_for_redeem() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler.handle("redeem", json!([]), &ctx).await.unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_null_returns_invalid_params_for_cancel() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("cancel", Value::Null, &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_params_null_returns_invalid_params_for_pending_list() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let err = handler
            .handle("pending_list", Value::Null, &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_handler_is_send() {
        _assert_send::<PairingHandler>();
        _assert_send::<Arc<PairingHandler>>();
    }

    #[tokio::test]
    async fn test_handler_is_sync() {
        _assert_sync::<PairingHandler>();
        _assert_sync::<Arc<PairingHandler>>();
    }

    #[tokio::test]
    async fn test_store_is_send_and_sync() {
        _assert_send::<PairingStore>();
        _assert_sync::<PairingStore>();
    }

    #[tokio::test]
    async fn test_arc_store_is_send_and_sync() {
        _assert_send::<Arc<PairingStore>>();
        _assert_sync::<Arc<PairingStore>>();
    }

    #[tokio::test]
    async fn test_registry_dispatch_create() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let result = registry
            .dispatch("_loomdesk.dev/pairing/create", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result["secret"].is_string());
        assert!(result["pairingId"].is_string());
    }

    #[tokio::test]
    async fn test_registry_dispatch_redeem() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let created = registry
            .dispatch("_loomdesk.dev/pairing/create", json!({}), &ctx)
            .await
            .unwrap();
        let secret = created["secret"].as_str().unwrap().to_string();
        let result = registry
            .dispatch(
                "_loomdesk.dev/pairing/redeem",
                json!({
                    "secret": secret,
                    "clientInfo": default_client_info()
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result["clientToken"].is_string());
    }

    #[tokio::test]
    async fn test_registry_dispatch_pending_list() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let result = registry
            .dispatch("_loomdesk.dev/pairing/pending_list", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result["items"].is_array());
    }

    #[tokio::test]
    async fn test_registry_dispatch_cancel() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let created = registry
            .dispatch("_loomdesk.dev/pairing/create", json!({}), &ctx)
            .await
            .unwrap();
        let pid = created["pairingId"].as_str().unwrap().to_string();
        let result = registry
            .dispatch(
                "_loomdesk.dev/pairing/cancel",
                json!({"pairingId": pid}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["cancelled"], true);
    }

    #[tokio::test]
    async fn test_registry_dispatch_transports() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let result = registry
            .dispatch("_loomdesk.dev/pairing/transports", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result["transports"].is_array());
    }

    #[tokio::test]
    async fn test_registry_dispatch_unknown_method_returns_method_not_found() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let ctx = make_ctx("conn-1", "user");
        let err = registry
            .dispatch("_loomdesk.dev/pairing/unknown", json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }

    #[tokio::test]
    async fn test_registry_capability_snapshot_wraps_under_pairing() {
        let mut registry = super::super::ExtensionRegistry::new();
        registry.register("pairing", Arc::new(PairingHandler::new()));
        let snapshot = registry.build_capability_snapshot();
        assert!(snapshot["pairing"].is_object());
        assert_eq!(snapshot["pairing"]["create"], true);
        assert_eq!(snapshot["pairing"]["redeem"], true);
        assert_eq!(snapshot["pairing"]["pending_list"], true);
        assert_eq!(snapshot["pairing"]["cancel"], true);
        assert_eq!(snapshot["pairing"]["transports"], true);
        assert_eq!(snapshot["pairing"].as_object().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn test_concurrent_create_and_redeem_distinct_pairings() {
        use std::sync::Arc;
        let handler = Arc::new(PairingHandler::new());
        let mut handles = Vec::new();
        for i in 0..5 {
            let h = Arc::clone(&handler);
            handles.push(tokio::spawn(async move {
                let ctx = make_ctx("conn-1", "user");
                let created = h
                    .handle("create", json!({"ttlSeconds": 60 + i as u64}), &ctx)
                    .await
                    .unwrap();
                let secret = created["secret"].as_str().unwrap().to_string();
                let result = redeem(&h, &ctx, &secret).await;
                result.is_ok()
            }));
        }
        let mut success = 0;
        for h in handles {
            if h.await.unwrap() {
                success += 1;
            }
        }
        assert_eq!(success, 5);
    }

    #[tokio::test]
    async fn test_concurrent_redeem_same_pairing_serialized() {
        use std::sync::Arc;
        let handler = Arc::new(PairingHandler::new());
        let ctx = make_ctx("conn-1", "user");
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let mut handles = Vec::new();
        for _ in 0..10 {
            let h = Arc::clone(&handler);
            let secret = secret.clone();
            handles.push(tokio::spawn(async move {
                let ctx = make_ctx("conn-1", "user");
                redeem(&h, &ctx, &secret).await
            }));
        }
        let mut success = 0;
        let mut forbidden = 0;
        for h in handles {
            match h.await.unwrap() {
                Ok(_) => success += 1,
                Err(e) if e.code == -32002 => forbidden += 1,
                Err(e) => panic!("unexpected error code {}: {}", e.code, e.message),
            }
        }
        assert_eq!(success, 1);
        assert_eq!(forbidden, 9);
    }

    #[tokio::test]
    async fn test_with_store_shares_state() {
        let store = Arc::new(PairingStore::new());
        let h1 = PairingHandler::with_store(Arc::clone(&store));
        let h2 = PairingHandler::with_store(Arc::clone(&store));
        let ctx = make_ctx("conn-1", "user");
        let created = create_pairing(&h1, &ctx).await;
        let pid = created["pairingId"].as_str().unwrap().to_string();
        assert!(h2.store().get(&pid).is_some());
    }

    #[tokio::test]
    async fn test_response_keys_camel_case_for_create() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("create", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(s.contains("\"pairingId\""));
        assert!(s.contains("\"expiresAt\""));
        assert!(!s.contains("pairing_id"));
        assert!(!s.contains("expires_at"));
    }

    #[tokio::test]
    async fn test_response_keys_camel_case_for_redeem() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let created = create_pairing(&handler, &ctx).await;
        let secret = created["secret"].as_str().unwrap().to_string();
        let result = redeem(&handler, &ctx, &secret).await.unwrap();
        let s = result.to_string();
        assert!(s.contains("\"pairingId\""));
        assert!(s.contains("\"clientToken\""));
        assert!(!s.contains("client_token"));
        assert!(!s.contains("pairing_id"));
    }

    #[tokio::test]
    async fn test_response_keys_camel_case_for_transports() {
        let ctx = make_ctx("relay-xyz", "user");
        let handler = PairingHandler::new();
        let result = handler.handle("transports", json!({}), &ctx).await.unwrap();
        let s = result.to_string();
        assert!(s.contains("\"relayId\""));
        assert!(!s.contains("relay_id"));
    }

    #[tokio::test]
    async fn test_pending_list_excludes_only_redeemed_or_canceled() {
        let ctx = make_ctx("conn-1", "user");
        let handler = PairingHandler::new();
        let c1 = create_pairing(&handler, &ctx).await;
        let c1_secret = c1["secret"].as_str().unwrap().to_string();
        let c2 = create_pairing(&handler, &ctx).await;
        let c2_pid = c2["pairingId"].as_str().unwrap().to_string();
        let _c3 = create_pairing(&handler, &ctx).await;
        redeem(&handler, &ctx, &c1_secret).await.unwrap();
        handler
            .handle("cancel", json!({"pairingId": c2_pid}), &ctx)
            .await
            .unwrap();
        let result = handler
            .handle("pending_list", json!({}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn test_default_impl_equivalent_to_new() {
        let h_new = PairingHandler::new();
        let h_default: PairingHandler = Default::default();
        let ctx = make_ctx("conn-1", "user");
        let r1 = h_new.handle("transports", json!({}), &ctx).await.unwrap();
        let r2 = h_default
            .handle("transports", json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(r1.to_string(), r2.to_string());
    }
}
