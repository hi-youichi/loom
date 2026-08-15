use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use config::home::loom_home;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::boundary::validate_path;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_ICON_BYTES: usize = 256 * 1024;
const MAX_PROJECT_ID_LEN: usize = 128;
const REDACTED: &str = "****";
pub const PROJECT_CHANGED_METHOD: &str = "_loomdesk.dev/project/changed";

pub type ProjectId = String;
pub type ProjectTimestamp = DateTime<Utc>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconImage {
    pub mime: String,
    pub updated_at: i64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub server_type: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub default_mode: Option<String>,
    pub default_model: Option<String>,
    pub default_provider: Option<String>,
    pub allow_file_access: bool,
    pub allowed_paths: Vec<String>,
    pub deny_patterns: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub mcp: BTreeMap<String, McpServerConfig>,
    pub git_identity: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            default_mode: None,
            default_model: None,
            default_provider: None,
            allow_file_access: true,
            allowed_paths: Vec::new(),
            deny_patterns: Vec::new(),
            env: BTreeMap::new(),
            mcp: BTreeMap::new(),
            git_identity: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub description: String,
    pub icon: String,
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_image: Option<IconImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_background: Option<String>,
    pub color: String,
    pub is_active: bool,
    pub agent_profile: Option<String>,
    pub mcp_servers: Vec<String>,
    pub session_count: u32,
    pub last_opened_at: Option<ProjectTimestamp>,
    pub created_at: ProjectTimestamp,
    pub updated_at: ProjectTimestamp,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub item: ProjectItem,
    pub config: ProjectConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectListRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectGetRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCreateRequest {
    pub path: String,
    #[serde(default)]
    pub preferred_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub icon_background: Option<String>,
    #[serde(default)]
    pub sidebar_collapsed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRemoveRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfigUpdate {
    #[serde(default)]
    pub default_mode: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub allow_file_access: Option<bool>,
    #[serde(default)]
    pub allowed_paths: Option<Vec<String>>,
    #[serde(default)]
    pub deny_patterns: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    pub mcp: Option<BTreeMap<String, Option<McpServerConfig>>>,
    #[serde(default)]
    pub git_identity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectUpdateRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon_background: Option<String>,
    #[serde(default)]
    pub sidebar_collapsed: Option<bool>,
    #[serde(default)]
    pub agent_profile: Option<String>,
    #[serde(default)]
    pub config: Option<ProjectConfigUpdate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectIconRequest {
    pub id: String,
    pub icon: String,
    #[serde(default)]
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIconResponse {
    pub id: String,
    pub icon: String,
    pub icon_url: Option<String>,
}

pub trait ProjectStore: Send + Sync {
    fn list(&self) -> Result<Vec<ProjectRecord>, String>;
    fn get(&self, id: &str) -> Result<Option<ProjectRecord>, String>;
    fn update(&self, id: &str, record: ProjectRecord) -> Result<(), String>;
    fn insert(&self, record: ProjectRecord) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<bool, String>;
}

#[derive(Default)]
pub struct MemoryProjectStore {
    records: Mutex<BTreeMap<String, ProjectRecord>>,
}

impl MemoryProjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, record: ProjectRecord) -> Result<(), String> {
        self.records
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|mut records| {
                records.insert(record.item.id.clone(), record);
            })
    }
}

impl ProjectStore for MemoryProjectStore {
    fn list(&self) -> Result<Vec<ProjectRecord>, String> {
        self.records
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|records| records.values().cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<ProjectRecord>, String> {
        self.records
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|records| records.get(id).cloned())
    }

    fn update(&self, id: &str, record: ProjectRecord) -> Result<(), String> {
        self.records
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|mut records| {
                if records.contains_key(id) {
                    records.insert(id.to_string(), record);
                }
            })
    }

    fn insert(&self, record: ProjectRecord) -> Result<(), String> {
        MemoryProjectStore::insert(self, record)
    }

    fn remove(&self, id: &str) -> Result<bool, String> {
        self.records
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|mut records| records.remove(id).is_some())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectStoreFile {
    version: u32,
    #[serde(default)]
    records: BTreeMap<String, ProjectRecord>,
    #[serde(default)]
    manual_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_hint: Option<String>,
}

impl Default for ProjectStoreFile {
    fn default() -> Self {
        Self {
            version: 1,
            records: BTreeMap::new(),
            manual_order: Vec::new(),
            active_hint: None,
        }
    }
}

/// File-backed project registry persisted to `loom_home()/projects.json`.
/// Mutations rewrite the whole file atomically (tmp + rename).
pub struct FileProjectStore {
    path: PathBuf,
    state: Mutex<ProjectStoreFile>,
}

impl FileProjectStore {
    pub fn open(path: PathBuf) -> Self {
        let state = Self::load(&path);
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    pub fn at_loom_home() -> Self {
        Self::open(loom_home().join("projects.json"))
    }

    fn load(path: &Path) -> ProjectStoreFile {
        match fs::read_to_string(path) {
            Ok(contents) if contents.trim().is_empty() => ProjectStoreFile::default(),
            Ok(contents) => serde_json::from_str::<ProjectStoreFile>(&contents).unwrap_or_else(
                |e| {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to parse projects store; starting empty"
                    );
                    ProjectStoreFile::default()
                },
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProjectStoreFile::default(),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to read projects store; starting empty"
                );
                ProjectStoreFile::default()
            }
        }
    }

    fn persist(&self, state: &ProjectStoreFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .map_err(|e| format!("failed to rename {}: {e}", self.path.display()))?;
        Ok(())
    }
}

