use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

use config::home::loom_home;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_RUNS_PER_TASK: usize = 50;

pub struct ScheduledTaskHandler;

impl ScheduledTaskHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ScheduledTaskHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunStatus {
    Success,
    Failed,
    Running,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub schedule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<TaskRunStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub run_id: String,
    pub task_id: String,
    pub status: TaskRunStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduledTaskStore {
    #[serde(default)]
    pub tasks: Vec<ScheduledTask>,
    #[serde(default)]
    pub runs: Vec<TaskRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskChangeType {
    Started,
    Completed,
    Failed,
    Cancelled,
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskChangedNotification {
    pub id: String,
    #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub change: TaskChangeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskRunStatus>,
}

fn store_path(ctx: &ExtensionContext) -> PathBuf {
    if let Some(wd) = &ctx.working_directory {
        wd.join(".loom").join("scheduled-tasks.json")
    } else {
        loom_home().join("scheduled-tasks.json")
    }
}

fn load_store(ctx: &ExtensionContext) -> Result<ScheduledTaskStore, ExtensionError> {
    let path = store_path(ctx);
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            if contents.trim().is_empty() {
                return Ok(ScheduledTaskStore::default());
            }
            serde_json::from_str::<ScheduledTaskStore>(&contents).map_err(|e| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(format!(
                    "failed to parse scheduled-tasks store at {}: {e}",
                    path.display()
                ))),
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ScheduledTaskStore::default()),
        Err(e) => Err(ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(format!(
                "failed to read scheduled-tasks store at {}: {e}",
                path.display()
            ))),
        }),
    }
}

fn save_store(
    ctx: &ExtensionContext,
    store: &mut ScheduledTaskStore,
) -> Result<(), ExtensionError> {
    prune_runs(store);

    let path = store_path(ctx);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(format!(
                "failed to create directory {}: {e}",
                parent.display()
            ))),
        })?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to serialize scheduled-tasks store: {e}"
        ))),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to write scheduled-tasks store at {}: {e}",
            tmp.display()
        ))),
    })?;
    std::fs::rename(&tmp, &path).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to rename scheduled-tasks store: {e}"
        ))),
    })
}

fn prune_runs(store: &mut ScheduledTaskStore) {
    let mut task_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for task in &store.tasks {
        task_ids.insert(task.id.clone());
    }

    let mut runs_by_task: std::collections::HashMap<String, Vec<TaskRun>> =
        std::collections::HashMap::new();
    for run in store.runs.drain(..) {
        runs_by_task
            .entry(run.task_id.clone())
            .or_default()
            .push(run);
    }

    for task_id in &task_ids {
        if let Some(runs) = runs_by_task.get_mut(task_id) {
            runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
            runs.truncate(MAX_RUNS_PER_TASK);
        }
    }

    store.runs = runs_by_task.into_values().flatten().collect();
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn generate_run_id() -> String {
    format!("run-{}", uuid::Uuid::new_v4())
}

fn require_param_str(params: &Value, key: &str) -> Result<String, ExtensionError> {
    match params.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(ExtensionError::invalid_params(format!(
            "{key} must not be empty"
        ))),
        Some(_) => Err(ExtensionError::invalid_params(format!(
            "{key} must be a string"
        ))),
        None => Err(ExtensionError::invalid_params(format!(
            "missing required parameter: {key}"
        ))),
    }
}

fn optional_param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn build_notification(
    task_id: &str,
    run_id: Option<&str>,
    change: TaskChangeType,
    status: TaskRunStatus,
) -> Value {
    let notif = TaskChangedNotification {
        id: task_id.to_string(),
        run_id: run_id.map(|s| s.to_string()),
        change,
        status: Some(status),
    };
    serde_json::to_value(&notif).unwrap_or(Value::Null)
}

fn build_crud_notification(task_id: &str, change: TaskChangeType) -> Value {
    let notif = TaskChangedNotification {
        id: task_id.to_string(),
        run_id: None,
        change,
        status: None,
    };
    serde_json::to_value(&notif).unwrap_or(Value::Null)
}

