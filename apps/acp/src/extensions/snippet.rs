use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::auth;
use super::pagination::{encode_cursor, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_NAME: usize = 256;
const MAX_DESCRIPTION: usize = 4096;
const MAX_BODY: usize = 1_048_576;
const MAX_CATEGORY: usize = 256;
const MAX_VARIABLES: usize = 256;
const MAX_TAGS: usize = 256;
const MAX_TAG: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub variables: Vec<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnippetListRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SnippetCreateRequest {
    #[serde(default)]
    pub client_request_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub body: String,
    #[serde(default)]
    pub variables: Option<Vec<String>>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SnippetUpdateRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<Option<String>>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub body: Option<Option<String>>,
    #[serde(default)]
    pub variables: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub category: Option<Option<String>>,
    #[serde(default)]
    pub tags: Option<Option<Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnippetDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetDeleteResponse {
    pub id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetChangedParams {
    pub change: SnippetChange,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnippetChange {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListCursor {
    generation: u64,
    offset: usize,
}

#[derive(Default)]
struct SnippetState {
    records: HashMap<String, HashMap<String, SnippetItem>>,
    generations: HashMap<String, u64>,
    idempotency: HashMap<(String, String), (String, String)>,
    notifications: Vec<Value>,
}

#[derive(Clone)]
pub struct SnippetHandler {
    state: Arc<Mutex<SnippetState>>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl SnippetHandler {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SnippetState::default())),
            clock: Arc::new(Utc::now),
        }
    }

    pub fn with_clock<F>(clock: F) -> Self
    where
        F: Fn() -> DateTime<Utc> + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(Mutex::new(SnippetState::default())),
            clock: Arc::new(clock),
        }
    }

    pub fn notifications(&self) -> Vec<Value> {
        self.state
            .lock()
            .map(|state| state.notifications.clone())
            .unwrap_or_default()
    }

    fn principal(ctx: &ExtensionContext) -> Result<String, ExtensionError> {
        if ctx.principal.trim().is_empty() {
            return Err(ExtensionError::forbidden("no authenticated principal"));
        }
        Ok(ctx.principal.clone())
    }

    fn capability(ctx: &ExtensionContext, method: &str) -> Result<(), ExtensionError> {
        auth::check_capability(ctx, "snippet", method)
    }
}

impl Default for SnippetHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for SnippetHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => self.list(params, ctx),
            "create" => self.create(params, ctx),
            "update" => self.update(params, ctx),
            "delete" => self.delete(params, ctx),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "create": true, "update": true, "delete": true})
    }
}

impl SnippetHandler {
    fn list(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::capability(ctx, "list")?;
        let request: SnippetListRequest = if params.is_null() {
            SnippetListRequest {
                cursor: None,
                limit: None,
            }
        } else {
            object_params(params)?
        };
        let pagination = PaginationParams {
            cursor: request.cursor,
            limit: request.limit,
        };
        if pagination.limit == Some(0) {
            return Err(ExtensionError::invalid_params(
                "limit must be greater than zero",
            ));
        }
        let cursor = pagination.decode_cursor::<ListCursor>()?;
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        let principal = Self::principal(ctx)?;
        let state = self
            .state
            .lock()
            .map_err(|_| internal_error("snippet store unavailable"))?;
        let mut items: Vec<SnippetItem> = state
            .records
            .get(&principal)
            .into_iter()
            .flat_map(|records| records.values().cloned())
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let generation = state.generations.get(&principal).copied().unwrap_or(0);
        let offset = match cursor {
            Some(cursor) if cursor.generation == generation => cursor.offset,
            Some(_) => {
                return Err(ExtensionError::invalid_params(
                    "cursor is outside the current snapshot",
                ))
            }
            None => 0,
        };
        if offset > items.len() {
            return Err(ExtensionError::invalid_params(
                "cursor is outside the current snapshot",
            ));
        }
        let end = (offset + limit).min(items.len());
        let has_more = end < items.len();
        let next_cursor = has_more.then(|| {
            encode_cursor(serde_json::json!({
                "generation": generation,
                "offset": end
            }))
        });
        Ok(serde_json::json!({
            "items": &items[offset..end],
            "nextCursor": next_cursor,
            "hasMore": has_more,
        }))
    }

    fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::capability(ctx, "create")?;
        auth::check_server_policy(ctx, "snippet", "create")?;
        let request: SnippetCreateRequest = object_params(params)?;
        validate_text("name", &request.name, MAX_NAME)?;
        validate_text("body", &request.body, MAX_BODY)?;
        if let Some(description) = &request.description {
            validate_optional_text("description", description, MAX_DESCRIPTION)?;
        }
        let variables = match request.variables {
            Some(values) => {
                let values = validate_values("variables", values, MAX_VARIABLES, MAX_NAME)?;
                validate_variables_match(&request.body, &values)?;
                values
            }
            None => derive_variables(&request.body)?,
        };
        let tags = validate_values("tags", request.tags.unwrap_or_default(), MAX_TAGS, MAX_TAG)?;
        let category = request
            .category
            .map(|value| {
                let value = value.trim().to_string();
                if value.is_empty() {
                    Err(ExtensionError::invalid_params("category must not be empty"))
                } else {
                    validate_optional_text("category", &value, MAX_CATEGORY).map(|_| value)
                }
            })
            .transpose()?;
        let principal = Self::principal(ctx)?;
        let fingerprint = serde_json::to_string(&(
            &request.name,
            &request.description,
            &request.body,
            &variables,
            &category,
            &tags,
        ))
        .map_err(|_| ExtensionError::invalid_params("invalid create parameters"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("snippet store unavailable"))?;
        let client_request_id = request
            .client_request_id
            .clone()
            .filter(|v| !v.trim().is_empty());
        if let Some(key) = client_request_id.as_ref() {
            if let Some((old_fingerprint, id)) =
                state.idempotency.get(&(principal.clone(), key.clone()))
            {
                if old_fingerprint != &fingerprint {
                    return Err(ExtensionError::invalid_params(
                        "clientRequestId was reused with different parameters",
                    ));
                }
                let item = state
                    .records
                    .get(&principal)
                    .and_then(|r| r.get(id))
                    .ok_or_else(|| internal_error("idempotency record is inconsistent"))?;
                return serde_json::to_value(item)
                    .map_err(|_| internal_error("failed to serialize snippet"));
            }
        }
        let now = (self.clock)();
        let item = SnippetItem {
            id: format!("snip_{}", Uuid::new_v4().simple()),
            name: request.name.trim().to_string(),
            description: request.description.unwrap_or_default(),
            body: request.body,
            variables,
            category,
            tags,
            created_at: now,
            updated_at: now,
        };
        let id = item.id.clone();
        state
            .records
            .entry(principal.clone())
            .or_default()
            .insert(id.clone(), item.clone());
        *state.generations.entry(principal.clone()).or_default() += 1;
        if let Some(key) = client_request_id {
            state
                .idempotency
                .insert((principal, key), (fingerprint, id.clone()));
        }
        emit(&mut state, SnippetChange::Created, id);
        serde_json::to_value(item).map_err(|_| internal_error("failed to serialize snippet"))
    }

    fn update(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::capability(ctx, "update")?;
        auth::check_server_policy(ctx, "snippet", "update")?;
        let request: SnippetUpdateRequest = object_params(params)?;
        validate_id(&request.id)?;
        let principal = Self::principal(ctx)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("snippet store unavailable"))?;
        let result = {
            let current = state
                .records
                .get(&principal)
                .and_then(|r| r.get(&request.id))
                .ok_or_else(|| snippet_not_found(&request.id))?;
            let mut item = current.clone();
            if let Some(value) = request.name {
                item.name =
                    value.ok_or_else(|| ExtensionError::invalid_params("name cannot be null"))?;
                validate_text("name", &item.name, MAX_NAME)?;
            }
            if let Some(value) = request.description {
                item.description = value.unwrap_or_default();
                validate_optional_text("description", &item.description, MAX_DESCRIPTION)?;
            }
            if let Some(value) = request.body {
                item.body =
                    value.ok_or_else(|| ExtensionError::invalid_params("body cannot be null"))?;
                validate_text("body", &item.body, MAX_BODY)?;
                if request.variables.is_none() {
                    item.variables = derive_variables(&item.body)?;
                }
            }
            if let Some(value) = request.variables {
                item.variables = validate_values(
                    "variables",
                    value.unwrap_or_default(),
                    MAX_VARIABLES,
                    MAX_NAME,
                )?;
                validate_variables_match(&item.body, &item.variables)?;
            }
            if let Some(value) = request.category {
                item.category = value
                    .map(|value| {
                        let value = value.trim().to_string();
                        validate_text("category", &value, MAX_CATEGORY).map(|_| value)
                    })
                    .transpose()?;
            }
            if let Some(value) = request.tags {
                item.tags = validate_values("tags", value.unwrap_or_default(), MAX_TAGS, MAX_TAG)?;
            }
            item.updated_at = (self.clock)();
            state
                .records
                .get_mut(&principal)
                .and_then(|records| records.get_mut(&request.id))
                .ok_or_else(|| snippet_not_found(&request.id))?
                .clone_from(&item);
            item.clone()
        };
        *state.generations.entry(principal.clone()).or_default() += 1;
        emit(&mut state, SnippetChange::Updated, request.id);
        serde_json::to_value(result).map_err(|_| internal_error("failed to serialize snippet"))
    }

    fn delete(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        Self::capability(ctx, "delete")?;
        auth::check_server_policy(ctx, "snippet", "delete")?;
        let request: SnippetDeleteRequest = object_params(params)?;
        validate_id(&request.id)?;
        let principal = Self::principal(ctx)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("snippet store unavailable"))?;
        let records = state
            .records
            .get_mut(&principal)
            .ok_or_else(|| snippet_not_found(&request.id))?;
        if records.remove(&request.id).is_none() {
            return Err(snippet_not_found(&request.id));
        }
        *state.generations.entry(principal).or_default() += 1;
        emit(&mut state, SnippetChange::Deleted, request.id.clone());
        serde_json::to_value(SnippetDeleteResponse {
            id: request.id,
            deleted: true,
        })
        .map_err(|_| internal_error("failed to serialize delete response"))
    }
}

