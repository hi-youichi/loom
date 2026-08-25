use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::auth;
use super::pagination::{encode_cursor, PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_TEXT: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    pub enabled: bool,
    pub scope: CommandScope,
    pub agent_mode: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandListRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandCreateRequest {
    #[serde(default)]
    pub client_request_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub prompt_template: String,
    #[serde(default)]
    pub scope: Option<CommandScope>,
    #[serde(default)]
    pub agent_mode: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandUpdateRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<Option<String>>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub prompt_template: Option<Option<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub scope: Option<CommandScope>,
    #[serde(default)]
    pub agent_mode: Option<Option<String>>,
    #[serde(default)]
    pub icon: Option<Option<String>>,
    #[serde(default)]
    pub shortcut: Option<Option<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDeleteResponse {
    pub id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandChange {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandChangedNotification {
    pub change: CommandChange,
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandCursor {
    offset: usize,
    principal: String,
    project: String,
    generation: u64,
}

#[derive(Clone)]
struct StoredCommand {
    item: CommandItem,
    project: String,
}

#[derive(Default)]
struct CommandState {
    records: HashMap<String, HashMap<String, StoredCommand>>,
    idempotency: HashMap<(String, String, String), (String, CommandItem)>,
    generation: u64,
    notifications: Vec<Value>,
}

#[derive(Clone, Default)]
pub struct CommandHandler {
    state: Arc<Mutex<CommandState>>,
}

impl CommandHandler {
    pub fn new() -> Self {
        Self::default()
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

    fn project(ctx: &ExtensionContext) -> String {
        canonical_project(ctx.working_directory.as_deref())
    }

    fn capability(ctx: &ExtensionContext, method: &str) -> Result<(), ExtensionError> {
        if ctx.client_capabilities.supports_command(method) {
            Ok(())
        } else {
            Err(ExtensionError::capability_not_supported("command"))
        }
    }
}

#[async_trait]
impl ExtensionHandler for CommandHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                Self::capability(ctx, "list")?;
                self.list(params, ctx)
            }
            "create" => {
                Self::capability(ctx, "create")?;
                self.create(params, ctx)
            }
            "update" => {
                Self::capability(ctx, "update")?;
                self.update(params, ctx)
            }
            "delete" => {
                Self::capability(ctx, "delete")?;
                self.delete(params, ctx)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "create": true, "update": true, "delete": true})
    }
}

impl CommandHandler {
    fn list(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: CommandListRequest = object_params(params)?;
        if request.limit == Some(0) {
            return Err(ExtensionError::invalid_params(
                "limit must be greater than zero",
            ));
        }
        let pagination = PaginationParams {
            cursor: request.cursor,
            limit: request.limit,
        };
        let cursor = pagination.decode_cursor::<CommandCursor>()?;
        let principal = Self::principal(ctx)?;
        let project = Self::project(ctx);
        let state = self
            .state
            .lock()
            .map_err(|_| internal_error("command store lock poisoned"))?;
        let mut items: Vec<_> = state
            .records
            .get(&principal)
            .into_iter()
            .flat_map(|r| r.values())
            .filter(|r| r.item.scope == CommandScope::Global || r.project == project)
            .map(|r| r.item.clone())
            .collect();
        items.extend(builtin_commands());
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let (offset, generation) = match cursor {
            Some(cursor)
                if cursor.principal == principal
                    && cursor.project == project
                    && cursor.generation == state.generation =>
            {
                (cursor.offset, cursor.generation)
            }
            Some(_) => {
                return Err(ExtensionError::invalid_params(
                    "cursor is stale or outside the current context",
                ));
            }
            None => (0, state.generation),
        };
        if offset > items.len() {
            return Err(ExtensionError::invalid_params(
                "cursor is outside the current snapshot",
            ));
        }
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        let end = offset.saturating_add(limit).min(items.len());
        let next = (end < items.len()).then(|| {
            encode_cursor(serde_json::json!({
                "offset": end, "principal": principal, "project": project, "generation": generation
            }))
        });
        Ok(PaginatedResult::new(items[offset..end].to_vec(), next).to_json())
    }

    fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "command", "create")?;
        let request: CommandCreateRequest = object_params(params)?;
        validate_name(&request.name)?;
        validate_text("promptTemplate", &request.prompt_template)?;
        validate_optional("description", request.description.as_deref())?;
        validate_optional("agentMode", request.agent_mode.as_deref())?;
        validate_optional("icon", request.icon.as_deref())?;
        validate_optional("shortcut", request.shortcut.as_deref())?;
        let principal = Self::principal(ctx)?;
        let project = Self::project(ctx);
        let scope = request.scope.unwrap_or(CommandScope::Global);
        if scope == CommandScope::Project && project.is_empty() {
            return Err(ExtensionError::invalid_params(
                "project scope requires a working directory",
            ));
        }
        let key = request.client_request_id.clone().unwrap_or_default();
        let fingerprint = serde_json::to_string(&(
            &request.name,
            &request.description,
            &request.prompt_template,
            &scope,
            &request.agent_mode,
            &request.icon,
            &request.shortcut,
            &project,
        ))
        .map_err(|_| internal_error("failed to fingerprint command"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("command store lock poisoned"))?;
        if !key.trim().is_empty() {
            if let Some((old, item)) =
                state
                    .idempotency
                    .get(&(principal.clone(), project.clone(), key.clone()))
            {
                if old != &fingerprint {
                    return Err(ExtensionError::invalid_params(
                        "clientRequestId was reused with different parameters",
                    ));
                }
                return serde_json::to_value(item)
                    .map_err(|_| internal_error("failed to serialize command"));
            }
        }
        let records = state.records.entry(principal.clone()).or_default();
        if records.values().any(|r| {
            r.item.name == request.name.trim()
                && r.item.scope == scope
                && (scope == CommandScope::Global || r.project == project)
        }) {
            return Err(command_conflict(&request.name));
        }
        let now = Utc::now();
        let item = CommandItem {
            id: format!("cmd_{}", Uuid::new_v4().simple()),
            name: request.name.trim().into(),
            description: request.description.unwrap_or_default(),
            prompt_template: request.prompt_template,
            enabled: true,
            scope: scope.clone(),
            agent_mode: request.agent_mode,
            icon: request.icon,
            shortcut: request.shortcut,
            created_at: now,
            updated_at: now,
        };
        let id = item.id.clone();
        records.insert(
            id.clone(),
            StoredCommand {
                item: item.clone(),
                project: if scope == CommandScope::Project {
                    project.clone()
                } else {
                    String::new()
                },
            },
        );
        if !key.trim().is_empty() {
            state
                .idempotency
                .insert((principal, project, key), (fingerprint, item.clone()));
        }
        state.generation = state.generation.wrapping_add(1);
        emit(&mut state, CommandChange::Created, &id, true);
        serde_json::to_value(item).map_err(|_| internal_error("failed to serialize command"))
    }

    fn update(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "command", "update")?;
        let request: CommandUpdateRequest = object_params(params)?;
        validate_id(&request.id)?;
        if let Some(Some(name)) = &request.name {
            validate_name(name)?;
        }
        if let Some(Some(template)) = &request.prompt_template {
            validate_text("promptTemplate", template)?;
        }
        if let Some(Some(value)) = &request.description {
            validate_optional("description", Some(value))?;
        }
        if let Some(Some(value)) = &request.agent_mode {
            validate_optional("agentMode", Some(value))?;
        }
        if let Some(Some(value)) = &request.icon {
            validate_optional("icon", Some(value))?;
        }
        if let Some(Some(value)) = &request.shortcut {
            validate_optional("shortcut", Some(value))?;
        }
        let principal = Self::principal(ctx)?;
        let project = Self::project(ctx);
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("command store lock poisoned"))?;
        let records = state
            .records
            .get_mut(&principal)
            .ok_or_else(|| command_not_found(&request.id))?;
        let existing = records
            .get(&request.id)
            .ok_or_else(|| command_not_found(&request.id))?
            .clone();
        if existing.item.scope == CommandScope::Project && existing.project != project {
            return Err(command_not_found(&request.id));
        }
        let mut item = existing.item.clone();
        let old_name = item.name.clone();
        let old_enabled = item.enabled;
        if let Some(Some(value)) = request.name {
            item.name = value.trim().into();
        }
        if let Some(value) = request.description {
            item.description = value.unwrap_or_default();
        }
        if let Some(Some(value)) = request.prompt_template {
            item.prompt_template = value;
        }
        if let Some(value) = request.enabled {
            item.enabled = value;
        }
        if let Some(value) = request.scope {
            item.scope = value;
        }
        if let Some(value) = request.agent_mode {
            item.agent_mode = value;
        }
        if let Some(value) = request.icon {
            item.icon = value;
        }
        if let Some(value) = request.shortcut {
            item.shortcut = value;
        }
        if item.prompt_template.trim().is_empty() {
            return Err(ExtensionError::invalid_params(
                "promptTemplate must not be empty",
            ));
        }
        let resulting_project = if item.scope == CommandScope::Project {
            project.clone()
        } else {
            String::new()
        };
        if item.scope == CommandScope::Project && resulting_project.is_empty() {
            return Err(ExtensionError::invalid_params(
                "project scope requires a working directory",
            ));
        }
        if records.values().any(|r| {
            r.item.id != request.id
                && r.item.name == item.name
                && r.item.scope == item.scope
                && (item.scope == CommandScope::Global || r.project == resulting_project)
        }) {
            return Err(command_conflict(&item.name));
        }
        item.updated_at = Utc::now();
        records.insert(
            request.id.clone(),
            StoredCommand {
                item: item.clone(),
                project: resulting_project,
            },
        );
        state.generation = state.generation.wrapping_add(1);
        emit(
            &mut state,
            CommandChange::Updated,
            &request.id,
            old_name != item.name || old_enabled != item.enabled,
        );
        serde_json::to_value(item).map_err(|_| internal_error("failed to serialize command"))
    }

    fn delete(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "command", "delete")?;
        let request: CommandDeleteRequest = object_params(params)?;
        validate_id(&request.id)?;
        let principal = Self::principal(ctx)?;
        let project = Self::project(ctx);
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("command store lock poisoned"))?;
        let records = state
            .records
            .get_mut(&principal)
            .ok_or_else(|| command_not_found(&request.id))?;
        if records
            .get(&request.id)
            .is_some_and(|r| r.item.scope == CommandScope::Project && r.project != project)
        {
            return Err(command_not_found(&request.id));
        }
        if records.remove(&request.id).is_none() {
            return Err(command_not_found(&request.id));
        }
        state.generation = state.generation.wrapping_add(1);
        emit(&mut state, CommandChange::Deleted, &request.id, true);
        serde_json::to_value(CommandDeleteResponse {
            id: request.id,
            deleted: true,
        })
        .map_err(|_| internal_error("failed to serialize delete response"))
    }
}

