use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub const SYNC_EXCLUDED_KEYS: &[&str] = &[
    "securityScopedBookmarks",
    "localFilePaths",
    "oauthState",
    "clientInstanceId",
];

pub type SettingsDocument = Map<String, Value>;
pub type FlatSettingChanges = HashMap<String, Value>;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsLoadRequest {
    #[serde(default)]
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLoadResponse {
    pub settings: Value,
    pub version: u64,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSaveRequest {
    pub changes: HashMap<String, Value>,
    #[serde(default)]
    pub expected_version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSaveResponse {
    pub applied: bool,
    pub version: u64,
    pub merged: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartOpencodeRequest {
    pub confirm_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestartOpencodeResponse {
    pub restarted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChangedParams {
    pub version: u64,
    pub changed_keys: Vec<String>,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SettingsStoreState {
    pub settings: SettingsDocument,
    pub version: u64,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SettingsScope {
    pub principal: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsStoreError {
    VersionConflict,
    Persistence(String),
    Serialization(String),
    VersionOverflow,
}

pub trait SettingsStore: Send + Sync {
    fn load(&self, scope: &SettingsScope) -> Result<SettingsStoreState, SettingsStoreError>;
    fn save(
        &self,
        scope: &SettingsScope,
        expected: Option<u64>,
        state: &SettingsStoreState,
    ) -> Result<(), SettingsStoreError>;
}

#[derive(Default)]
pub struct MemorySettingsStore {
    states: Mutex<HashMap<SettingsScope, SettingsStoreState>>,
}

impl MemorySettingsStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SettingsStore for MemorySettingsStore {
    fn load(&self, scope: &SettingsScope) -> Result<SettingsStoreState, SettingsStoreError> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| SettingsStoreError::Persistence("settings store lock poisoned".into()))?;
        Ok(states
            .entry(scope.clone())
            .or_insert_with(|| SettingsStoreState {
                settings: Map::new(),
                version: 0,
                synced_at: Utc::now(),
            })
            .clone())
    }

    fn save(
        &self,
        scope: &SettingsScope,
        expected: Option<u64>,
        state: &SettingsStoreState,
    ) -> Result<(), SettingsStoreError> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| SettingsStoreError::Persistence("settings store lock poisoned".into()))?;
        let current = states
            .entry(scope.clone())
            .or_insert_with(|| SettingsStoreState {
                settings: Map::new(),
                version: 0,
                synced_at: Utc::now(),
            });
        if expected.is_some_and(|version| version != current.version) {
            return Err(SettingsStoreError::VersionConflict);
        }
        *current = state.clone();
        Ok(())
    }
}

pub struct SqliteSettingsStore {
    connection: Mutex<Connection>,
}

impl SqliteSettingsStore {
    pub fn new(path: &str) -> Result<Self, String> {
        let store = Self {
            connection: Mutex::new(Connection::open(path).map_err(|e| e.to_string())?),
        };
        store.connection.lock().map_err(|_| "settings store lock poisoned")?.execute_batch(
            "CREATE TABLE IF NOT EXISTS loom_settings (scope TEXT PRIMARY KEY, document TEXT NOT NULL, version INTEGER NOT NULL, synced_at TEXT NOT NULL)"
        ).map_err(|e| e.to_string())?;
        Ok(store)
    }

    fn key(scope: &SettingsScope) -> String {
        serde_json::to_string(scope).unwrap_or_default()
    }
}

impl SettingsStore for SqliteSettingsStore {
    fn load(&self, scope: &SettingsScope) -> Result<SettingsStoreState, SettingsStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Persistence("settings store lock poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT document, version, synced_at FROM loom_settings WHERE scope = ?1")
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
        let mut rows = statement
            .query([Self::key(scope)])
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?
        {
            let document: String = row
                .get(0)
                .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
            let synced_at: String = row
                .get(2)
                .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
            return Ok(SettingsStoreState {
                settings: serde_json::from_str(&document)
                    .map_err(|e| SettingsStoreError::Serialization(e.to_string()))?,
                version: row
                    .get::<_, i64>(1)
                    .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?
                    as u64,
                synced_at: DateTime::parse_from_rfc3339(&synced_at)
                    .map_err(|e| SettingsStoreError::Serialization(e.to_string()))?
                    .with_timezone(&Utc),
            });
        }
        Ok(SettingsStoreState {
            settings: Map::new(),
            version: 0,
            synced_at: Utc::now(),
        })
    }

    fn save(
        &self,
        scope: &SettingsScope,
        expected: Option<u64>,
        state: &SettingsStoreState,
    ) -> Result<(), SettingsStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Persistence("settings store lock poisoned".into()))?;
        let transaction = connection
            .transaction()
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
        let key = Self::key(scope);
        let current = transaction
            .query_row(
                "SELECT version FROM loom_settings WHERE scope = ?1",
                [key.clone()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))?
            .unwrap_or(0) as u64;
        if expected.is_some_and(|version| version != current) {
            return Err(SettingsStoreError::VersionConflict);
        }
        let document = serde_json::to_string(&state.settings)
            .map_err(|e| SettingsStoreError::Serialization(e.to_string()))?;
        transaction.execute("INSERT INTO loom_settings(scope, document, version, synced_at) VALUES(?1, ?2, ?3, ?4) ON CONFLICT(scope) DO UPDATE SET document=excluded.document, version=excluded.version, synced_at=excluded.synced_at", params![key, document, state.version as i64, state.synced_at.to_rfc3339()]).map_err(|e| SettingsStoreError::Persistence(e.to_string()))?;
        transaction
            .commit()
            .map_err(|e| SettingsStoreError::Persistence(e.to_string()))
    }
}

pub trait SettingsAuthorizer: Send + Sync {
    fn capability_enabled(&self, _method: &str) -> bool {
        true
    }
    fn authorized(&self, _method: &str, ctx: &ExtensionContext) -> bool {
        !ctx.principal.trim().is_empty()
    }
}

pub trait SettingsNotifier: Send + Sync {
    fn notify_others(
        &self,
        excluded_connection: &str,
        params: &SettingsChangedParams,
    ) -> Result<(), String>;
}

pub trait RestartScheduler: Send + Sync {
    fn schedule(&self, principal: &str) -> Result<(), RestartScheduleError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartScheduleError {
    RateLimited,
    Scheduling(String),
}

struct DefaultAuthorizer;
impl SettingsAuthorizer for DefaultAuthorizer {
    fn authorized(&self, _method: &str, ctx: &ExtensionContext) -> bool {
        !ctx.principal.trim().is_empty()
    }
}

struct DefaultNotifier;
impl SettingsNotifier for DefaultNotifier {
    fn notify_others(
        &self,
        _excluded_connection: &str,
        _params: &SettingsChangedParams,
    ) -> Result<(), String> {
        Ok(())
    }
}

struct DefaultScheduler {
    last: Mutex<Option<std::time::Instant>>,
    window: std::time::Duration,
}
impl DefaultScheduler {
    fn new() -> Self {
        Self {
            last: Mutex::new(None),
            window: std::time::Duration::from_secs(60),
        }
    }
}
impl RestartScheduler for DefaultScheduler {
    fn schedule(&self, _principal: &str) -> Result<(), RestartScheduleError> {
        let mut last = self.last.lock().map_err(|_| {
            RestartScheduleError::Scheduling("restart limiter lock poisoned".into())
        })?;
        if last.is_some_and(|instant| instant.elapsed() < self.window) {
            return Err(RestartScheduleError::RateLimited);
        }
        *last = Some(std::time::Instant::now());
        Ok(())
    }
}

pub struct SettingsHandler {
    store: Arc<dyn SettingsStore>,
    authorizer: Arc<dyn SettingsAuthorizer>,
    notifier: Arc<dyn SettingsNotifier>,
    scheduler: Arc<dyn RestartScheduler>,
}

impl SettingsHandler {
    pub fn new() -> Self {
        Self::with_dependencies(
            Arc::new(MemorySettingsStore::new()),
            Arc::new(DefaultAuthorizer),
            Arc::new(DefaultNotifier),
            Arc::new(DefaultScheduler::new()),
        )
    }

    pub fn with_dependencies(
        store: Arc<dyn SettingsStore>,
        authorizer: Arc<dyn SettingsAuthorizer>,
        notifier: Arc<dyn SettingsNotifier>,
        scheduler: Arc<dyn RestartScheduler>,
    ) -> Self {
        Self {
            store,
            authorizer,
            notifier,
            scheduler,
        }
    }

    fn scope(ctx: &ExtensionContext) -> Result<SettingsScope, ExtensionError> {
        if ctx.principal.trim().is_empty() {
            return Err(Self::forbidden("authenticated principal required"));
        }
        Ok(SettingsScope {
            principal: ctx.principal.clone(),
            session_id: ctx.session_id.clone(),
        })
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }
    fn conflict() -> ExtensionError {
        ExtensionError {
            code: -32005,
            message: "version_conflict".into(),
            data: None,
        }
    }
    fn rate_limited() -> ExtensionError {
        ExtensionError {
            code: -32008,
            message: "rate_limited".into(),
            data: None,
        }
    }
    fn forbidden(message: impl Into<String>) -> ExtensionError {
        ExtensionError::forbidden(message)
    }
    fn capability(method: &str) -> ExtensionError {
        ExtensionError {
            code: -32001,
            message: "capability_not_supported".into(),
            data: Some(Value::String(format!("settings.{method}"))),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))
    }

    fn store_error(error: SettingsStoreError) -> ExtensionError {
        match error {
            SettingsStoreError::VersionConflict => Self::conflict(),
            SettingsStoreError::Persistence(message)
            | SettingsStoreError::Serialization(message) => Self::internal(message),
            SettingsStoreError::VersionOverflow => Self::internal("settings version overflow"),
        }
    }
}

fn sensitive(key: &str) -> bool {
    SYNC_EXCLUDED_KEYS.contains(&key)
}

fn sanitize(value: Value) -> Option<Value> {
    match value {
        Value::Object(object) => Some(Value::Object(
            object
                .into_iter()
                .filter_map(|(key, value)| {
                    if sensitive(&key) {
                        None
                    } else {
                        sanitize(value).map(|value| (key, value))
                    }
                })
                .collect(),
        )),
        Value::Array(array) => Some(Value::Array(
            array.into_iter().filter_map(sanitize).collect(),
        )),
        other => Some(other),
    }
}

fn path(path: &str) -> Result<Vec<String>, ExtensionError> {
    let segments: Vec<_> = path.split('.').map(str::to_string).collect();
    if path.trim().is_empty()
        || segments
            .iter()
            .any(|segment| segment.is_empty() || segment.trim() != *segment)
    {
        return Err(ExtensionError::invalid_params("invalid setting path"));
    }
    Ok(segments)
}

fn validate_keys(keys: &[String]) -> Result<Vec<String>, ExtensionError> {
    let mut normalized = Vec::with_capacity(keys.len());
    for key in keys {
        if key.trim().is_empty() || key.trim() != key || key.contains('.') {
            return Err(ExtensionError::invalid_params("invalid setting key"));
        }
        if !normalized.contains(key) {
            normalized.push(key.clone());
        }
    }
    Ok(normalized)
}

fn validate_changes(
    changes: &HashMap<String, Value>,
) -> Result<Vec<(String, Value, Vec<String>)>, ExtensionError> {
    let mut entries = changes
        .iter()
        .map(|(key, value)| Ok((key.clone(), value.clone(), path(key)?)))
        .collect::<Result<Vec<_>, ExtensionError>>()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (index, (_, _, left)) in entries.iter().enumerate() {
        if left.iter().any(|segment| sensitive(segment)) {
            continue;
        }
        for (_, _, right) in entries.iter().skip(index + 1) {
            if right.iter().any(|segment| sensitive(segment)) {
                continue;
            }
            if left.len() <= right.len() && left == &right[..left.len()]
                || right.len() <= left.len() && right == &left[..right.len()]
            {
                return Err(ExtensionError::invalid_params("conflicting setting paths"));
            }
        }
    }
    Ok(entries)
}

fn apply_change(
    document: &mut Map<String, Value>,
    segments: &[String],
    value: &Value,
) -> Result<(), ExtensionError> {
    if segments.len() == 1 {
        if value.is_null() {
            document.remove(&segments[0]);
        } else {
            document.insert(
                segments[0].clone(),
                sanitize(value.clone()).unwrap_or(Value::Null),
            );
        }
        return Ok(());
    }
    let entry = document
        .entry(segments[0].to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let object = entry
        .as_object_mut()
        .ok_or_else(|| ExtensionError::invalid_params("setting path crosses a scalar"))?;
    apply_change(object, &segments[1..], value)
}

fn select(document: &Map<String, Value>, keys: Option<Vec<String>>) -> Value {
    match keys {
        None => Value::Object(document.clone()),
        Some(keys) => Value::Object(
            keys.into_iter()
                .filter_map(|key| document.get(&key).cloned().map(|value| (key, value)))
                .collect(),
        ),
    }
}

#[async_trait]
impl ExtensionHandler for SettingsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let capability = match method {
            "load" => "load",
            "save" => "save",
            "restart_opencode" => "restart_opencode",
            _ => return Err(ExtensionError::method_not_found()),
        };
        if !self.authorizer.capability_enabled(capability) {
            return Err(Self::capability(capability));
        }
        if !self.authorizer.authorized(capability, ctx) {
            return Err(Self::forbidden("settings authorization required"));
        }
        let scope = Self::scope(ctx)?;
        match method {
            "load" => {
                let request: SettingsLoadRequest = Self::parse(params)?;
                let keys = request.keys.as_deref().map(validate_keys).transpose()?;
                let state = self.store.load(&scope).map_err(Self::store_error)?;
                let settings =
                    sanitize(select(&state.settings, keys)).unwrap_or(Value::Object(Map::new()));
                serde_json::to_value(SettingsLoadResponse {
                    settings,
                    version: state.version,
                    synced_at: state.synced_at,
                })
                .map_err(|error| Self::internal(error.to_string()))
            }
            "save" => {
                let request: SettingsSaveRequest = Self::parse(params)?;
                if request.changes.is_empty() {
                    return Err(ExtensionError::invalid_params("changes must not be empty"));
                }
                if !self.authorizer.authorized("save", ctx) {
                    return Err(Self::forbidden("settings write permission required"));
                }
                let entries = validate_changes(&request.changes)?;
                let mut state = self.store.load(&scope).map_err(Self::store_error)?;
                state.settings = sanitize(Value::Object(state.settings))
                    .and_then(|value| value.as_object().cloned())
                    .unwrap_or_default();
                if request
                    .expected_version
                    .is_some_and(|version| version != state.version)
                {
                    return Err(Self::conflict());
                }
                let mut merged = HashMap::new();
                let mut staged = state.settings.clone();
                for (key, value, segments) in entries {
                    if segments.iter().any(|segment| sensitive(segment)) {
                        continue;
                    }
                    let value = sanitize(value).unwrap_or(Value::Null);
                    let before = staged.clone();
                    apply_change(&mut staged, &segments, &value)?;
                    if staged != before {
                        merged.insert(key, value);
                    }
                }
                if merged.is_empty() {
                    return serde_json::to_value(SettingsSaveResponse {
                        applied: true,
                        version: state.version,
                        merged,
                    })
                    .map_err(|error| Self::internal(error.to_string()));
                }
                state.settings = staged;
                state.version = state
                    .version
                    .checked_add(1)
                    .ok_or_else(|| Self::store_error(SettingsStoreError::VersionOverflow))?;
                state.synced_at = Utc::now();
                self.store
                    .save(&scope, request.expected_version, &state)
                    .map_err(Self::store_error)?;
                let mut changed_keys: Vec<_> = merged
                    .keys()
                    .filter(|key| !key.split('.').any(sensitive))
                    .cloned()
                    .collect();
                changed_keys.sort();
                let notification = SettingsChangedParams {
                    version: state.version,
                    changed_keys,
                    synced_at: state.synced_at,
                };
                let _ = self
                    .notifier
                    .notify_others(&ctx.connection_id, &notification);
                serde_json::to_value(SettingsSaveResponse {
                    applied: true,
                    version: state.version,
                    merged,
                })
                .map_err(|error| Self::internal(error.to_string()))
            }
            "restart_opencode" => {
                let request: RestartOpencodeRequest = Self::parse(params)?;
                if request.confirm_token.trim().is_empty() {
                    return Err(ExtensionError::invalid_params(
                        "confirmToken must not be empty",
                    ));
                }
                if !self.authorizer.authorized("restart_opencode", ctx) {
                    return Err(Self::forbidden("restart permission required"));
                }
                self.scheduler
                    .schedule(&ctx.principal)
                    .map_err(|error| match error {
                        RestartScheduleError::RateLimited => Self::rate_limited(),
                        RestartScheduleError::Scheduling(message) => Self::internal(message),
                    })?;
                serde_json::to_value(RestartOpencodeResponse {
                    restarted: true,
                    message: "OpenCode server is restarting. Please reconnect.".into(),
                })
                .map_err(|error| Self::internal(error.to_string()))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({ "load": true, "save": true, "restart_opencode": true })
    }
}

impl Default for SettingsHandler {
    fn default() -> Self {
        Self::new()
    }
}