#[async_trait]
impl ExtensionHandler for ScheduledTaskHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => handle_list(params, ctx).await,
            "run" => handle_run(params, ctx).await,
            "cancel" => handle_cancel(params, ctx).await,
            "create" => handle_create(params, ctx).await,
            "update" => handle_update(params, ctx).await,
            "delete" => handle_delete(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "run": true,
            "cancel": true,
            "create": true,
            "update": true,
            "delete": true
        })
    }
}

fn parse_task_input(params: &Value) -> Result<(String, String, bool, String), ExtensionError> {
    let name = require_param_str(params, "name")?;
    let schedule = require_param_str(params, "schedule")?;
    if schedule.trim().is_empty() {
        return Err(ExtensionError::invalid_params("schedule must not be empty"));
    }
    let description = optional_param_str(params, "description").unwrap_or_default();
    let enabled = params
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Ok((name, description, enabled, schedule))
}

async fn handle_create(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "scheduled-task", "create")?;

    let (name, description, enabled, schedule) = parse_task_input(&params)?;

    let mut store = load_store(ctx)?;

    if store.tasks.iter().any(|t| t.name == name) {
        return Err(ExtensionError::conflict(format!(
            "name_conflict: a task named '{name}' already exists"
        )));
    }

    let task = ScheduledTask {
        id: format!("task-{}", uuid::Uuid::new_v4()),
        name,
        description,
        enabled,
        schedule,
        last_run: None,
        last_run_status: None,
        next_run: None,
    };
    let value = serde_json::to_value(&task).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("failed to serialize task: {e}"))),
    })?;
    let id = task.id.clone();
    store.tasks.push(task);
    save_store(ctx, &mut store)?;

    Ok(serde_json::json!({
        "task": value,
        "notification": build_crud_notification(&id, TaskChangeType::Created),
    }))
}

async fn handle_update(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "scheduled-task", "update")?;

    let id = require_param_str(&params, "id")?;
    let mut store = load_store(ctx)?;

    let new_name = optional_param_str(&params, "name");
    if let Some(ref name) = new_name {
        if store.tasks.iter().any(|t| t.name == *name && t.id != id) {
            return Err(ExtensionError::conflict(format!(
                "name_conflict: a task named '{name}' already exists"
            )));
        }
    }

    let task = store
        .tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| ExtensionError::not_found(format!("task '{id}' not found")))?;

    if let Some(name) = new_name {
        task.name = name;
    }
    if params.get("description").map(|v| v.is_string()).unwrap_or(false) {
        task.description = optional_param_str(&params, "description").unwrap_or_default();
    }
    if let Some(enabled) = params.get("enabled").and_then(|v| v.as_bool()) {
        task.enabled = enabled;
    }
    if let Some(schedule) = optional_param_str(&params, "schedule") {
        task.schedule = schedule;
    }

    let value = serde_json::to_value(&*task).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("failed to serialize task: {e}"))),
    })?;
    save_store(ctx, &mut store)?;

    Ok(serde_json::json!({
        "task": value,
        "notification": build_crud_notification(&id, TaskChangeType::Updated),
    }))
}

async fn handle_delete(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "scheduled-task", "delete")?;

    let id = require_param_str(&params, "id")?;
    let mut store = load_store(ctx)?;

    let existed = store
        .tasks
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| ExtensionError::not_found(format!("task '{id}' not found")))?;
    store.tasks.remove(existed);
    store.runs.retain(|r| r.task_id != id);
    save_store(ctx, &mut store)?;

    Ok(serde_json::json!({
        "deleted": true,
        "notification": build_crud_notification(&id, TaskChangeType::Deleted),
    }))
}

async fn handle_list(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let pagination: PaginationParams = serde_json::from_value(params.clone())
        .map_err(|e| ExtensionError::invalid_params(format!("invalid pagination params: {e}")))?;

    let store = load_store(ctx)?;

    let mut tasks: Vec<ScheduledTask> = store.tasks;
    tasks.sort_by(|a, b| a.id.cmp(&b.id));

    let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
    let offset = pagination
        .decode_cursor::<serde_json::Value>()?
        .and_then(|v| v.get("offset").and_then(|o| o.as_u64()))
        .map(|o| o as usize)
        .unwrap_or(0);

    let items: Vec<Value> = tasks
        .into_iter()
        .map(|t| serde_json::to_value(&t).unwrap_or(Value::Null))
        .collect();

    let result = PaginatedResult::from_slice(items, offset, limit);
    Ok(result.to_json())
}

