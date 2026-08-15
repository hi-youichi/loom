use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::boundary;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub enabled: bool,
    pub installed: bool,
    pub state: PluginState,
    pub capabilities: Vec<PluginCapability>,
    pub mcp_servers: Vec<String>,
    pub commands: Vec<String>,
    pub hooks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginState {
    Active,
    Inactive,
    Error,
    Installing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginCapability {
    Mcp,
    Command,
    Hook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginListRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginInstallRequest {
    pub source: PluginSource,
    pub identifier: String,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_true")]
    pub auto_enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginSource {
    Registry,
    Url,
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginUninstallRequest {
    pub id: String,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default = "default_true")]
    pub cleanup: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginEnableRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginDisableRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupDetail {
    pub removed: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub mcp_servers: CleanupDetail,
    pub commands: CleanupDetail,
    pub hooks: CleanupDetail,
}

#[derive(Default)]
pub struct PluginStore {
    items: RwLock<HashMap<String, PluginItem>>,
    idempotency: Mutex<HashMap<String, (String, String, Value)>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

pub struct PluginHandler {
    store: Arc<PluginStore>,
}

fn default_true() -> bool {
    true
}

impl PluginHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(PluginStore::default()),
        }
    }

    pub fn with_store(store: Arc<PluginStore>) -> Self {
        Self { store }
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn not_found(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32004,
            message: "not_found".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn conflict(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32003,
            message: "conflict".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))
    }

    fn required(value: &str, field: &str) -> Result<String, ExtensionError> {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ExtensionError::invalid_params(format!(
                "{field} must not be empty"
            )));
        }
        Ok(value.to_owned())
    }

    fn optional(value: Option<&str>, field: &str) -> Result<Option<String>, ExtensionError> {
        value.map(|value| Self::required(value, field)).transpose()
    }

    fn lock_for(&self, key: &str) -> Result<Arc<Mutex<()>>, ExtensionError> {
        let mut locks = self
            .store
            .locks
            .lock()
            .map_err(|_| Self::internal("plugin lock unavailable"))?;
        Ok(locks
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn authorize(ctx: &ExtensionContext) -> Result<(), ExtensionError> {
        if ctx.principal.trim().is_empty() {
            Err(ExtensionError::forbidden(
                "plugin write authorization required",
            ))
        } else {
            Ok(())
        }
    }

    fn fingerprint<T: Serialize>(request: &T) -> Result<String, ExtensionError> {
        serde_json::to_string(request).map_err(|error| Self::internal(error.to_string()))
    }

    fn replay(
        &self,
        key: Option<&str>,
        principal: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>, ExtensionError> {
        let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) else {
            return Ok(None);
        };
        let records = self
            .store
            .idempotency
            .lock()
            .map_err(|_| Self::internal("idempotency store unavailable"))?;
        match records.get(key) {
            Some((owner, request, result)) if owner == principal && request == fingerprint => {
                Ok(Some(result.clone()))
            }
            Some(_) => Err(Self::conflict(
                "clientRequestId is already bound to another request",
            )),
            None => Ok(None),
        }
    }

    fn remember(
        &self,
        key: Option<&str>,
        principal: &str,
        fingerprint: &str,
        result: &Value,
    ) -> Result<(), ExtensionError> {
        let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) else {
            return Ok(());
        };
        self.store
            .idempotency
            .lock()
            .map_err(|_| Self::internal("idempotency store unavailable"))?
            .insert(
                key.to_owned(),
                (principal.to_owned(), fingerprint.to_owned(), result.clone()),
            );
        Ok(())
    }

    fn validate_request_id(value: Option<&str>) -> Result<(), ExtensionError> {
        if let Some(value) = value {
            Self::required(value, "clientRequestId")?;
        }
        Ok(())
    }

    fn validate_path(identifier: &str, ctx: &ExtensionContext) -> Result<(), ExtensionError> {
        let base = ctx
            .working_directory
            .as_deref()
            .ok_or_else(|| ExtensionError::directory_boundary_violation(identifier))?;
        let path = Path::new(identifier);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ExtensionError::directory_boundary_violation(identifier));
        }
        boundary::validate_path(identifier, Some(base))
            .map(|_| ())
            .map_err(|error| {
                if error.code == -32003 {
                    Self::not_found(format!("path does not exist: {identifier}"))
                } else {
                    error
                }
            })
    }

    fn page(items: Vec<PluginItem>, request: PluginListRequest) -> Result<Value, ExtensionError> {
        let pagination = PaginationParams {
            cursor: request.cursor,
            limit: request.limit,
        };
        if pagination.limit == Some(0) {
            return Err(ExtensionError::invalid_params(
                "limit must be greater than zero",
            ));
        }
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        let offset = pagination
            .decode_cursor::<OffsetCursor>()?
            .map(|cursor| cursor.offset)
            .unwrap_or(0);
        if offset > items.len() {
            return Err(ExtensionError::invalid_params("cursor is out of range"));
        }
        Ok(PaginatedResult::from_slice(items, offset, limit).to_json())
    }

    fn install_item(identifier: &str, request: &PluginInstallRequest) -> PluginItem {
        let now = Utc::now().to_rfc3339();
        PluginItem {
            id: identifier.to_owned(),
            name: identifier
                .rsplit('/')
                .next()
                .unwrap_or(identifier)
                .to_owned(),
            version: request.version.clone().unwrap_or_else(|| "latest".into()),
            description: String::new(),
            author: None,
            homepage: None,
            enabled: request.auto_enable,
            installed: true,
            state: if request.auto_enable {
                PluginState::Active
            } else {
                PluginState::Inactive
            },
            capabilities: vec![],
            mcp_servers: vec![],
            commands: vec![],
            hooks: vec![],
            error_message: None,
            installed_at: Some(now.clone()),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OffsetCursor {
    offset: usize,
}

#[async_trait]
impl ExtensionHandler for PluginHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                let request: PluginListRequest = Self::parse(params)?;
                let mut items = self
                    .store
                    .items
                    .read()
                    .map_err(|_| Self::internal("plugin catalog unavailable"))?
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                items.sort_by_key(|item| (item.name.to_ascii_lowercase(), item.id.clone()));
                Self::page(items, request)
            }
            "install" => {
                let request: PluginInstallRequest = Self::parse(params)?;
                let identifier = Self::required(&request.identifier, "identifier")?;
                Self::validate_request_id(request.client_request_id.as_deref())?;
                Self::optional(request.version.as_deref(), "version")?;
                Self::authorize(ctx)?;
                match &request.source {
                    PluginSource::Url => {
                        let Some((scheme, authority)) = identifier.split_once("://") else {
                            return Err(ExtensionError::invalid_params(
                                "identifier must be an https URL",
                            ));
                        };
                        let host = authority.split('/').next().unwrap_or_default();
                        if !scheme.eq_ignore_ascii_case("https")
                            || host.is_empty()
                            || host.contains('@')
                            || identifier.chars().any(char::is_whitespace)
                        {
                            return Err(ExtensionError::invalid_params(
                                "identifier must be an https URL without credentials",
                            ));
                        }
                    }
                    PluginSource::Path => Self::validate_path(&identifier, ctx)?,
                    PluginSource::Registry => {}
                }
                let fingerprint = Self::fingerprint(&request)?;
                let request_lock = request
                    .client_request_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|key| self.lock_for(key))
                    .transpose()?;
                let _request_guard = request_lock
                    .as_ref()
                    .map(|lock| {
                        lock.lock()
                            .map_err(|_| Self::internal("plugin transaction unavailable"))
                    })
                    .transpose()?;
                if let Some(result) = self.replay(
                    request.client_request_id.as_deref(),
                    &ctx.principal,
                    &fingerprint,
                )? {
                    return Ok(result);
                }
                let lock = self.lock_for(&identifier)?;
                let _guard = lock
                    .lock()
                    .map_err(|_| Self::internal("plugin transaction unavailable"))?;
                if self
                    .store
                    .items
                    .read()
                    .map_err(|_| Self::internal("plugin catalog unavailable"))?
                    .contains_key(&identifier)
                {
                    return Err(Self::conflict("plugin is already installed"));
                }
                let item = Self::install_item(&identifier, &request);
                let result = serde_json::to_value(&item)
                    .map_err(|error| Self::internal(error.to_string()))?;
                self.store
                    .items
                    .write()
                    .map_err(|_| Self::internal("plugin catalog unavailable"))?
                    .insert(identifier, item);
                self.remember(
                    request.client_request_id.as_deref(),
                    &ctx.principal,
                    &fingerprint,
                    &result,
                )?;
                Ok(result)
            }
            "uninstall" => {
                let request: PluginUninstallRequest = Self::parse(params)?;
                let id = Self::required(&request.id, "id")?;
                Self::validate_request_id(request.client_request_id.as_deref())?;
                Self::authorize(ctx)?;
                let fingerprint = Self::fingerprint(&request)?;
                let request_lock = request
                    .client_request_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|key| self.lock_for(key))
                    .transpose()?;
                let _request_guard = request_lock
                    .as_ref()
                    .map(|lock| {
                        lock.lock()
                            .map_err(|_| Self::internal("plugin transaction unavailable"))
                    })
                    .transpose()?;
                if let Some(result) = self.replay(
                    request.client_request_id.as_deref(),
                    &ctx.principal,
                    &fingerprint,
                )? {
                    return Ok(result);
                }
                let lock = self.lock_for(&id)?;
                let _guard = lock
                    .lock()
                    .map_err(|_| Self::internal("plugin transaction unavailable"))?;
                let item = self
                    .store
                    .items
                    .write()
                    .map_err(|_| Self::internal("plugin catalog unavailable"))?
                    .remove(&id)
                    .ok_or_else(|| Self::not_found("plugin not found"))?;
                let cleanup = request.cleanup.then(|| CleanupResult {
                    mcp_servers: CleanupDetail {
                        removed: item.mcp_servers,
                        failed: vec![],
                    },
                    commands: CleanupDetail {
                        removed: item.commands,
                        failed: vec![],
                    },
                    hooks: CleanupDetail {
                        removed: item.hooks,
                        failed: vec![],
                    },
                });
                let result = serde_json::json!({"id": id, "deleted": true, "cleanup": cleanup, "errors": []});
                self.remember(
                    request.client_request_id.as_deref(),
                    &ctx.principal,
                    &fingerprint,
                    &result,
                )?;
                Ok(result)
            }
            "enable" => self.toggle(params, ctx, true),
            "disable" => self.toggle(params, ctx, false),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "install": true, "uninstall": true, "enable": true, "disable": true})
    }
}