impl ProjectStore for FileProjectStore {
    fn list(&self) -> Result<Vec<ProjectRecord>, String> {
        self.state
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|state| state.records.values().cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<ProjectRecord>, String> {
        self.state
            .lock()
            .map_err(|_| "project store lock poisoned".into())
            .map(|state| state.records.get(id).cloned())
    }

    fn update(&self, id: &str, record: ProjectRecord) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "project store lock poisoned".to_string())?;
        if state.records.contains_key(id) {
            state.records.insert(id.to_string(), record);
            self.persist(&state)?;
        }
        Ok(())
    }

    fn insert(&self, record: ProjectRecord) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "project store lock poisoned".to_string())?;
        state.records.insert(record.item.id.clone(), record);
        self.persist(&state)
    }

    fn remove(&self, id: &str) -> Result<bool, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "project store lock poisoned".to_string())?;
        if state.records.remove(id).is_none() {
            return Ok(false);
        }
        state.manual_order.retain(|entry| entry != id);
        self.persist(&state)?;
        Ok(true)
    }
}

pub trait ProjectAuthorizer: Send + Sync {
    fn authorized(&self, method: &str, ctx: &ExtensionContext) -> bool;
}

pub trait ProjectNotifier: Send + Sync {
    fn publish(&self, change: &str, id: &str, excluded_connection: &str) -> Result<(), String>;
}

pub trait ProjectCapabilityChecker: Send + Sync {
    fn declared(&self, method: &str, ctx: &ExtensionContext) -> bool;
}

struct DefaultAuthorizer;
impl ProjectAuthorizer for DefaultAuthorizer {
    fn authorized(&self, _method: &str, ctx: &ExtensionContext) -> bool {
        !ctx.principal.trim().is_empty()
            && ctx
                .session_id
                .as_deref()
                .is_some_and(|session| !session.trim().is_empty())
    }
}

struct DefaultNotifier;
impl ProjectNotifier for DefaultNotifier {
    fn publish(&self, _change: &str, _id: &str, _excluded_connection: &str) -> Result<(), String> {
        Ok(())
    }
}

struct DefaultCapabilityChecker;
impl ProjectCapabilityChecker for DefaultCapabilityChecker {
    fn declared(&self, _method: &str, _ctx: &ExtensionContext) -> bool {
        true
    }
}

pub struct ProjectHandler {
    store: Arc<dyn ProjectStore>,
    authorizer: Arc<dyn ProjectAuthorizer>,
    notifier: Arc<dyn ProjectNotifier>,
    capability_checker: Arc<dyn ProjectCapabilityChecker>,
    operation_lock: Mutex<()>,
    capabilities: BTreeMap<String, bool>,
}

impl ProjectHandler {
    pub fn new() -> Self {
        Self::with_dependencies(
            Arc::new(MemoryProjectStore::new()),
            Arc::new(DefaultAuthorizer),
            Arc::new(DefaultNotifier),
        )
    }

    pub fn with_dependencies(
        store: Arc<dyn ProjectStore>,
        authorizer: Arc<dyn ProjectAuthorizer>,
        notifier: Arc<dyn ProjectNotifier>,
    ) -> Self {
        Self {
            store,
            authorizer,
            notifier,
            capability_checker: Arc::new(DefaultCapabilityChecker),
            operation_lock: Mutex::new(()),
            capabilities: ["list", "get", "create", "remove", "update", "icon"]
                .into_iter()
                .map(|name| (name.into(), true))
                .collect(),
        }
    }