fn canonical_project(path: Option<&Path>) -> String {
    let Some(path) = path else {
        return String::new();
    };
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(Path::new("\\")),
            Component::Normal(value) => result.push(value),
        }
    }
    result
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn object_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
    if !params.is_object() {
        return Err(ExtensionError::invalid_params("params must be an object"));
    }
    serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid params: {e}")))
}

fn validate_id(id: &str) -> Result<(), ExtensionError> {
    validate_text("id", id)
}

fn validate_name(name: &str) -> Result<(), ExtensionError> {
    validate_text("name", name)?;
    if !name.trim().starts_with('/') {
        return Err(ExtensionError::invalid_params("name must begin with '/'"));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        return Err(ExtensionError::invalid_params(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().count() > MAX_TEXT {
        return Err(ExtensionError::invalid_params(format!(
            "{field} is too long"
        )));
    }
    Ok(())
}

fn validate_optional(field: &str, value: Option<&str>) -> Result<(), ExtensionError> {
    if let Some(value) = value {
        if value.chars().count() > MAX_TEXT {
            return Err(ExtensionError::invalid_params(format!(
                "{field} is too long"
            )));
        }
    }
    Ok(())
}

fn emit(state: &mut CommandState, change: CommandChange, id: &str, availability: bool) {
    state.notifications.push(serde_json::json!({"jsonrpc":"2.0","method":"_anureo.dev/command/changed","params": CommandChangedNotification { change, id: id.into() }}));
    if availability {
        state.notifications.push(serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"available_commands_update"}}}));
    }
}

