use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::agent_registry::AgentRegistry;

use super::pagination::{encode_cursor, PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;
const MAX_STRING: usize = 64 * 1024;
const MAX_TOOLS: usize = 128;
const MAX_TAGS: usize = 128;
const MAX_TEMPERATURE: f64 = 2.0;
const MAX_TOKENS: u32 = 16_777_216;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub tools: Vec<String>,
    pub tool_choice: ToolChoice,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub context_window: Option<u32>,
    pub is_built_in: bool,
    pub is_active: bool,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentListRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCreateRequest {
    pub client_request_id: Option<String>,
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub tools: Option<Vec<String>>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentUpdateRequest {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub tools: Option<Vec<String>>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCursor {
    pub offset: usize,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentChange {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentChangedParams {
    pub change: AgentChange,
    pub id: String,
}

pub trait AgentEventSink: Send + Sync {
    fn emit(&self, params: AgentChangedParams);
}

#[derive(Default)]
pub struct AgentProfileStore {
    profiles: HashMap<String, AgentProfile>,
    builtin_overrides: HashMap<String, AgentProfile>,
    owners: HashMap<String, String>,
    idempotency: HashMap<(String, String), (String, AgentProfile)>,
    active: HashMap<String, String>,
    revision: u64,
}

pub struct AgentProfileHandler {
    store: Arc<RwLock<AgentProfileStore>>,
    registry: AgentRegistry,
    event_sink: Option<Arc<dyn AgentEventSink>>,
}

impl AgentProfileHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(AgentProfileStore::default())),
            registry: AgentRegistry::new(),
            event_sink: None,
        }
    }
    pub fn with_store(store: Arc<RwLock<AgentProfileStore>>) -> Self {
        Self {
            store,
            ..Self::new()
        }
    }
    pub fn with_event_sink(mut self, sink: Arc<dyn AgentEventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }
    pub fn store(&self) -> Arc<RwLock<AgentProfileStore>> {
        self.store.clone()
    }
    pub async fn set_active(&self, connection_id: impl Into<String>, profile: impl Into<String>) {
        self.store
            .write()
            .await
            .active
            .insert(connection_id.into(), profile.into());
    }

    fn invalid(message: impl Into<String>) -> ExtensionError {
        ExtensionError::invalid_params(message)
    }
    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }
    fn not_found(id: &str) -> ExtensionError {
        ExtensionError {
            code: -32004,
            message: "not_found".into(),
            data: Some(Value::String(id.into())),
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
            return Err(Self::invalid("params must be an object"));
        }
        serde_json::from_value(params).map_err(|e| Self::invalid(e.to_string()))
    }
    fn reject_nulls(params: &Value, fields: &[&str]) -> Result<(), ExtensionError> {
        if fields
            .iter()
            .any(|f| params.get(*f).is_some_and(Value::is_null))
        {
            return Err(Self::invalid("null is not allowed"));
        }
        Ok(())
    }
    fn text(value: &str, field: &str) -> Result<String, ExtensionError> {
        let value = value.trim();
        if value.is_empty() || value.len() > MAX_STRING {
            Err(Self::invalid(format!("{field} must be non-empty")))
        } else {
            Ok(value.into())
        }
    }
    fn optional(value: Option<&String>, field: &str) -> Result<Option<String>, ExtensionError> {
        value.map(|v| Self::text(v, field)).transpose()
    }
    fn validate_lists(tools: &[String], tags: &[String]) -> Result<(), ExtensionError> {
        if tools.len() > MAX_TOOLS
            || tools.iter().any(|v| {
                v.trim().is_empty()
                    || v.len() > 256
                    || !v
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || ".:_-".contains(c))
            })
            || tools
                .iter()
                .enumerate()
                .any(|(i, v)| tools[..i].contains(v))
        {
            return Err(Self::invalid("invalid tools"));
        }
        if tags.len() > MAX_TAGS
            || tags.iter().any(|v| {
                v.trim().is_empty()
                    || v.len() > 128
                    || !v
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._- ".contains(c))
            })
            || tags.iter().enumerate().any(|(i, v)| tags[..i].contains(v))
        {
            return Err(Self::invalid("invalid tags"));
        }
        Ok(())
    }
    fn validate_numbers(
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<(), ExtensionError> {
        if temperature.is_some_and(|v| !v.is_finite() || !(0.0..=MAX_TEMPERATURE).contains(&v))
            || max_tokens.is_some_and(|v| v == 0 || v > MAX_TOKENS)
        {
            Err(Self::invalid("invalid numeric value"))
        } else {
            Ok(())
        }
    }
    fn color(value: Option<&String>) -> Result<Option<String>, ExtensionError> {
        let value = Self::optional(value, "color")?;
        if value.as_ref().is_some_and(|v| {
            !(v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit()))
        }) {
            return Err(Self::invalid("invalid color"));
        }
        Ok(value)
    }
    fn authorized(ctx: &ExtensionContext) -> Result<(), ExtensionError> {
        if ctx.principal.trim().is_empty() {
            Err(ExtensionError::forbidden("server authorization rejected"))
        } else {
            Ok(())
        }
    }
    fn now() -> String {
        chrono::Utc::now().to_rfc3339()
    }
    fn emit(&self, change: AgentChange, id: String) {
        if let Some(sink) = &self.event_sink {
            sink.emit(AgentChangedParams { change, id });
        }
    }

    fn builtins(&self, state: &AgentProfileStore) -> Vec<AgentProfile> {
        self.registry
            .to_session_modes()
            .into_iter()
            .filter_map(|mode| {
                let name = mode.id.to_string();
                let base = agent::profile::resolve_profile(&name).ok()?;
                let system_prompt = base
                    .role
                    .as_ref()
                    .and_then(|r| r.content.clone())
                    .unwrap_or_default();
                let profile = AgentProfile {
                    id: format!("agent_{name}"),
                    name: name.clone(),
                    display_name: name.clone(),
                    description: String::new(),
                    system_prompt,
                    model: None,
                    provider: None,
                    tools: Vec::new(),
                    tool_choice: ToolChoice::Auto,
                    temperature: None,
                    max_tokens: None,
                    context_window: None,
                    is_built_in: true,
                    is_active: false,
                    icon: None,
                    color: None,
                    tags: Vec::new(),
                    created_at: "1970-01-01T00:00:00+00:00".into(),
                    updated_at: "1970-01-01T00:00:00+00:00".into(),
                };
                Some(
                    state
                        .builtin_overrides
                        .get(&profile.id)
                        .cloned()
                        .unwrap_or(profile),
                )
            })
            .collect()
    }
    fn snapshot(&self, state: &AgentProfileStore, connection_id: &str) -> Vec<AgentProfile> {
        let active = state.active.get(connection_id);
        let mut result = self.builtins(state);
        result.extend(state.profiles.values().cloned());
        result.sort_by(|a, b| a.id.cmp(&b.id));
        for profile in &mut result {
            profile.is_active = active.is_some_and(|v| v == &profile.id || v == &profile.name);
        }
        result
    }
    fn duplicate_name(&self, state: &AgentProfileStore, name: &str, id: Option<&str>) -> bool {
        self.snapshot(state, "")
            .into_iter()
            .any(|p| Some(p.id.as_str()) != id && p.name == name)
    }

    async fn list(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: AgentListRequest = Self::parse(params)?;
        let pagination = PaginationParams {
            cursor: request.cursor,
            limit: request.limit,
        };
        let cursor = pagination.decode_cursor::<AgentCursor>()?;
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        if limit == 0 {
            return Err(Self::invalid("limit must be greater than zero"));
        }
        let state = self.store.read().await;
        if cursor
            .as_ref()
            .is_some_and(|c| c.revision != state.revision)
        {
            return Err(Self::invalid("cursor snapshot is stale"));
        }
        let offset = cursor.as_ref().map_or(0, |c| c.offset);
        let full = self.snapshot(&state, &ctx.connection_id);
        if offset > full.len() {
            return Err(Self::invalid("cursor is outside the result set"));
        }
        let page = PaginatedResult::from_slice(full, offset, limit);
        let mut result = page.to_json();
        if result["hasMore"].as_bool() == Some(true) {
            let end = offset + page.items.len();
            result["nextCursor"] = Value::String(encode_cursor(
                serde_json::json!({"offset": end, "revision": state.revision}),
            ));
        }
        Ok(result)
    }

    async fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::reject_nulls(
            &params,
            &[
                "clientRequestId",
                "name",
                "displayName",
                "description",
                "systemPrompt",
                "model",
                "provider",
                "tools",
                "toolChoice",
                "temperature",
                "maxTokens",
                "icon",
                "color",
                "tags",
            ],
        )?;
        let request: AgentCreateRequest = Self::parse(params.clone())?;
        Self::authorized(ctx)?;
        let name = Self::text(&request.name, "name")?;
        let system_prompt = Self::text(&request.system_prompt, "systemPrompt")?;
        let tools = request.tools.clone().unwrap_or_default();
        let tags = request.tags.clone().unwrap_or_default();
        Self::validate_lists(&tools, &tags)?;
        Self::validate_numbers(request.temperature, request.max_tokens)?;
        let profile = AgentProfile {
            id: format!("agent_{}", uuid::Uuid::new_v4()),
            name,
            display_name: Self::optional(request.display_name.as_ref(), "displayName")?
                .unwrap_or_default(),
            description: Self::optional(request.description.as_ref(), "description")?
                .unwrap_or_default(),
            system_prompt,
            model: Self::optional(request.model.as_ref(), "model")?,
            provider: Self::optional(request.provider.as_ref(), "provider")?,
            tools,
            tool_choice: request.tool_choice.clone().unwrap_or(ToolChoice::Auto),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            context_window: None,
            is_built_in: false,
            is_active: false,
            icon: Self::optional(request.icon.as_ref(), "icon")?,
            color: Self::color(request.color.as_ref())?,
            tags,
            created_at: Self::now(),
            updated_at: Self::now(),
        };
        let fingerprint =
            serde_json::to_string(&request).map_err(|e| Self::internal(e.to_string()))?;
        let mut state = self.store.write().await;
        if let Some(key) = &request.client_request_id {
            let key = Self::text(key, "clientRequestId")?;
            if let Some((old, prior)) = state.idempotency.get(&(ctx.principal.clone(), key.clone()))
            {
                if old == &fingerprint {
                    return serde_json::to_value(prior).map_err(|e| Self::internal(e.to_string()));
                }
                return Err(Self::conflict("clientRequestId was already used"));
            }
        }
        if self.duplicate_name(&state, &profile.name, None) {
            return Err(Self::conflict("profile name already exists"));
        }
        state
            .owners
            .insert(profile.id.clone(), ctx.principal.clone());
        state.profiles.insert(profile.id.clone(), profile.clone());
        if let Some(key) = request.client_request_id {
            let key = Self::text(&key, "clientRequestId")?;
            state
                .idempotency
                .insert((ctx.principal.clone(), key), (fingerprint, profile.clone()));
        }
        state.revision = state.revision.wrapping_add(1);
        drop(state);
        self.emit(AgentChange::Created, profile.id.clone());
        serde_json::to_value(profile).map_err(|e| Self::internal(e.to_string()))
    }

    async fn update(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::reject_nulls(
            &params,
            &[
                "id",
                "name",
                "displayName",
                "description",
                "systemPrompt",
                "model",
                "provider",
                "tools",
                "toolChoice",
                "temperature",
                "maxTokens",
                "icon",
                "color",
                "tags",
            ],
        )?;
        let request: AgentUpdateRequest = Self::parse(params.clone())?;
        Self::authorized(ctx)?;
        let id = Self::text(&request.id, "id")?;
        let mut state = self.store.write().await;
        let mut profile = state
            .profiles
            .get(&id)
            .cloned()
            .or_else(|| self.builtins(&state).into_iter().find(|p| p.id == id))
            .ok_or_else(|| Self::not_found(&id))?;
        if !profile.is_built_in
            && state
                .owners
                .get(&id)
                .is_some_and(|owner| owner != &ctx.principal)
        {
            return Err(ExtensionError::forbidden(
                "profile is not owned by principal",
            ));
        }
        let has = |field: &str| params.get(field).is_some();
        if has("name") {
            if profile.is_built_in {
                return Err(ExtensionError::forbidden("built-in name cannot be changed"));
            }
            let name = Self::text(
                request
                    .name
                    .as_ref()
                    .ok_or_else(|| Self::invalid("name cannot be null"))?,
                "name",
            )?;
            if self.duplicate_name(&state, &name, Some(&id)) {
                return Err(Self::conflict("profile name already exists"));
            }
            profile.name = name;
        }
        if has("displayName") {
            profile.display_name =
                Self::text(request.display_name.as_ref().unwrap(), "displayName")?;
        }
        if has("description") {
            profile.description = Self::text(request.description.as_ref().unwrap(), "description")?;
        }
        if has("systemPrompt") {
            profile.system_prompt =
                Self::text(request.system_prompt.as_ref().unwrap(), "systemPrompt")?;
        }
        if has("model") {
            profile.model = Self::optional(request.model.as_ref(), "model")?;
        }
        if has("provider") {
            profile.provider = Self::optional(request.provider.as_ref(), "provider")?;
        }
        if has("tools") {
            profile.tools = request.tools.clone().unwrap_or_default();
        }
        if has("toolChoice") {
            profile.tool_choice = request
                .tool_choice
                .clone()
                .ok_or_else(|| Self::invalid("toolChoice cannot be null"))?;
        }
        if has("temperature") {
            profile.temperature = request.temperature;
        }
        if has("maxTokens") {
            profile.max_tokens = request.max_tokens;
        }
        if has("icon") {
            profile.icon = Self::optional(request.icon.as_ref(), "icon")?;
        }
        if has("color") {
            profile.color = Self::color(request.color.as_ref())?;
        }
        if has("tags") {
            profile.tags = request.tags.clone().unwrap_or_default();
        }
        Self::validate_lists(&profile.tools, &profile.tags)?;
        Self::validate_numbers(profile.temperature, profile.max_tokens)?;
        profile.updated_at = Self::now();
        if profile.is_built_in {
            state.builtin_overrides.insert(id.clone(), profile.clone());
        } else {
            state.profiles.insert(id.clone(), profile.clone());
        }
        state.revision = state.revision.wrapping_add(1);
        drop(state);
        self.emit(AgentChange::Updated, id);
        serde_json::to_value(profile).map_err(|e| Self::internal(e.to_string()))
    }

    async fn delete(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: AgentDeleteRequest = Self::parse(params)?;
        Self::authorized(ctx)?;
        let id = Self::text(&request.id, "id")?;
        let mut state = self.store.write().await;
        let profile = state
            .profiles
            .get(&id)
            .cloned()
            .or_else(|| self.builtins(&state).into_iter().find(|p| p.id == id))
            .ok_or_else(|| Self::not_found(&id))?;
        if profile.is_built_in
            || state
                .active
                .values()
                .any(|v| v == &profile.id || v == &profile.name)
        {
            return Err(ExtensionError::forbidden(
                "built-in or active profile cannot be deleted",
            ));
        }
        if state
            .owners
            .get(&id)
            .is_some_and(|owner| owner != &ctx.principal)
        {
            return Err(ExtensionError::forbidden(
                "profile is not owned by principal",
            ));
        }
        state.profiles.remove(&id);
        state.owners.remove(&id);
        state.revision = state.revision.wrapping_add(1);
        drop(state);
        self.emit(AgentChange::Deleted, id.clone());
        Ok(serde_json::json!({"id": id, "deleted": true}))
    }
}

impl Default for AgentProfileHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for AgentProfileHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => self.list(params, ctx).await,
            "create" => self.create(params, ctx).await,
            "update" => self.update(params, ctx).await,
            "delete" => self.delete(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }
    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "create": true, "update": true, "delete": true})
    }
}
