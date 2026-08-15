use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::auth;
use super::pagination::{encode_cursor, PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 100;
const DEFAULT_PURGE_DAYS: u64 = 30;
const MAX_PURGE_DAYS: u64 = 3650;
const MAX_TTL_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;
const MAX_NAME_LEN: usize = 256;
const MAX_PLATFORM_LEN: usize = 64;
const MAX_SCOPE_LEN: usize = 128;
const MAX_SCOPES: usize = 64;

pub struct ClientAuthHandler {
    store: Arc<ClientAuthStore>,
    connections: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

impl ClientAuthHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(ClientAuthStore::default()),
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn with_store(store: Arc<ClientAuthStore>) -> Self {
        Self {
            store,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn store(&self) -> &Arc<ClientAuthStore> {
        &self.store
    }

    pub fn register_connection(
        &self,
        client_id: impl Into<String>,
        connection_id: impl Into<String>,
    ) {
        if let Ok(mut connections) = self.connections.lock() {
            connections
                .entry(client_id.into())
                .or_default()
                .insert(connection_id.into());
        }
    }
}

impl Default for ClientAuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct ClientAuthStore {
    entries: Mutex<HashMap<String, ClientAuthRecord>>,
    version: Mutex<u64>,
}

impl ClientAuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    pub fn insert(&self, record: ClientAuthRecord) {
        let _ = self.insert_checked(record);
    }

    fn insert_checked(&self, record: ClientAuthRecord) -> Result<(), ExtensionError> {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(record.client_id.clone(), record);
            self.bump_version_locked();
            Ok(())
        } else {
            Err(internal_error("client store unavailable"))
        }
    }

    pub fn get(&self, client_id: &str) -> Option<ClientAuthRecord> {
        self.entries.lock().ok()?.get(client_id).cloned()
    }

    pub fn remove(&self, client_id: &str) -> Option<ClientAuthRecord> {
        let mut entries = self.entries.lock().ok()?;
        let result = entries.remove(client_id);
        if result.is_some() {
            self.bump_version_locked();
        }
        result
    }

    fn bump_version_locked(&self) {
        if let Ok(mut version) = self.version.lock() {
            *version = version.wrapping_add(1);
        }
    }

    fn snapshot(
        &self,
        include_revoked: bool,
    ) -> Result<(u64, Vec<ClientAuthRecord>), ExtensionError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| internal_error("client store unavailable"))?;
        let version = *self
            .version
            .lock()
            .map_err(|_| internal_error("client store unavailable"))?;
        let records = entries
            .values()
            .filter(|record| include_revoked || !record.revoked)
            .cloned()
            .collect();
        Ok((version, records))
    }

    fn revoke(&self, client_id: &str) -> Result<RevokeOutcome, ExtensionError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| internal_error("client store unavailable"))?;
        let record = entries.get_mut(client_id).ok_or(RevokeOutcome::NotFound);
        match record {
            Err(outcome) => Ok(outcome),
            Ok(record) if record.revoked => record
                .revoked_at
                .clone()
                .map(RevokeOutcome::AlreadyRevoked)
                .ok_or_else(|| internal_error("revoked record has no revocation time")),
            Ok(record) => {
                let revoked_at = chrono::Utc::now().to_rfc3339();
                record.revoked = true;
                record.revoked_at = Some(revoked_at.clone());
                self.bump_version_locked();
                Ok(RevokeOutcome::RevokedNow(revoked_at))
            }
        }
    }

    fn purge_revoked(&self, older_than_days: u64) -> Result<u32, ExtensionError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days as i64);
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| internal_error("client store unavailable"))?;
        let mut ids = Vec::new();
        for (id, record) in entries.iter() {
            if !record.revoked {
                continue;
            }
            let timestamp = record
                .revoked_at
                .as_ref()
                .ok_or_else(|| internal_error("revoked record has no revocation time"))?
                .parse::<chrono::DateTime<chrono::FixedOffset>>()
                .map_err(|_| internal_error("revoked record has invalid revocation time"))?;
            if timestamp.with_timezone(&chrono::Utc) < cutoff {
                ids.push(id.clone());
            }
        }
        let count = u32::try_from(ids.len())
            .map_err(|_| internal_error("purge count exceeds response range"))?;
        for id in ids {
            entries.remove(&id);
        }
        if count != 0 {
            self.bump_version_locked();
        }
        Ok(count)
    }

    pub fn validate_token(&self, token: &str) -> Option<ClientAuthRecord> {
        let hash = sha256_hex(token);
        let now = chrono::Utc::now();
        let entries = self.entries.lock().ok()?;
        entries
            .values()
            .find(|record| {
                !record.revoked
                    && record.token_hash == hash
                    && record
                        .expires_at
                        .as_ref()
                        .map(|value| {
                            value
                                .parse::<chrono::DateTime<chrono::FixedOffset>>()
                                .map(|date| date.with_timezone(&chrono::Utc) > now)
                                .unwrap_or(false)
                        })
                        .unwrap_or(true)
            })
            .cloned()
    }
}