    /// Handler backed by `loom_home()/projects.json` with default authorization
    /// and a no-op notifier (hub wiring is tracked as backlog).
    pub fn persistent() -> Self {
        Self::with_dependencies(
            Arc::new(FileProjectStore::at_loom_home()),
            Arc::new(DefaultAuthorizer),
            Arc::new(DefaultNotifier),
        )
    }

    pub fn with_capabilities(mut self, capabilities: BTreeMap<String, bool>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_capability_checker(mut self, checker: Arc<dyn ProjectCapabilityChecker>) -> Self {
        self.capability_checker = checker;
        self
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn not_found(id: &str) -> ExtensionError {
        ExtensionError::not_found(format!("project '{id}' not found"))
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params).map_err(|e| ExtensionError::invalid_params(e.to_string()))
    }

    fn id(id: &str) -> Result<(), ExtensionError> {
        if id.trim().is_empty() {
            Err(ExtensionError::invalid_params("id must not be empty"))
        } else {
            Ok(())
        }
    }

    fn active(item: &mut ProjectItem, ctx: &ExtensionContext) {
        item.is_active = match (&ctx.session_id, &ctx.working_directory) {
            (Some(session), _) if !session.trim().is_empty() => item.is_active,
            _ => false,
        };
    }

    fn item(mut record: ProjectRecord, ctx: &ExtensionContext) -> ProjectItem {
        record.item.path = server_path(&record.item.path, ctx.working_directory.as_deref());
        Self::active(&mut record.item, ctx);
        record.item
    }

    fn snapshot(record: ProjectRecord, ctx: &ExtensionContext) -> Value {
        let item = Self::item(record.clone(), ctx);
        let mut config = record.config;
        config.env = config
            .env
            .into_iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    if secret_key(&key) {
                        REDACTED.into()
                    } else {
                        value
                    },
                )
            })
            .collect();
        let mut value = serde_json::to_value(item).unwrap_or(Value::Null);
        value["config"] = redact_value(serde_json::to_value(config).unwrap_or(Value::Null), false);
        value
    }

    fn validate_config_update(
        update: &ProjectConfigUpdate,
        ctx: &ExtensionContext,
    ) -> Result<(), ExtensionError> {
        if let Some(paths) = &update.allowed_paths {
            for path in paths {
                validate_path(path, ctx.working_directory.as_deref())
                    .map_err(|_| ExtensionError::directory_boundary_violation(path))?;
            }
        }
        if let Some(env) = &update.env {
            for (key, value) in env {
                if !valid_env_key(key) || value.as_deref().is_some_and(|v| v == REDACTED) {
                    return Err(ExtensionError::invalid_params("invalid environment entry"));
                }
            }
        }
        if let Some(mcp) = &update.mcp {
            for (name, config) in mcp {
                if name.trim().is_empty() {
                    return Err(ExtensionError::invalid_params(
                        "MCP server name must not be empty",
                    ));
                }
                if let Some(config) = config {
                    valid_mcp(config)?;
                }
            }
        }
        Ok(())
    }

    fn apply_update(
        mut record: ProjectRecord,
        request: &ProjectUpdateRequest,
    ) -> Result<ProjectRecord, ExtensionError> {
        if let Some(name) = &request.name {
            if name.trim().is_empty() {
                return Err(ExtensionError::invalid_params("name must not be empty"));
            }
            record.item.name = name.clone();
        }
        if let Some(description) = &request.description {
            record.item.description = description.clone();
        }
        if let Some(color) = &request.color {
            if !valid_color(color) {
                return Err(ExtensionError::invalid_params("invalid color"));
            }
            record.item.color = color.clone();
        }
        if let Some(background) = &request.icon_background {
            if background.is_empty() {
                record.item.icon_background = None;
            } else if valid_color(background) {
                record.item.icon_background = Some(background.clone());
            } else {
                return Err(ExtensionError::invalid_params("invalid color"));
            }
        }
        if let Some(collapsed) = request.sidebar_collapsed {
            record.item.sidebar_collapsed = collapsed;
        }
        if let Some(profile) = &request.agent_profile {
            if profile.trim().is_empty() {
                return Err(ExtensionError::invalid_params(
                    "agentProfile must not be empty",
                ));
            }
            record.item.agent_profile = Some(profile.clone());
        }
        if let Some(update) = &request.config {
            if let Some(value) = &update.default_mode {
                record.config.default_mode = Some(value.clone());
            }
            if let Some(value) = &update.default_model {
                record.config.default_model = Some(value.clone());
            }
            if let Some(value) = &update.default_provider {
                record.config.default_provider = Some(value.clone());
            }
            if let Some(value) = update.allow_file_access {
                record.config.allow_file_access = value;
            }
            if let Some(value) = &update.allowed_paths {
                record.config.allowed_paths = value.clone();
            }
            if let Some(value) = &update.deny_patterns {
                record.config.deny_patterns = value.clone();
            }
            if let Some(values) = &update.env {
                for (key, value) in values {
                    match value {
                        Some(value) => {
                            record.config.env.insert(key.clone(), value.clone());
                        }
                        None => {
                            record.config.env.remove(key);
                        }
                    }
                }
            }
            if let Some(values) = &update.mcp {
                for (key, value) in values {
                    match value {
                        Some(value) => {
                            record.config.mcp.insert(key.clone(), value.clone());
                        }
                        None => {
                            record.config.mcp.remove(key);
                        }
                    }
                }
            }
            if let Some(value) = &update.git_identity {
                record.config.git_identity = Some(value.clone());
            }
        }
        record.item.default_model = record.config.default_model.clone();
        record.item.mcp_servers = record.config.mcp.keys().cloned().collect();
        record.item.updated_at = Utc::now();
        Ok(record)
    }
}