fn object_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
    if !params.is_object() {
        return Err(ExtensionError::invalid_params("params must be an object"));
    }
    serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid params: {e}")))
}

fn validate_id(id: &str) -> Result<(), ExtensionError> {
    validate_text("id", id, 256)
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        return Err(ExtensionError::invalid_params(format!(
            "{name} must not be empty"
        )));
    }
    if value.chars().count() > max {
        return Err(ExtensionError::invalid_params(format!(
            "{name} is too long"
        )));
    }
    Ok(())
}

fn validate_optional_text(name: &str, value: &str, max: usize) -> Result<(), ExtensionError> {
    if value.chars().count() > max {
        return Err(ExtensionError::invalid_params(format!(
            "{name} is too long"
        )));
    }
    Ok(())
}

fn validate_values(
    name: &str,
    values: Vec<String>,
    max_count: usize,
    max_item: usize,
) -> Result<Vec<String>, ExtensionError> {
    if values.len() > max_count {
        return Err(ExtensionError::invalid_params(format!("too many {name}")));
    }
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        validate_text(name, &value, max_item)?;
        let value = value.trim().to_string();
        if result.iter().any(|existing| existing == &value) {
            return Err(ExtensionError::invalid_params(format!(
                "{name} must not contain duplicates"
            )));
        }
        result.push(value);
    }
    Ok(result)
}

fn derive_variables(body: &str) -> Result<Vec<String>, ExtensionError> {
    let mut result = Vec::new();
    let mut index = 0;
    while let Some(start) = body[index..].find("{{") {
        let start = index + start;
        let after_start = start + 2;
        let end = body[after_start..]
            .find("}}")
            .map(|value| after_start + value)
            .ok_or_else(|| ExtensionError::invalid_params("malformed placeholder"))?;
        let name = body[after_start..end].trim();
        if !valid_variable_name(name) {
            return Err(ExtensionError::invalid_params("invalid placeholder"));
        }
        if !result.iter().any(|value| value == name) {
            result.push(name.to_string());
        }
        index = end + 2;
    }
    if body[index..].contains("}}") {
        return Err(ExtensionError::invalid_params("malformed placeholder"));
    }
    Ok(result)
}

fn valid_variable_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn validate_variables_match(body: &str, variables: &[String]) -> Result<(), ExtensionError> {
    let derived = derive_variables(body)?;
    if derived != variables {
        return Err(ExtensionError::invalid_params(
            "variables must match body placeholders",
        ));
    }
    Ok(())
}

fn emit(state: &mut SnippetState, change: SnippetChange, id: String) {
    state.notifications.push(serde_json::json!({"jsonrpc":"2.0","method":"_loomdesk.dev/snippet/changed","params": SnippetChangedParams { change, id }}));
}

fn snippet_not_found(id: &str) -> ExtensionError {
    ExtensionError {
        code: -32004,
        message: "not_found".into(),
        data: Some(Value::String(format!("snippet '{id}' not found"))),
    }
}

fn internal_error(message: &str) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}
