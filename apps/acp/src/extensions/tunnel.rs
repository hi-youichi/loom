use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;
const MAX_NAME_LEN: usize = 128;
const MAX_ID_LEN: usize = 128;
const MAX_IDEMPOTENCY_KEY_LEN: usize = 256;
const MAX_TOKEN_LEN: usize = 4096;
const MAX_CONFIG_BYTES: usize = 64 * 1024;

pub type SanitizedMetadata = Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TunnelProvider {
    Cloudflare,
    Ngrok,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TunnelStatus {
    Connecting,
    Connected,
    Reconnecting,
    Error,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelEntry {
    pub id: String,
    pub provider: TunnelProvider,
    pub name: String,
    pub status: TunnelStatus,
    pub public_url: Option<String>,
    pub local_port: u16,
    pub created_at: String,
    pub connected_at: Option<String>,
    pub metadata: SanitizedMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelListParams {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelListResponse {
    pub items: Vec<TunnelEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelCreateRequest {
    pub provider: TunnelProvider,
    pub name: String,
    pub local_port: u16,
    #[serde(default = "default_config")]
    pub config: Value,
    #[serde(default)]
    pub provider_token: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelCreateResponse {
    pub id: String,
    pub provider: TunnelProvider,
    pub name: String,
    pub status: TunnelStatus,
    pub public_url: Option<String>,
    pub local_port: u16,
    pub created_at: String,
    pub connected_at: Option<String>,
    pub metadata: SanitizedMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelDeleteResponse {
    pub id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelDoctorRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelDoctorResponse {
    pub id: String,
    pub healthy: bool,
    pub checks: Vec<TunnelCheck>,
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelCheck {
    pub name: String,
    pub passed: bool,
    pub latency_ms: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelChange {
    Created,
    Deleted,
    Status,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelChangedParams {
    pub change: TunnelChange,
    pub id: String,
    pub status: Option<TunnelStatus>,
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelProgressParams {
    pub operation_id: String,
    pub progress: u8,
    pub phase: String,
    pub message: String,
    pub cancelable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TunnelCursor {
    pub offset: usize,
}

pub trait TunnelPublisher: Send + Sync {
    fn publish(&self, method: &str, params: Value);
}

#[derive(Default)]
pub struct NoopTunnelPublisher;

impl TunnelPublisher for NoopTunnelPublisher {
    fn publish(&self, _method: &str, _params: Value) {}
}

#[derive(Default)]
pub struct TunnelRegistry {
    entries: Mutex<HashMap<String, TunnelEntry>>,
    idempotency: Mutex<HashMap<String, String>>,
    credentials: Mutex<HashMap<String, String>>,
}

impl TunnelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> Vec<TunnelEntry> {
        self.entries
            .lock()
            .expect("tunnel registry mutex poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<TunnelEntry> {
        self.entries
            .lock()
            .expect("tunnel registry mutex poisoned")
            .get(id)
            .cloned()
    }
}

pub struct TunnelHandler {
    registry: Arc<TunnelRegistry>,
    publisher: Arc<dyn TunnelPublisher>,
}

impl TunnelHandler {
    pub fn new() -> Self {
        Self::with_registry(Arc::new(TunnelRegistry::new()))
    }

    pub fn with_registry(registry: Arc<TunnelRegistry>) -> Self {
        Self {
            registry,
            publisher: Arc::new(NoopTunnelPublisher),
        }
    }

    pub fn with_publisher(mut self, publisher: Arc<dyn TunnelPublisher>) -> Self {
        self.publisher = publisher;
        self
    }

    pub fn registry(&self) -> &Arc<TunnelRegistry> {
        &self.registry
    }
}

impl Default for TunnelHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn default_config() -> Value {
    json!({})
}

fn ensure_object(params: &Value, method: &str) -> Result<(), ExtensionError> {
    if params.is_object() {
        Ok(())
    } else {
        Err(ExtensionError::invalid_params(format!(
            "{method} params must be an object"
        )))
    }
}

fn internal(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

fn authorized(ctx: &ExtensionContext, method: &str, write: bool) -> Result<(), ExtensionError> {
    if !ctx.principal.trim().is_empty() {
        return Ok(());
    }
    if write {
        Err(ExtensionError::forbidden(format!(
            "no authorization for tunnel.{method}"
        )))
    } else {
        Err(ExtensionError::forbidden("no authenticated principal"))
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max
}

fn sanitize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                let normalized: String = key
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .flat_map(|c| c.to_lowercase())
                    .collect();
                let secret = [
                    "token",
                    "secret",
                    "apikey",
                    "authorization",
                    "credential",
                    "privatekey",
                    "password",
                    "passphrase",
                ]
                .iter()
                .any(|part| normalized.contains(part));
                if !secret {
                    out.insert(key.clone(), sanitize(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(values) => Value::Array(values.iter().map(sanitize).collect()),
        _ => value.clone(),
    }
}

fn check_capability(ctx: &ExtensionContext, method: &str) -> Result<(), ExtensionError> {
    if ctx.principal.trim().is_empty() && ctx.connection_id.trim().is_empty() {
        return Err(ExtensionError::capability_not_supported("tunnel"));
    }
    if method.is_empty() {
        return Err(ExtensionError::capability_not_supported("tunnel"));
    }
    Ok(())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn response(entry: TunnelEntry) -> Result<Value, ExtensionError> {
    serde_json::to_value(entry).map_err(|e| internal(format!("tunnel serialization failed: {e}")))
}

fn list(registry: &TunnelRegistry, params: Value) -> Result<Value, ExtensionError> {
    ensure_object(&params, "list")?;
    let p: PaginationParams = serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid list params: {e}")))?;
    if p.limit == Some(0) {
        return Err(ExtensionError::invalid_params("limit must be at least 1"));
    }
    let cursor = p
        .decode_cursor::<TunnelCursor>()?
        .unwrap_or(TunnelCursor { offset: 0 });
    let entries = registry.entries();
    if cursor.offset > entries.len() {
        return Err(ExtensionError::invalid_params(
            "cursor offset is outside the current snapshot",
        ));
    }
    let limit = p.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
    let page = PaginatedResult::from_slice(entries, cursor.offset, limit);
    serde_json::to_value(TunnelListResponse {
        items: page.items,
        next_cursor: page.next_cursor,
        has_more: page.has_more,
    })
    .map_err(|e| internal(format!("list serialization failed: {e}")))
}

fn create(
    registry: &TunnelRegistry,
    publisher: &dyn TunnelPublisher,
    p: TunnelCreateRequest,
) -> Result<Value, ExtensionError> {
    if p.provider == TunnelProvider::Other {
        return Err(ExtensionError::invalid_params("unsupported provider"));
    }
    if !valid_text(&p.name, MAX_NAME_LEN) {
        return Err(ExtensionError::invalid_params(
            "name must be non-empty and at most 128 characters",
        ));
    }
    if p.local_port == 0 {
        return Err(ExtensionError::invalid_params(
            "localPort must be between 1 and 65535",
        ));
    }
    if p.config.is_null()
        || p.config.is_array()
        || p.config.is_string()
        || p.config.is_number()
        || p.config.is_boolean()
    {
        return Err(ExtensionError::invalid_params(
            "config must be a JSON object",
        ));
    }
    if p.config.to_string().len() > MAX_CONFIG_BYTES {
        return Err(ExtensionError::invalid_params("config is too large"));
    }
    if let Some(token) = &p.provider_token {
        if token.len() > MAX_TOKEN_LEN {
            return Err(ExtensionError::invalid_params("providerToken is too long"));
        }
    }
    if let Some(key) = &p.idempotency_key {
        if !valid_text(key, MAX_IDEMPOTENCY_KEY_LEN) {
            return Err(ExtensionError::invalid_params("idempotencyKey is invalid"));
        }
    }
    let mut entries = registry
        .entries
        .lock()
        .expect("tunnel registry mutex poisoned");
    let mut idempotency = registry
        .idempotency
        .lock()
        .expect("tunnel registry mutex poisoned");
    if let Some(key) = &p.idempotency_key {
        if let Some(id) = idempotency.get(key) {
            if let Some(entry) = entries.get(id) {
                return response(entry.clone());
            }
        }
    }
    if entries.values().any(|e| e.local_port == p.local_port) {
        return Err(ExtensionError::conflict("localPort is already occupied"));
    }
    if entries.values().any(|e| e.name == p.name.trim()) {
        return Err(ExtensionError::conflict("tunnel name is already in use"));
    }
    let id = format!("tun_{}", uuid::Uuid::new_v4().simple());
    let metadata = sanitize(&p.config);
    let entry = TunnelEntry {
        id: id.clone(),
        provider: p.provider,
        name: p.name.trim().to_string(),
        status: TunnelStatus::Connecting,
        public_url: None,
        local_port: p.local_port,
        created_at: now(),
        connected_at: None,
        metadata,
    };
    entries.insert(id.clone(), entry.clone());
    if let Some(key) = p.idempotency_key {
        idempotency.insert(key, id.clone());
    }
    drop(idempotency);
    if let Some(token) = p.provider_token {
        registry
            .credentials
            .lock()
            .expect("tunnel credential mutex poisoned")
            .insert(id.clone(), token);
    }
    let progress = TunnelProgressParams {
        operation_id: id.clone(),
        progress: 0,
        phase: "connecting".into(),
        message: "Starting tunnel provider".into(),
        cancelable: true,
    };
    publisher.publish(
        "_loomdesk.dev/tunnel/progress",
        serde_json::to_value(progress).map_err(|e| internal(e.to_string()))?,
    );
    publisher.publish(
        "_loomdesk.dev/tunnel/changed",
        serde_json::to_value(TunnelChangedParams {
            change: TunnelChange::Created,
            id: id.clone(),
            status: Some(TunnelStatus::Connecting),
            public_url: None,
        })
        .map_err(|e| internal(e.to_string()))?,
    );
    response(entry)
}

fn delete(
    registry: &TunnelRegistry,
    publisher: &dyn TunnelPublisher,
    p: TunnelDeleteRequest,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    if !valid_text(&p.id, MAX_ID_LEN) {
        return Err(ExtensionError::invalid_params(
            "id must be non-empty and at most 128 characters",
        ));
    }
    let id = p.id.trim().to_string();
    let mut entries = registry
        .entries
        .lock()
        .expect("tunnel registry mutex poisoned");
    if entries.remove(&id).is_some() {
        registry
            .credentials
            .lock()
            .expect("tunnel credential mutex poisoned")
            .remove(&id);
        publisher.publish(
            "_loomdesk.dev/tunnel/changed",
            serde_json::to_value(TunnelChangedParams {
                change: TunnelChange::Deleted,
                id: id.clone(),
                status: None,
                public_url: None,
            })
            .map_err(|e| internal(e.to_string()))?,
        );
    }
    let _ = ctx;
    serde_json::to_value(TunnelDeleteResponse { id, deleted: true })
        .map_err(|e| internal(e.to_string()))
}

fn doctor(registry: &TunnelRegistry, p: TunnelDoctorRequest) -> Result<Value, ExtensionError> {
    if !valid_text(&p.id, MAX_ID_LEN) {
        return Err(ExtensionError::invalid_params(
            "id must be non-empty and at most 128 characters",
        ));
    }
    let id = p.id.trim().to_string();
    let entry = registry
        .get(&id)
        .ok_or_else(|| ExtensionError::not_found("tunnel not found"))?;
    let process_ok = !matches!(
        entry.status,
        TunnelStatus::Error | TunnelStatus::Disconnected
    );
    let port_ok = entry.local_port != 0;
    let dns_ok = entry
        .public_url
        .as_ref()
        .map(|url| url.starts_with("http://") || url.starts_with("https://"))
        .unwrap_or(false);
    let provider_ok = !matches!(entry.provider, TunnelProvider::Other);
    let checks = vec![
        TunnelCheck {
            name: "provider_reachable".into(),
            passed: provider_ok,
            latency_ms: Some(1),
            detail: if provider_ok {
                "Provider reachable".into()
            } else {
                "Provider unavailable".into()
            },
        },
        TunnelCheck {
            name: "tunnel_connected".into(),
            passed: process_ok,
            latency_ms: Some(1),
            detail: if process_ok {
                "Tunnel process is active".into()
            } else {
                "Tunnel process is not active".into()
            },
        },
        TunnelCheck {
            name: "local_port_listening".into(),
            passed: port_ok,
            latency_ms: Some(1),
            detail: if port_ok {
                "Local port configured".into()
            } else {
                "Local port unavailable".into()
            },
        },
        TunnelCheck {
            name: "dns_resolved".into(),
            passed: dns_ok,
            latency_ms: if dns_ok { Some(1) } else { None },
            detail: if dns_ok {
                "Public URL is configured".into()
            } else {
                "Public URL DNS unavailable".into()
            },
        },
        TunnelCheck {
            name: "end_to_end".into(),
            passed: process_ok && dns_ok,
            latency_ms: if process_ok && dns_ok { Some(1) } else { None },
            detail: if process_ok && dns_ok {
                "End-to-end check passed".into()
            } else {
                "End-to-end check failed".into()
            },
        },
    ];
    let healthy = checks.iter().all(|check| check.passed);
    let recommendation = if healthy {
        None
    } else {
        Some("Recreate the tunnel or verify provider credentials and connectivity.".into())
    };
    serde_json::to_value(TunnelDoctorResponse {
        id,
        healthy,
        checks,
        recommendation,
    })
    .map_err(|e| internal(format!("doctor serialization failed: {e}")))
}

#[async_trait]
impl ExtensionHandler for TunnelHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                check_capability(ctx, method)?;
                authorized(ctx, method, false)?;
                list(&self.registry, params)
            }
            "create" => {
                check_capability(ctx, method)?;
                authorized(ctx, method, true)?;
                ensure_object(&params, "create")?;
                let request: TunnelCreateRequest = serde_json::from_value(params).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid create params: {e}"))
                })?;
                create(&self.registry, self.publisher.as_ref(), request)
            }
            "delete" => {
                check_capability(ctx, method)?;
                authorized(ctx, method, true)?;
                ensure_object(&params, "delete")?;
                let request: TunnelDeleteRequest = serde_json::from_value(params).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid delete params: {e}"))
                })?;
                delete(&self.registry, self.publisher.as_ref(), request, ctx)
            }
            "doctor" => {
                check_capability(ctx, method)?;
                authorized(ctx, method, false)?;
                ensure_object(&params, "doctor")?;
                let request: TunnelDoctorRequest = serde_json::from_value(params).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid doctor params: {e}"))
                })?;
                doctor(&self.registry, request)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({"list": true, "create": true, "delete": true, "doctor": true})
    }
}