impl Default for ProjectHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for ProjectHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let capability = match method {
            "list" | "get" | "create" | "remove" | "update" | "icon" => method,
            _ => return Err(ExtensionError::method_not_found()),
        };
        if !self.capabilities.get(capability).copied().unwrap_or(false) {
            return Err(ExtensionError::capability_not_supported("project"));
        }
        if !self.capability_checker.declared(method, ctx) {
            return Err(ExtensionError::capability_not_supported("project"));
        }
        match method {
            "list" => {
                let request: ProjectListRequest = if params.is_null() {
                    ProjectListRequest {
                        cursor: None,
                        limit: None,
                    }
                } else {
                    Self::parse(params)?
                };
                let pagination = PaginationParams {
                    cursor: request.cursor,
                    limit: request.limit.map(|value| value as usize),
                };
                let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
                if limit == 0 || limit > MAX_LIMIT {
                    return Err(ExtensionError::invalid_params("invalid limit"));
                }
                let offset = pagination
                    .decode_cursor::<ProjectCursor>()?
                    .map(|cursor| cursor.offset)
                    .unwrap_or(0);
                let records = self.store.list().map_err(Self::internal)?;
                if offset > records.len() {
                    return Err(ExtensionError::invalid_params("cursor is out of range"));
                }
                Ok(PaginatedResult::from_slice(
                    records
                        .into_iter()
                        .map(|record| Self::item(record, ctx))
                        .collect(),
                    offset,
                    limit,
                )
                .to_json())
            }
            "get" => {
                let request: ProjectGetRequest = Self::parse(params)?;
                Self::id(&request.id)?;
                let record = self
                    .store
                    .get(&request.id)
                    .map_err(Self::internal)?
                    .ok_or_else(|| Self::not_found(&request.id))?;
                Ok(Self::snapshot(record, ctx))
            }
            "create" => {
                let _guard = self
                    .operation_lock
                    .lock()
                    .map_err(|_| Self::internal("project operation lock poisoned"))?;
                let request: ProjectCreateRequest = Self::parse(params)?;
                if request.path.trim().is_empty() {
                    return Err(ExtensionError::invalid_params("path must not be empty"));
                }
                if !self.authorizer.authorized(method, ctx) {
                    return Err(ExtensionError::forbidden(
                        "project create authorization required",
                    ));
                }
                if let Some(name) = &request.name {
                    if name.trim().is_empty() {
                        return Err(ExtensionError::invalid_params("name must not be empty"));
                    }
                }
                if let Some(color) = &request.color {
                    if !valid_color(color) {
                        return Err(ExtensionError::invalid_params("invalid color"));
                    }
                }
                if let Some(background) = &request.icon_background {
                    if !background.is_empty() && !valid_color(background) {
                        return Err(ExtensionError::invalid_params("invalid color"));
                    }
                }
                let canonical = server_path(&request.path, ctx.working_directory.as_deref());
                let records = self.store.list().map_err(Self::internal)?;
                if let Some(existing) = records
                    .iter()
                    .find(|record| {
                        server_path(&record.item.path, ctx.working_directory.as_deref())
                            == canonical
                    })
                    .cloned()
                {
                    let mut value = Self::snapshot(existing, ctx);
                    value["existed"] = Value::Bool(true);
                    return Ok(value);
                }
                let id = match &request.preferred_id {
                    Some(preferred) => {
                        let trimmed = preferred.trim();
                        if !valid_project_id(trimmed) {
                            return Err(ExtensionError::invalid_params("invalid preferredId"));
                        }
                        if records.iter().any(|record| record.item.id == trimmed) {
                            return Err(ExtensionError::conflict(format!(
                                "project id '{trimmed}' is already taken"
                            )));
                        }
                        trimmed.to_string()
                    }
                    None => path_hash(&canonical),
                };
                let name = request
                    .name
                    .clone()
                    .unwrap_or_else(|| default_project_name(&canonical));
                let config = ProjectConfig {
                    default_model: request
                        .default_model
                        .clone()
                        .filter(|model| !model.trim().is_empty()),
                    ..ProjectConfig::default()
                };
                let now = Utc::now();
                let record = ProjectRecord {
                    item: ProjectItem {
                        id: id.clone(),
                        name,
                        path: canonical,
                        description: String::new(),
                        icon: "none".into(),
                        icon_url: None,
                        icon_image: None,
                        icon_background: request.icon_background.clone().filter(|v| !v.is_empty()),
                        color: request.color.clone().unwrap_or_else(|| "#4A90D9".into()),
                        is_active: false,
                        agent_profile: None,
                        mcp_servers: Vec::new(),
                        session_count: 0,
                        last_opened_at: None,
                        created_at: now,
                        updated_at: now,
                        sidebar_collapsed: request.sidebar_collapsed.unwrap_or(false),
                        default_model: config.default_model.clone(),
                    },
                    config,
                };
                self.store.insert(record.clone()).map_err(Self::internal)?;
                self.notifier
                    .publish("created", &id, &ctx.connection_id)
                    .map_err(Self::internal)?;
                let mut value = Self::snapshot(record, ctx);
                value["existed"] = Value::Bool(false);
                Ok(value)
            }
            "remove" => {
                let _guard = self
                    .operation_lock
                    .lock()
                    .map_err(|_| Self::internal("project operation lock poisoned"))?;
                let request: ProjectRemoveRequest = Self::parse(params)?;
                Self::id(&request.id)?;
                if !self.authorizer.authorized(method, ctx) {
                    return Err(ExtensionError::forbidden(
                        "project remove authorization required",
                    ));
                }
                let removed = self.store.remove(&request.id).map_err(Self::internal)?;
                if !removed {
                    return Err(Self::not_found(&request.id));
                }
                self.notifier
                    .publish("removed", &request.id, &ctx.connection_id)
                    .map_err(Self::internal)?;
                Ok(serde_json::json!({ "removed": true, "id": request.id }))
            }
            "update" => {
                let _guard = self
                    .operation_lock
                    .lock()
                    .map_err(|_| Self::internal("project operation lock poisoned"))?;
                let request: ProjectUpdateRequest = Self::parse(params)?;
                Self::id(&request.id)?;
                if !self.authorizer.authorized(method, ctx) {
                    return Err(ExtensionError::forbidden(
                        "project update authorization required",
                    ));
                }
                if let Some(config) = &request.config {
                    Self::validate_config_update(config, ctx)?;
                }
                if let Some(profile) = &request.agent_profile {
                    if ![
                        "default",
                        "architect",
                        "planner",
                        "coder",
                    ]
                    .contains(&profile.as_str())
                    {
                        return Err(ExtensionError::conflict(format!(
                            "unknown agent profile '{profile}'"
                        )));
                    }
                }
                let record = self
                    .store
                    .get(&request.id)
                    .map_err(Self::internal)?
                    .ok_or_else(|| Self::not_found(&request.id))?;
                let updated = Self::apply_update(record, &request)?;
                self.store
                    .update(&request.id, updated.clone())
                    .map_err(Self::internal)?;
                self.notifier
                    .publish("updated", &request.id, &ctx.connection_id)
                    .map_err(Self::internal)?;
                Ok(Self::snapshot(updated, ctx))
            }
            "icon" => {
                let _guard = self
                    .operation_lock
                    .lock()
                    .map_err(|_| Self::internal("project operation lock poisoned"))?;
                let request: ProjectIconRequest = Self::parse(params)?;
                Self::id(&request.id)?;
                if !self.authorizer.authorized(method, ctx) {
                    return Err(ExtensionError::forbidden(
                        "project icon authorization required",
                    ));
                }
                let mut record = self
                    .store
                    .get(&request.id)
                    .map_err(Self::internal)?
                    .ok_or_else(|| Self::not_found(&request.id))?;
                let icon_url = match request.icon.as_str() {
                    "none" => {
                        if request.icon_data.is_some() {
                            return Err(ExtensionError::invalid_params(
                                "iconData is not valid for none",
                            ));
                        }
                        record.item.icon = "none".into();
                        record.item.icon_url = None;
                        record.item.icon_image = None;
                        None
                    }
                    "custom" => {
                        let data = request.icon_data.as_deref().ok_or_else(|| {
                            ExtensionError::invalid_params("iconData is required")
                        })?;
                        validate_icon(data)?;
                        let mime = data_url_mime(data).ok_or_else(|| {
                            ExtensionError::invalid_params("invalid icon data URL")
                        })?;
                        record.item.icon = "custom".into();
                        record.item.icon_url = Some(data.into());
                        record.item.icon_image = Some(IconImage {
                            mime,
                            updated_at: Utc::now().timestamp_millis(),
                            source: "custom".into(),
                        });
                        Some(data.into())
                    }
                    name if valid_builtin(name) => {
                        if request.icon_data.is_some() {
                            return Err(ExtensionError::invalid_params(
                                "iconData is only valid for custom",
                            ));
                        }
                        record.item.icon = name.into();
                        record.item.icon_url = None;
                        record.item.icon_image = None;
                        None
                    }
                    _ => return Err(ExtensionError::invalid_params("unsupported icon")),
                };
                record.item.updated_at = Utc::now();
                self.store
                    .update(&request.id, record)
                    .map_err(Self::internal)?;
                self.notifier
                    .publish("icon_changed", &request.id, &ctx.connection_id)
                    .map_err(Self::internal)?;
                Ok(serde_json::to_value(ProjectIconResponse {
                    id: request.id.clone(),
                    icon: if icon_url.is_some() {
                        "custom".into()
                    } else {
                        self.store
                            .get(&request.id)
                            .map_err(Self::internal)?
                            .map(|r| r.item.icon)
                            .unwrap_or_else(|| "none".into())
                    },
                    icon_url,
                })
                .map_err(|e| Self::internal(e.to_string()))?)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "get": true,
            "create": true,
            "remove": true,
            "update": true,
            "icon": true
        })
    }
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["key", "token", "secret", "password", "credential", "auth"]
        .iter()
        .any(|part| key.contains(part))
}
fn valid_project_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PROJECT_ID_LEN
        && id.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}