async fn handle_run(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "scheduled-task", "run")?;

    let id = require_param_str(&params, "id")?;
    let idempotency_key = require_param_str(&params, "idempotencyKey")?;

    let mut store = load_store(ctx)?;

    if let Some(existing_run) = store
        .runs
        .iter()
        .find(|r| r.idempotency_key == idempotency_key)
    {
        let existing = existing_run.clone();
        return Ok(serde_json::json!({
            "id": existing.task_id,
            "runId": existing.run_id,
            "status": existing.status,
            "startedAt": existing.started_at,
            "notification": build_notification(
                &existing.task_id,
                Some(&existing.run_id),
                TaskChangeType::Started,
                existing.status.clone(),
            ),
        }));
    }

    let task = store
        .tasks
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| ExtensionError::not_found(format!("task '{id}' not found")))?;

    if !task.enabled {
        return Err(ExtensionError::forbidden(format!(
            "task '{id}' is disabled"
        )));
    }

    let has_in_progress = store
        .runs
        .iter()
        .any(|r| r.task_id == id && r.status == TaskRunStatus::Running);

    if has_in_progress {
        return Err(ExtensionError::conflict(format!(
            "already_in_progress: task '{id}' already has a run in progress"
        )));
    }

    let now = now_iso();
    let run_id = generate_run_id();

    let run = TaskRun {
        run_id: run_id.clone(),
        task_id: id.clone(),
        status: TaskRunStatus::Running,
        started_at: now.clone(),
        ended_at: None,
        idempotency_key: idempotency_key.clone(),
        output: None,
    };

    store.runs.push(run);

    if let Some(task) = store.tasks.iter_mut().find(|t| t.id == id) {
        task.last_run = Some(now.clone());
    }

    save_store(ctx, &mut store)?;

    Ok(serde_json::json!({
        "id": id,
        "runId": run_id,
        "status": "running",
        "startedAt": now,
        "notification": build_notification(
            &id,
            Some(&run_id),
            TaskChangeType::Started,
            TaskRunStatus::Running,
        ),
    }))
}

async fn handle_cancel(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "scheduled-task", "cancel")?;

    let id = require_param_str(&params, "id")?;
    let run_id = optional_param_str(&params, "runId");

    let mut store = load_store(ctx)?;

    let task_exists = store.tasks.iter().any(|t| t.id == id);
    if !task_exists {
        return Err(ExtensionError::not_found(format!("task '{id}' not found")));
    }

    let now = now_iso();

    let target_run_idx = if let Some(ref target_run_id) = run_id {
        store
            .runs
            .iter()
            .position(|r| r.run_id == *target_run_id && r.task_id == id)
    } else {
        store
            .runs
            .iter()
            .position(|r| r.task_id == id && r.status == TaskRunStatus::Running)
    };

    let run_idx = target_run_idx.ok_or_else(|| {
        ExtensionError::not_found(format!("no in-progress run found for task '{id}'"))
    })?;

    let run = &mut store.runs[run_idx];
    run.status = TaskRunStatus::Cancelled;
    run.ended_at = Some(now.clone());
    let cancelled_run_id = run.run_id.clone();

    if let Some(task) = store.tasks.iter_mut().find(|t| t.id == id) {
        task.last_run_status = Some(TaskRunStatus::Cancelled);
    }

    save_store(ctx, &mut store)?;

    Ok(serde_json::json!({
        "id": id,
        "runId": cancelled_run_id,
        "status": "cancelled",
        "cancelledAt": now,
        "notification": build_notification(
            &id,
            Some(&cancelled_run_id),
            TaskChangeType::Cancelled,
            TaskRunStatus::Cancelled,
        ),
    }))
}
