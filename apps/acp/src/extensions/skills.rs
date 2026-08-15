use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::boundary;
use super::pagination::{encode_cursor, PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

fn latest() -> String {
    "latest".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Registry,
    Git,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SkillInstallSource {
    #[default]
    Registry,
    Git,
    Local,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillChange {
    Install,
    Uninstall,
    Update,
    Configure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub source: SkillSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub installed_at: String,
    pub updated_at: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsListRequest {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsSearchRequest {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsInstallRequest {
    #[serde(default)]
    source: SkillInstallSource,
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default = "latest")]
    version: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsUninstallRequest {
    skill_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsConfigureRequest {
    skill_id: String,
    config: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillSearchResult {
    id: String,
    name: String,
    version: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_version: Option<String>,
    update_available: bool,
    download_count: u64,
    rating: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallCursor {
    offset: usize,
    #[serde(default)]
    generation: Option<u64>,
}

#[derive(Default)]
struct Catalog {
    installed: HashMap<String, SkillInfo>,
    registry: HashMap<String, SkillSearchResult>,
    generation: u64,
}

pub trait SkillsEventPublisher: Send + Sync {
    fn publish(&self, change: SkillChange, skill_id: &str);
}

struct NoopPublisher;
impl SkillsEventPublisher for NoopPublisher {
    fn publish(&self, _change: SkillChange, _skill_id: &str) {}
}

pub struct SkillsHandler {
    catalog: Arc<RwLock<Catalog>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    publisher: Arc<dyn SkillsEventPublisher>,
}

impl SkillsHandler {
    pub fn new() -> Self {
        Self::with_publisher(Arc::new(NoopPublisher))
    }

    pub fn with_publisher(publisher: Arc<dyn SkillsEventPublisher>) -> Self {
        Self {
            catalog: Arc::new(RwLock::new(Catalog::default())),
            locks: Mutex::new(HashMap::new()),
            publisher,
        }
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn forbidden(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "forbidden".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn spec_conflict(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32000,
            message: "conflict".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params).map_err(|e| ExtensionError::invalid_params(e.to_string()))
    }

    fn parse_optional<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        match params {
            Value::Null => serde_json::from_value(Value::Object(Map::new()))
                .map_err(|e| ExtensionError::invalid_params(e.to_string())),
            value => Self::parse(value),
        }
    }

    fn id(value: &str) -> Result<String, ExtensionError> {
        let id = value.trim();
        if id.is_empty() || id.chars().any(|c| c.is_control()) {
            return Err(ExtensionError::invalid_params("skillId must not be empty"));
        }
        Ok(id.to_string())
    }

    fn lock_for(&self, id: &str) -> Result<Arc<Mutex<()>>, ExtensionError> {
        let mut locks = self
            .locks
            .lock()
            .map_err(|_| Self::internal("skill lock unavailable"))?;
        Ok(locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn pagination<T: Clone + Serialize>(
        full: Vec<T>,
        cursor: Option<String>,
        limit: Option<usize>,
        default: usize,
        generation: u64,
    ) -> Result<Value, ExtensionError> {
        let params = PaginationParams { cursor, limit };
        let limit = params.limit_or_default(default, 100);
        if limit == 0 {
            return Err(ExtensionError::invalid_params(
                "limit must be greater than zero",
            ));
        }
        let offset = params
            .decode_cursor::<InstallCursor>()?
            .map(|c| {
                if c.generation.is_some_and(|value| value != generation) {
                    return Err(ExtensionError::invalid_params("stale cursor"));
                }
                Ok(c.offset)
            })
            .transpose()?
            .unwrap_or(0);
        if offset > full.len() {
            return Err(ExtensionError::invalid_params("cursor is out of range"));
        }
        if offset.checked_add(limit).is_none() {
            return Err(ExtensionError::invalid_params("cursor is out of range"));
        }
        let mut result = PaginatedResult::from_slice(full, offset, limit).to_json();
        if result["hasMore"].as_bool() == Some(true) {
            let count = result["items"].as_array().map_or(0, Vec::len);
            result["nextCursor"] = Value::String(encode_cursor(serde_json::json!({
                "offset": offset + count,
                "generation": generation,
            })));
        }
        Ok(result)
    }

    #[allow(dead_code)]
    fn redact(value: Value) -> Value {
        match value {
            Value::Object(object) => Value::Object(
                object
                    .into_iter()
                    .filter_map(|(key, value)| {
                        let lower = key.to_ascii_lowercase();
                        if [
                            "token",
                            "secret",
                            "password",
                            "privatekey",
                            "api_key",
                            "apikey",
                        ]
                        .iter()
                        .any(|part| lower.contains(part))
                        {
                            None
                        } else {
                            Some((key, Self::redact(value)))
                        }
                    })
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.into_iter().map(Self::redact).collect()),
            other => other,
        }
    }

    fn merge(target: &mut Map<String, Value>, patch: Map<String, Value>) {
        for (key, value) in patch {
            if value.is_null() {
                target.remove(&key);
                continue;
            }
            if let (Some(existing), Some(incoming)) = (target.get_mut(&key), value.as_object()) {
                if let Some(existing_object) = existing.as_object_mut() {
                    Self::merge(existing_object, incoming.clone());
                    continue;
                }
            }
            target.insert(key, value);
        }
    }

    fn schema_valid(config: &Value, schema: Option<&Value>) -> bool {
        let Some(schema) = schema else {
            return config.is_object();
        };
        if schema
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t == "object")
            && !config.is_object()
        {
            return false;
        }
        let Some(object) = config.as_object() else {
            return false;
        };
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            if required
                .iter()
                .filter_map(Value::as_str)
                .any(|key| !object.contains_key(key))
            {
                return false;
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, value) in object {
                if let Some(expected) = properties
                    .get(key)
                    .and_then(|v| v.get("type"))
                    .and_then(Value::as_str)
                {
                    let valid = match expected {
                        "string" => value.is_string(),
                        "boolean" => value.is_boolean(),
                        "number" => value.is_number(),
                        "integer" => value.as_i64().is_some(),
                        "array" => value.is_array(),
                        "object" => value.is_object(),
                        "null" => value.is_null(),
                        _ => true,
                    };
                    if !valid {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn local_path(url: &str, ctx: &ExtensionContext) -> Result<std::path::PathBuf, ExtensionError> {
        let base = ctx.working_directory.as_deref().ok_or_else(|| {
            ExtensionError::invalid_params("workingDirectory is required for local installs")
        })?;
        let path = Path::new(url);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ExtensionError::directory_boundary_violation(url));
        }
        boundary::validate_path(url, Some(base))
    }

    fn installed_search(info: &SkillInfo) -> SkillSearchResult {
        SkillSearchResult {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version.clone(),
            description: info.description.clone(),
            category: info.category.clone(),
            tags: vec![],
            installed: true,
            installed_version: Some(info.version.clone()),
            update_available: false,
            download_count: 0,
            rating: 0.0,
            registry_url: info.source_url.clone(),
        }
    }

    fn public_skill(info: &SkillInfo) -> Value {
        serde_json::to_value(info).unwrap_or(Value::Null)
    }
}

#[async_trait]
impl ExtensionHandler for SkillsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                let catalog = self
                    .catalog
                    .read()
                    .map_err(|_| Self::internal("skill storage unavailable"))?;
                let generation = catalog.generation;
                let mut items: Vec<_> = catalog.installed.values().cloned().collect();
                items.sort_by_key(|item| (item.name.to_ascii_lowercase(), item.id.clone()));
                let request: SkillsListRequest = Self::parse_optional(params)?;
                let page = Self::pagination(items, request.cursor, request.limit, 50, generation)?;
                Ok(page)
            }
            "search" => {
                let request: SkillsSearchRequest = Self::parse_optional(params)?;
                let catalog = self
                    .catalog
                    .read()
                    .map_err(|_| Self::internal("skill registry unavailable"))?;
                let mut results: Vec<_> = if catalog.registry.is_empty() {
                    catalog
                        .installed
                        .values()
                        .map(Self::installed_search)
                        .collect()
                } else {
                    catalog.registry.values().cloned().collect()
                };
                let query = request
                    .query
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase();
                let category = request
                    .category
                    .map(|v| v.trim().to_ascii_lowercase())
                    .filter(|v| !v.is_empty());
                results.retain(|item| {
                    (query.is_empty()
                        || item.name.to_ascii_lowercase().contains(&query)
                        || item.description.to_ascii_lowercase().contains(&query)
                        || item
                            .tags
                            .iter()
                            .any(|tag| tag.to_ascii_lowercase().contains(&query)))
                        && category.as_ref().is_none_or(|value| {
                            item.category
                                .as_deref()
                                .is_some_and(|c| c.eq_ignore_ascii_case(value))
                        })
                });
                for item in &mut results {
                    if let Some(installed) = catalog.installed.get(&item.id) {
                        item.installed = true;
                        item.installed_version = Some(installed.version.clone());
                        item.update_available = item.version != installed.version;
                    }
                }
                results.sort_by_key(|item| (item.name.to_ascii_lowercase(), item.id.clone()));
                Self::pagination(
                    results,
                    request.cursor,
                    request.limit,
                    20,
                    catalog.generation,
                )
            }
            "install" => {
                let request: SkillsInstallRequest = Self::parse(params)?;
                let version = Self::id(&request.version)?;
                let id = match &request.source {
                    SkillInstallSource::Registry => {
                        Self::id(request.skill_id.as_deref().unwrap_or(""))?
                    }
                    SkillInstallSource::Git => {
                        if request
                            .url
                            .as_deref()
                            .is_none_or(|url| url.trim().is_empty())
                        {
                            return Err(ExtensionError::invalid_params("url is required"));
                        }
                        Self::id(
                            request
                                .skill_id
                                .as_deref()
                                .or(request.url.as_deref())
                                .unwrap_or(""),
                        )?
                    }
                    SkillInstallSource::Local => {
                        if request
                            .url
                            .as_deref()
                            .is_none_or(|url| url.trim().is_empty())
                        {
                            return Err(ExtensionError::invalid_params("url is required"));
                        }
                        Self::id(
                            request
                                .skill_id
                                .as_deref()
                                .or(request.url.as_deref())
                                .unwrap_or(""),
                        )?
                    }
                };
                if ctx.principal.trim().is_empty() {
                    return Err(Self::forbidden(
                        "skills:write authorization required",
                    ));
                }
                if matches!(&request.source, SkillInstallSource::Local) {
                    let _ = Self::local_path(
                        request
                            .url
                            .as_deref()
                            .ok_or_else(|| ExtensionError::invalid_params("url is required"))?,
                        ctx,
                    )?;
                }
                let lock = self.lock_for(&id)?;
                let _guard = lock
                    .lock()
                    .map_err(|_| Self::internal("skill transaction unavailable"))?;
                let mut catalog = self
                    .catalog
                    .write()
                    .map_err(|_| Self::internal("skill storage unavailable"))?;
                let previous = catalog.installed.get(&id).cloned();
                if previous
                    .as_ref()
                    .is_some_and(|skill| skill.version == version)
                    && !request.force
                {
                    return Err(Self::spec_conflict("skill version already installed"));
                }
                let now = Utc::now().to_rfc3339();
                let info = SkillInfo {
                    id: id.clone(),
                    name: id.replace('-', " "),
                    version,
                    description: String::new(),
                    category: None,
                    source: match &request.source {
                        SkillInstallSource::Registry => SkillSource::Registry,
                        SkillInstallSource::Git => SkillSource::Git,
                        SkillInstallSource::Local => SkillSource::Local,
                    },
                    source_url: request.url.or_else(|| Some(format!("registry://{id}"))),
                    installed_at: previous
                        .as_ref()
                        .map(|v| v.installed_at.clone())
                        .unwrap_or_else(|| now.clone()),
                    updated_at: now,
                    enabled: previous.as_ref().map(|v| v.enabled).unwrap_or(true),
                    config_schema: previous.as_ref().and_then(|v| v.config_schema.clone()),
                    config: previous.as_ref().and_then(|v| v.config.clone()),
                };
                catalog.installed.insert(id.clone(), info.clone());
                catalog.generation = catalog.generation.wrapping_add(1);
                drop(catalog);
                self.publisher.publish(
                    if previous.is_some() {
                        SkillChange::Update
                    } else {
                        SkillChange::Install
                    },
                    &id,
                );
                Ok(serde_json::json!({
                    "installed": true,
                    "skill": Self::public_skill(&info),
                    "previousVersion": previous.map(|v| v.version)
                }))
            }
            "uninstall" => {
                let request: SkillsUninstallRequest = Self::parse(params)?;
                let id = Self::id(&request.skill_id)?;
                if ctx.principal.trim().is_empty() {
                    return Err(Self::forbidden(
                        "skills:write authorization required",
                    ));
                }
                let lock = self.lock_for(&id)?;
                let _guard = lock
                    .lock()
                    .map_err(|_| Self::internal("skill transaction unavailable"))?;
                let mut catalog = self
                    .catalog
                    .write()
                    .map_err(|_| Self::internal("skill storage unavailable"))?;
                let Some(skill) = catalog.installed.get(&id) else {
                    return Ok(serde_json::json!({"uninstalled": false, "skillId": id}));
                };
                if skill
                    .source_url
                    .as_deref()
                    .is_some_and(|url| url.starts_with("builtin:"))
                {
                    return Err(Self::forbidden(
                        "builtin skills cannot be uninstalled",
                    ));
                }
                catalog.installed.remove(&id);
                catalog.generation = catalog.generation.wrapping_add(1);
                drop(catalog);
                self.publisher.publish(SkillChange::Uninstall, &id);
                Ok(serde_json::json!({"uninstalled": true, "skillId": id}))
            }
            "configure" => {
                let request: SkillsConfigureRequest = Self::parse(params)?;
                let id = Self::id(&request.skill_id)?;
                let patch =
                    request.config.as_object().cloned().ok_or_else(|| {
                        ExtensionError::invalid_params("config must be an object")
                    })?;
                let lock = self.lock_for(&id)?;
                let _guard = lock
                    .lock()
                    .map_err(|_| Self::internal("skill transaction unavailable"))?;
                let mut catalog = self
                    .catalog
                    .write()
                    .map_err(|_| Self::internal("skill storage unavailable"))?;
                let skill = catalog
                    .installed
                    .get_mut(&id)
                    .ok_or_else(|| ExtensionError::invalid_params("unknown skill"))?;
                let mut merged = skill
                    .config
                    .clone()
                    .unwrap_or_else(|| Value::Object(Map::new()))
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                Self::merge(&mut merged, patch);
                let merged_value = Value::Object(merged);
                if !Self::schema_valid(&merged_value, skill.config_schema.as_ref()) {
                    return Err(ExtensionError::invalid_params(
                        "configuration does not match configSchema",
                    ));
                }
                let mut updated = skill.clone();
                updated.config = Some(merged_value.clone());
                updated.updated_at = Utc::now().to_rfc3339();
                catalog.installed.insert(id.clone(), updated);
                catalog.generation = catalog.generation.wrapping_add(1);
                drop(catalog);
                self.publisher.publish(SkillChange::Configure, &id);
                Ok(serde_json::json!({
                    "configured": true,
                    "skillId": id,
                    "config": merged_value
                }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "search": true, "install": true, "uninstall": true, "configure": true})
    }
}

impl Default for SkillsHandler {
    fn default() -> Self {
        Self::new()
    }
}