fn path_hash(path: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("proj-{hash:010x}")
}
fn default_project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
fn data_url_mime(value: &str) -> Option<String> {
    value
        .strip_prefix("data:")?
        .split(';')
        .next()
        .map(str::to_string)
}
fn valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| b == b'_' || b.is_ascii_alphanumeric())
        && !key.as_bytes()[0].is_ascii_digit()
}
fn valid_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_mcp(config: &McpServerConfig) -> Result<(), ExtensionError> {
    if !["stdio", "sse", "http"].contains(&config.server_type.as_str()) {
        return Err(ExtensionError::invalid_params("invalid MCP type"));
    }
    if config.server_type == "stdio"
        && config
            .command
            .as_deref()
            .is_none_or(|v| v.trim().is_empty())
    {
        return Err(ExtensionError::invalid_params("stdio MCP requires command"));
    }
    if config.server_type != "stdio"
        && config
            .url
            .as_deref()
            .is_none_or(|v| v.trim().is_empty() || v.contains('@'))
    {
        return Err(ExtensionError::invalid_params("invalid MCP URL"));
    }
    if config
        .args
        .as_ref()
        .is_some_and(|args| args.iter().any(|arg| arg.len() > 16 * 1024))
    {
        return Err(ExtensionError::invalid_params("invalid MCP arguments"));
    }
    Ok(())
}
#[derive(Debug, Deserialize)]
struct ProjectCursor {
    offset: usize,
}