#[derive(Debug, Clone)]
pub struct ClientAuthRecord {
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
    pub expires_at: Option<String>,
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
}

impl From<&ClientAuthRecord> for ClientAuthEntry {
    fn from(record: &ClientAuthRecord) -> Self {
        Self {
            client_id: record.client_id.clone(),
            name: record.name.clone(),
            platform: record.platform.clone(),
            auth_method: record.auth_method.clone(),
            created_at: record.created_at.clone(),
            last_active: record.last_active.clone(),
            revoked: record.revoked,
            revoked_at: record.revoked_at.clone(),
            scope: record.scope.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClientAuthListParams {
    #[serde(default)]
    include_revoked: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClientAuthCreateParams {
    name: String,
    platform: String,
    scope: Vec<String>,
    #[serde(default)]
    ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClientAuthRevokeParams {
    client_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClientAuthPurgeParams {
    #[serde(default)]
    older_than_days: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ClientAuthCursor {
    version: u64,
    include_revoked: bool,
    limit: usize,
    created_at: String,
    client_id: String,
}

enum RevokeOutcome {
    NotFound,
    RevokedNow(String),
    AlreadyRevoked(String),
}

fn internal_error(message: &str) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}
fn ensure_object(params: &Value) -> Result<(), ExtensionError> {
    if params.is_object() {
        Ok(())
    } else {
        Err(ExtensionError::invalid_params(
            "params must be a JSON object",
        ))
    }
}
fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn valid_scope(value: &str) -> bool {
    matches!(
        value,
        "client-auth:manage"
            | "pairing:create"
            | "session:read"
            | "session:write"
            | "git:read"
            | "read:org"
    )
}
fn client_id() -> String {
    format!("ct-{}", uuid::Uuid::new_v4())
}
fn client_token() -> String {
    format!(
        "ct-token-{}-{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[async_trait]
impl ExtensionHandler for ClientAuthHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => self.list(params, ctx),
            "create" => self.create(params, ctx),
            "revoke" => self.revoke(params, ctx),
            "purge_revoked" => self.purge(params, ctx),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({"list": true, "create": true, "revoke": true, "purge_revoked": true})
    }
}

impl ClientAuthHandler {
    fn authorize(ctx: &ExtensionContext, method: &str) -> Result<(), ExtensionError> {
        auth::check_server_policy(ctx, "client-auth", method)
    }

    fn disconnect(&self, client_id: &str) -> Result<(), ExtensionError> {
        self.connections
            .lock()
            .map_err(|_| internal_error("connection manager unavailable"))?
            .remove(client_id);
        Ok(())
    }

    fn list(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::authorize(ctx, "list")?;
        ensure_object(&params)?;
        let input: ClientAuthListParams = serde_json::from_value(params.clone())
            .map_err(|_| ExtensionError::invalid_params("invalid list params"))?;
        if input.limit == Some(0) || input.limit.is_some_and(|limit| limit > MAX_LIST_LIMIT) {
            return Err(ExtensionError::invalid_params(
                "limit must be between 1 and 100",
            ));
        }
        let include_revoked = input.include_revoked.unwrap_or(false);
        let pagination: PaginationParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid pagination"))?;
        let limit = pagination.limit_or_default(DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
        let cursor: Option<ClientAuthCursor> = pagination.decode_cursor()?;
        let (version, mut records) = self.store.snapshot(include_revoked)?;
        if let Some(cursor) = &cursor {
            if cursor.version != version
                || cursor.include_revoked != include_revoked
                || cursor.limit != limit
            {
                return Err(ExtensionError::invalid_params(
                    "cursor does not match this query",
                ));
            }
        }
        records.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.client_id.cmp(&right.client_id))
        });
        let start = cursor
            .map(|cursor| {
                records
                    .iter()
                    .position(|record| {
                        record.created_at > cursor.created_at
                            || (record.created_at == cursor.created_at
                                && record.client_id > cursor.client_id)
                    })
                    .unwrap_or(records.len())
            })
            .unwrap_or(0);
        let end = (start + limit).min(records.len());
        let items: Vec<ClientAuthEntry> = records[start..end]
            .iter()
            .map(ClientAuthEntry::from)
            .collect();
        let next = (end < records.len()).then(|| encode_cursor(json!({"version": version, "include_revoked": include_revoked, "limit": limit, "created_at": records[end - 1].created_at, "client_id": records[end - 1].client_id})));
        Ok(PaginatedResult::new(items, next).to_json())
    }

    fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::authorize(ctx, "create")?;
        ensure_object(&params)?;
        let input: ClientAuthCreateParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid create params"))?;
        if !valid_text(&input.name, MAX_NAME_LEN)
            || !valid_text(&input.platform, MAX_PLATFORM_LEN)
            || input.scope.is_empty()
            || input.scope.len() > MAX_SCOPES
        {
            return Err(ExtensionError::invalid_params(
                "invalid name, platform, or scope",
            ));
        }
        let input_scope_len = input.scope.len();
        let mut scope = input.scope;
        for entry in &scope {
            if !valid_text(entry, MAX_SCOPE_LEN)
                || !valid_scope(entry)
                || !entry
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
            {
                return Err(ExtensionError::invalid_params("invalid scope"));
            }
        }
        scope.sort();
        scope.dedup();
        if scope.len() != input_scope_len {
            return Err(ExtensionError::invalid_params("duplicate scope"));
        }
        let ttl = input.ttl_seconds.unwrap_or(0);
        if ttl > MAX_TTL_SECONDS {
            return Err(ExtensionError::invalid_params("ttlSeconds is too large"));
        }
        let now = chrono::Utc::now();
        let token = client_token();
        let created_at = now.to_rfc3339();
        let record = ClientAuthRecord {
            client_id: client_id(),
            name: input.name.clone(),
            platform: input.platform.clone(),
            auth_method: "admin".into(),
            created_at: created_at.clone(),
            last_active: None,
            revoked: false,
            revoked_at: None,
            scope: scope.clone(),
            token_hash: sha256_hex(&token),
            expires_at: (ttl != 0)
                .then(|| (now + chrono::Duration::seconds(ttl as i64)).to_rfc3339()),
        };
        let client_id = record.client_id.clone();
        self.store.insert_checked(record)?;
        serde_json::to_value(json!({"clientId": client_id, "name": input.name, "platform": input.platform, "clientToken": token, "createdAt": created_at, "scope": scope})).map_err(|_| internal_error("create serialization failed"))
    }

    fn revoke(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::authorize(ctx, "revoke")?;
        ensure_object(&params)?;
        let input: ClientAuthRevokeParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid revoke params"))?;
        if input.client_id.trim().is_empty() {
            return Err(ExtensionError::invalid_params("clientId must not be empty"));
        }
        if ctx.principal == input.client_id || ctx.connection_id == input.client_id {
            return Err(ExtensionError::forbidden(
                "cannot revoke the calling client",
            ));
        }
        let outcome = match self.store.revoke(&input.client_id)? {
            RevokeOutcome::NotFound => {
                return Err(ExtensionError::not_found("client does not exist"))
            }
            outcome => outcome,
        };
        let revoked_at = match outcome {
            RevokeOutcome::RevokedNow(value) => {
                self.disconnect(&input.client_id)?;
                value
            }
            RevokeOutcome::AlreadyRevoked(value) => value,
            RevokeOutcome::NotFound => unreachable!(),
        };
        serde_json::to_value(
            json!({"clientId": input.client_id, "revoked": true, "revokedAt": revoked_at}),
        )
        .map_err(|_| internal_error("revoke serialization failed"))
    }

    fn purge(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::authorize(ctx, "purge_revoked")?;
        ensure_object(&params)?;
        let input: ClientAuthPurgeParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid purge params"))?;
        let days = input.older_than_days.unwrap_or(DEFAULT_PURGE_DAYS);
        if days > MAX_PURGE_DAYS {
            return Err(ExtensionError::invalid_params("olderThanDays is too large"));
        }
        let purged = self.store.purge_revoked(days)?;
        serde_json::to_value(json!({"purged": purged, "purgedAt": chrono::Utc::now().to_rfc3339()}))
            .map_err(|_| internal_error("purge serialization failed"))
    }
}