impl PluginHandler {
    fn toggle(
        &self,
        params: Value,
        ctx: &ExtensionContext,
        enabled: bool,
    ) -> Result<Value, ExtensionError> {
        let id = if enabled {
            Self::parse::<PluginEnableRequest>(params)?.id
        } else {
            Self::parse::<PluginDisableRequest>(params)?.id
        };
        let id = Self::required(&id, "id")?;
        Self::authorize(ctx)?;
        let lock = self.lock_for(&id)?;
        let _guard = lock
            .lock()
            .map_err(|_| Self::internal("plugin transaction unavailable"))?;
        let mut items = self
            .store
            .items
            .write()
            .map_err(|_| Self::internal("plugin catalog unavailable"))?;
        let item = items
            .get_mut(&id)
            .ok_or_else(|| Self::not_found("plugin not found"))?;
        item.enabled = enabled;
        item.state = if enabled {
            PluginState::Active
        } else {
            PluginState::Inactive
        };
        item.error_message = None;
        item.updated_at = Utc::now().to_rfc3339();
        if enabled {
            Ok(serde_json::json!({
                "id": id,
                "enabled": item.enabled,
                "state": item.state,
                "errorMessage": item.error_message
            }))
        } else {
            Ok(serde_json::json!({
                "id": id,
                "enabled": item.enabled,
                "state": item.state
            }))
        }
    }
}

impl Default for PluginHandler {
    fn default() -> Self {
        Self::new()
    }
}