fn valid_builtin(icon: &str) -> bool {
    [
        "default",
        "folder",
        "git",
        "go",
        "javascript",
        "node",
        "python",
        "react",
        "rust",
        "server",
        "typescript",
        "web",
    ]
    .contains(&icon)
}
fn validate_icon(value: &str) -> Result<(), ExtensionError> {
    let (header, encoded) = value
        .split_once(",")
        .ok_or_else(|| ExtensionError::invalid_params("invalid icon data URL"))?;
    let mime = header
        .strip_prefix("data:")
        .and_then(|v| v.strip_suffix(";base64"))
        .ok_or_else(|| ExtensionError::invalid_params("invalid icon data URL"))?;
    if !["image/png", "image/jpeg", "image/svg+xml"].contains(&mime)
        || encoded.is_empty()
        || encoded.len() % 4 != 0
        || encoded
            .bytes()
            .any(|b| !(b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='))
    {
        return Err(ExtensionError::invalid_params("invalid icon data"));
    }
    let padding = encoded.chars().rev().take_while(|c| *c == '=').count();
    if padding > 2 || encoded[..encoded.len() - padding].contains('=') {
        return Err(ExtensionError::invalid_params("invalid icon data"));
    }
    let decoded = decode_base64(encoded)?;
    if decoded.len() > MAX_ICON_BYTES {
        return Err(ExtensionError::invalid_params("icon is too large"));
    }
    if mime == "image/svg+xml" {
        let svg = String::from_utf8(decoded)
            .map_err(|_| ExtensionError::invalid_params("invalid SVG data"))?
            .to_ascii_lowercase();
        if svg.contains("<script")
            || svg.contains("javascript:")
            || svg.contains("<!entity")
            || svg.contains("external")
            || svg.contains(" xlink:href=")
        {
            return Err(ExtensionError::invalid_params("unsafe SVG data"));
        }
    }
    Ok(())
}

fn decode_base64(value: &str) -> Result<Vec<u8>, ExtensionError> {
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let bytes = value.as_bytes();
    for chunk in bytes.chunks_exact(4) {
        let mut values = [0u8; 4];
        for (index, byte) in chunk.iter().enumerate() {
            values[index] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if index >= 2 => 0,
                _ => return Err(ExtensionError::invalid_params("invalid icon data")),
            };
        }
        output.push((values[0] << 2) | (values[1] >> 4));
        if chunk[2] != b'=' {
            output.push((values[1] << 4) | (values[2] >> 2));
        }
        if chunk[3] != b'=' {
            output.push((values[2] << 6) | values[3]);
        }
    }
    Ok(output)
}

fn server_path(path: &str, working_directory: Option<&Path>) -> String {
    let source = PathBuf::from(path);
    let absolute = if source.is_absolute() {
        source
    } else if let Some(base) = working_directory {
        base.join(source)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(source)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

fn redact_value(value: Value, sensitive_context: bool) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    let sensitive = sensitive_context || secret_key(&key) || key == "args";
                    let value = if sensitive && !value.is_object() && !value.is_array() {
                        Value::String(REDACTED.into())
                    } else {
                        redact_value(value, sensitive)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_value(value, sensitive_context))
                .collect(),
        ),
        Value::String(value) if sensitive_context || secret_value(&value) => {
            Value::String(REDACTED.into())
        }
        other => other,
    }
}