fn command_not_found(id: &str) -> ExtensionError {
    ExtensionError {
        code: -32004,
        message: "not_found".into(),
        data: Some(Value::String(format!("command '{id}' not found"))),
    }
}

fn command_conflict(name: &str) -> ExtensionError {
    ExtensionError {
        code: -32003,
        message: "conflict".into(),
        data: Some(Value::String(format!("command '{name}' already exists"))),
    }
}

fn internal_error(message: &str) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

/// Built-in slash commands, synthesized per request (not stored, not
/// user-mutable). Names mirror the anureo runtime's shipped set so the FE
/// command palette keeps its baseline after the anureo removal.
fn builtin_commands() -> Vec<CommandItem> {
    const EPOCH: DateTime<Utc> = DateTime::UNIX_EPOCH;
    fn cmd(id: &str, name: &str, description: &str, template: &str) -> CommandItem {
        CommandItem {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            prompt_template: template.into(),
            enabled: true,
            scope: CommandScope::Global,
            agent_mode: None,
            icon: None,
            shortcut: None,
            created_at: EPOCH,
            updated_at: EPOCH,
        }
    }
    vec![
        cmd(
            "builtin_new",
            "/new",
            "Clear context and start a new chat",
            "Start a new conversation with no prior context.",
        ),
        cmd(
            "builtin_share",
            "/share",
            "Create a shareable link for this conversation",
            "Generate a shareable link for this conversation.",
        ),
        cmd(
            "builtin_undo",
            "/undo",
            "Undo the last user message and its response",
            "Undo the previous turn: revert to the state before my last message.",
        ),
        cmd(
            "builtin_redo",
            "/redo",
            "Redo the last undone user message",
            "Redo the previously undone turn.",
        ),
        cmd(
            "builtin_summarize",
            "/summarize",
            "Summarize the conversation so far",
            "Summarize this conversation so far as a set of concise bullet points, then list any open questions or pending work.",
        ),
        cmd(
            "builtin_plan",
            "/plan",
            "Enter plan mode to design before executing",
            "Switch to plan mode: analyze the task, propose an implementation plan, and wait for approval before making changes.",
        ),
        cmd(
            "builtin_commit",
            "/commit",
            "Generate a commit for the current changes",
            "Review the current working tree diff, stage the relevant changes, and create a conventional-commit describing them.",
        ),
        cmd(
            "builtin_review",
            "/review",
            "Review recent code changes",
            "Review the recent changes in this repository. Focus on correctness, edge cases, and consistency with the existing code style.",
        ),
    ]
}