fn secret_value(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "token=",
        "secret=",
        "password=",
        "apikey=",
        "api_key=",
        "bearer ",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use tempfile::TempDir;

    const SVG_DATA_URL: &str = "data:image/svg+xml;base64,PHN2Zy8+";

    fn make_ctx(dir: &Path) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: "tester".into(),
            connection_id: "test-conn".into(),
            working_directory: Some(dir.to_path_buf()),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    fn file_handler(dir: &TempDir) -> ProjectHandler {
        ProjectHandler::with_dependencies(
            Arc::new(FileProjectStore::open(dir.path().join("projects.json"))),
            Arc::new(DefaultAuthorizer),
            Arc::new(DefaultNotifier),
        )
    }

    fn plain_record(id: &str) -> ProjectRecord {
        let now = Utc::now();
        ProjectRecord {
            item: ProjectItem {
                id: id.into(),
                name: id.into(),
                path: format!("/tmp/{id}"),
                description: String::new(),
                icon: "none".into(),
                icon_url: None,
                icon_image: None,
                icon_background: None,
                color: "#4A90D9".into(),
                is_active: false,
                agent_profile: None,
                mcp_servers: Vec::new(),
                session_count: 0,
                last_opened_at: None,
                created_at: now,
                updated_at: now,
                sidebar_collapsed: false,
                default_model: None,
            },
            config: ProjectConfig::default(),
        }
    }

    #[tokio::test]
    async fn create_is_idempotent_by_path() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        let first = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("alpha") }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(first["existed"], serde_json::json!(false));

        let second = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("alpha") }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(second["existed"], serde_json::json!(true));
        assert_eq!(second["id"], first["id"]);
    }

    #[tokio::test]
    async fn create_preferred_id_conflict() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("a"), "preferredId": "alpha" }),
                &ctx,
            )
            .await
            .unwrap();

        let error = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("b"), "preferredId": "alpha" }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, -32005);
    }

    #[tokio::test]
    async fn create_defaults_and_generated_id() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        let created = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("my-repo") }),
                &ctx,
            )
            .await
            .unwrap();

        let id = created["id"].as_str().unwrap().to_string();
        assert!(id.starts_with("proj-"), "generated id: {id}");
        assert_eq!(created["name"], "my-repo");
        assert_eq!(created["icon"], "none");
        assert_eq!(created["color"], "#4A90D9");
        assert_eq!(created["sidebarCollapsed"], false);
        assert_eq!(created["config"]["allowFileAccess"], true);
    }

    #[tokio::test]
    async fn create_requires_authorization() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(dir.path());
        ctx.session_id = None;
        let handler = file_handler(&dir);

        let error = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("a") }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, -32002);
    }

    #[tokio::test]
    async fn remove_then_missing() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        let created = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("a") }),
                &ctx,
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let removed = handler
            .handle("remove", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap();
        assert_eq!(removed["removed"], true);

        let error = handler
            .handle("remove", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap_err();
        assert_eq!(error.code, -32003);

        let listed = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(listed["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn file_store_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("projects.json");
        {
            let store = FileProjectStore::open(path.clone());
            store.insert(plain_record("p1")).unwrap();
        }
        let store = FileProjectStore::open(path.clone());
        assert!(store.get("p1").unwrap().is_some());
        assert!(!dir.path().join("projects.json.tmp").exists());
    }

    #[test]
    fn file_store_corrupt_file_starts_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("projects.json");
        fs::write(&path, "{ not json").unwrap();

        let store = FileProjectStore::open(path.clone());
        assert!(store.list().unwrap().is_empty());
        store.insert(plain_record("p1")).unwrap();
        assert!(FileProjectStore::open(path).get("p1").unwrap().is_some());
    }

    #[tokio::test]
    async fn icon_custom_sets_metadata() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        let created = handler
            .handle(
                "create",
                serde_json::json!({ "path": dir.path().join("a") }),
                &ctx,
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        handler
            .handle(
                "icon",
                serde_json::json!({ "id": id, "icon": "custom", "iconData": SVG_DATA_URL }),
                &ctx,
            )
            .await
            .unwrap();

        let fetched = handler
            .handle("get", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap();
        assert_eq!(fetched["icon"], "custom");
        assert_eq!(
            fetched["iconUrl"].as_str().unwrap().split(',').next(),
            Some("data:image/svg+xml;base64")
        );
        assert_eq!(fetched["iconImage"]["mime"], "image/svg+xml");
        assert_eq!(fetched["iconImage"]["source"], "custom");

        handler
            .handle("icon", serde_json::json!({ "id": id, "icon": "none" }), &ctx)
            .await
            .unwrap();
        let fetched = handler
            .handle("get", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap();
        assert!(fetched["iconImage"].is_null());
    }

    #[tokio::test]
    async fn update_supports_new_ui_fields() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(dir.path());
        let handler = file_handler(&dir);

        let created = handler
            .handle(
                "create",
                serde_json::json!({
                    "path": dir.path().join("a"),
                    "iconBackground": "#112233",
                    "sidebarCollapsed": true,
                    "defaultModel": "anthropic/claude"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["iconBackground"], "#112233");
        assert_eq!(created["sidebarCollapsed"], true);
        assert_eq!(created["defaultModel"], "anthropic/claude");

        handler
            .handle(
                "update",
                serde_json::json!({ "id": id, "iconBackground": "" }),
                &ctx,
            )
            .await
            .unwrap();
        let fetched = handler
            .handle("get", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap();
        assert!(fetched["iconBackground"].is_null());

        handler
            .handle(
                "update",
                serde_json::json!({ "id": id, "config": { "defaultModel": "openai/gpt" } }),
                &ctx,
            )
            .await
            .unwrap();
        let fetched = handler
            .handle("get", serde_json::json!({ "id": id }), &ctx)
            .await
            .unwrap();
        assert_eq!(fetched["defaultModel"], "openai/gpt");
        assert_eq!(fetched["config"]["defaultModel"], "openai/gpt");
    }
}
