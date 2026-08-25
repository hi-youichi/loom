use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

use config::home::anureo_home;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

pub struct GoalHandler;

impl GoalHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GoalHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Pending,
    Active,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

impl GoalStatus {
    fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalProgress {
    pub completed_steps: u32,
    pub total_steps: u32,
    pub percentage: u32,
    pub current_step: Option<String>,
    pub sessions_spawned: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStepStatus {
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalStep {
    pub index: u32,
    pub description: String,
    pub status: GoalStepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
    pub created_at: String,
    pub updated_at: String,
    pub session_ids: Vec<String>,
    #[serde(default)]
    pub progress: Option<GoalProgress>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing)]
    pub steps: Vec<GoalStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalStore {
    #[serde(default)]
    pub goals: Vec<Goal>,
}

impl GoalStore {
    fn find(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.iter_mut().find(|g| g.id == id)
    }

    fn find_by_idempotency_key(&self, key: &str) -> Option<&Goal> {
        self.goals
            .iter()
            .find(|g| g.idempotency_key.as_deref() == Some(key))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalChangeType {
    Started,
    Paused,
    Resumed,
    Cancelled,
    Completed,
    Progress,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalChangedNotification {
    pub id: String,
    pub change: GoalChangeType,
    pub status: GoalStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<GoalProgress>,
}

// ── Store helpers ──────────────────────────────────────────────────────

fn goals_file_path(ctx: &ExtensionContext) -> PathBuf {
    if let Some(wd) = &ctx.working_directory {
        wd.join(".anureo").join("goals.json")
    } else {
        anureo_home().join("goals.json")
    }
}

fn load_store(ctx: &ExtensionContext) -> Result<GoalStore, ExtensionError> {
    let path = goals_file_path(ctx);
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            if contents.trim().is_empty() {
                return Ok(GoalStore::default());
            }
            serde_json::from_str::<GoalStore>(&contents).map_err(|e| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(format!(
                    "failed to parse goals store at {}: {e}",
                    path.display()
                ))),
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GoalStore::default()),
        Err(e) => Err(ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(format!(
                "failed to read goals store at {}: {e}",
                path.display()
            ))),
        }),
    }
}

fn save_store(ctx: &ExtensionContext, store: &GoalStore) -> Result<(), ExtensionError> {
    let path = goals_file_path(ctx);
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
            "failed to serialize goals store: {e}"
        ))),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to write goals store at {}: {e}",
            tmp.display()
        ))),
    })?;
    std::fs::rename(&tmp, &path).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("failed to rename goals store: {e}"))),
    })
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn generate_goal_id() -> String {
    format!("goal-{}", uuid::Uuid::new_v4())
}

fn build_notification(
    id: &str,
    change: GoalChangeType,
    status: GoalStatus,
    progress: Option<GoalProgress>,
) -> Value {
    let notif = GoalChangedNotification {
        id: id.to_string(),
        change,
        status,
        progress,
    };
    serde_json::to_value(&notif).unwrap_or(Value::Null)
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

// ── ExtensionHandler impl ──────────────────────────────────────────────

#[async_trait]
impl ExtensionHandler for GoalHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => handle_list(params, ctx).await,
            "get" => handle_get(params, ctx).await,
            "start" => handle_start(params, ctx).await,
            "pause" => handle_pause(params, ctx).await,
            "resume" => handle_resume(params, ctx).await,
            "cancel" => handle_cancel(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "get": true,
            "start": true,
            "pause": true,
            "resume": true,
            "cancel": true
        })
    }
}

// ── Method handlers ────────────────────────────────────────────────────

async fn handle_list(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let pagination: PaginationParams = serde_json::from_value(params.clone())
        .map_err(|e| ExtensionError::invalid_params(format!("invalid pagination params: {e}")))?;

    let status_filter = params
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(GoalStatus::from_str_ci);

    let store = load_store(ctx)?;

    let mut goals: Vec<Goal> = store
        .goals
        .into_iter()
        .filter(|g| match &status_filter {
            Some(s) => &g.status == s,
            None => true,
        })
        .collect();

    goals.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
    let offset = pagination
        .decode_cursor::<serde_json::Value>()?
        .and_then(|v| v.get("offset").and_then(|o| o.as_u64()))
        .map(|o| o as usize)
        .unwrap_or(0);

    let items: Vec<Value> = goals
        .into_iter()
        .map(|g| serde_json::to_value(&g).unwrap_or(Value::Null))
        .collect();

    let result = PaginatedResult::from_slice(items, offset, limit);
    Ok(result.to_json())
}

async fn handle_get(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let id = require_param_str(&params, "id")?;

    let store = load_store(ctx)?;

    let goal = store
        .find(&id)
        .ok_or_else(|| ExtensionError::not_found(format!("goal '{id}' not found")))?;

    let mut result = serde_json::to_value(goal).unwrap_or(Value::Null);
    result["steps"] = serde_json::to_value(&goal.steps).unwrap_or(Value::Array(vec![]));

    Ok(result)
}

async fn handle_start(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "goal", "start")?;

    let title = require_param_str(&params, "title")?;
    let description = require_param_str(&params, "description")?;

    let session_id = optional_param_str(&params, "sessionId");
    let working_directory = optional_param_str(&params, "workingDirectory");
    let idempotency_key = optional_param_str(&params, "idempotencyKey");

    let mut store = load_store(ctx)?;

    if let Some(ref key) = idempotency_key {
        if let Some(existing) = store.find_by_idempotency_key(key) {
            let existing_goal = existing.clone();
            return Ok(serde_json::json!({
                "id": existing_goal.id,
                "title": existing_goal.title,
                "status": existing_goal.status,
                "sessionId": existing_goal.session_ids.first(),
                "createdAt": existing_goal.created_at,
                "notification": build_notification(
                    &existing_goal.id,
                    GoalChangeType::Started,
                    existing_goal.status.clone(),
                    existing_goal.progress.clone(),
                ),
            }));
        }
    }

    let now = now_iso();
    let goal_id = generate_goal_id();
    let session_ids: Vec<String> = session_id.iter().cloned().collect();

    let goal = Goal {
        id: goal_id.clone(),
        title: title.clone(),
        description,
        status: GoalStatus::Active,
        created_at: now.clone(),
        updated_at: now.clone(),
        session_ids,
        progress: None,
        metadata: None,
        steps: Vec::new(),
        idempotency_key,
        working_directory,
    };

    store.goals.push(goal);

    save_store(ctx, &store)?;

    Ok(serde_json::json!({
        "id": goal_id,
        "title": title,
        "status": "active",
        "sessionId": session_id,
        "createdAt": now,
        "notification": build_notification(
            &goal_id,
            GoalChangeType::Started,
            GoalStatus::Active,
            None,
        ),
    }))
}

async fn handle_pause(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "goal", "pause")?;

    let id = require_param_str(&params, "id")?;

    let mut store = load_store(ctx)?;

    let goal = store
        .find_mut(&id)
        .ok_or_else(|| ExtensionError::not_found(format!("goal '{id}' not found")))?;

    if goal.status != GoalStatus::Active {
        return Err(ExtensionError::invalid_params(format!(
            "goal '{id}' is not active (current status: {:?}); only active goals can be paused",
            goal.status
        )));
    }

    goal.status = GoalStatus::Paused;
    let now = now_iso();
    goal.updated_at = now.clone();

    let progress = goal.progress.clone();

    save_store(ctx, &store)?;

    Ok(serde_json::json!({
        "id": id,
        "status": "paused",
        "pausedAt": now,
        "notification": build_notification(
            &id,
            GoalChangeType::Paused,
            GoalStatus::Paused,
            progress,
        ),
    }))
}

async fn handle_resume(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "goal", "resume")?;

    let id = require_param_str(&params, "id")?;

    let mut store = load_store(ctx)?;

    let goal = store
        .find_mut(&id)
        .ok_or_else(|| ExtensionError::not_found(format!("goal '{id}' not found")))?;

    if goal.status != GoalStatus::Paused {
        return Err(ExtensionError::invalid_params(format!(
            "goal '{id}' is not paused (current status: {:?}); only paused goals can be resumed",
            goal.status
        )));
    }

    goal.status = GoalStatus::Active;
    let now = now_iso();
    goal.updated_at = now.clone();

    let progress = goal.progress.clone();

    save_store(ctx, &store)?;

    Ok(serde_json::json!({
        "id": id,
        "status": "active",
        "resumedAt": now,
        "notification": build_notification(
            &id,
            GoalChangeType::Resumed,
            GoalStatus::Active,
            progress,
        ),
    }))
}

async fn handle_cancel(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "goal", "cancel")?;

    let id = require_param_str(&params, "id")?;
    let reason = optional_param_str(&params, "reason");

    let mut store = load_store(ctx)?;

    let goal = store
        .find_mut(&id)
        .ok_or_else(|| ExtensionError::not_found(format!("goal '{id}' not found")))?;

    if goal.status.is_terminal() {
        let cancelled_at = goal.updated_at.clone();
        let progress = goal.progress.clone();
        let current_status = goal.status.clone();
        return Ok(serde_json::json!({
            "id": id,
            "status": "cancelled",
            "cancelledAt": cancelled_at,
            "notification": build_notification(
                &id,
                GoalChangeType::Cancelled,
                current_status,
                progress,
            ),
        }));
    }

    if let Some(ref r) = reason {
        let meta = goal
            .metadata
            .clone()
            .unwrap_or(Value::Object(Default::default()));
        let mut meta_obj = meta.as_object().cloned().unwrap_or_default();
        meta_obj.insert("cancellationReason".to_string(), Value::String(r.clone()));
        goal.metadata = Some(Value::Object(meta_obj));
    }

    goal.status = GoalStatus::Cancelled;
    let now = now_iso();
    goal.updated_at = now.clone();

    let progress = goal.progress.clone();

    save_store(ctx, &store)?;

    Ok(serde_json::json!({
        "id": id,
        "status": "cancelled",
        "cancelledAt": now,
        "notification": build_notification(
            &id,
            GoalChangeType::Cancelled,
            GoalStatus::Cancelled,
            progress,
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use std::path::PathBuf;

    fn make_ctx(wd: PathBuf) -> ExtensionContext {
        ExtensionContext {
            session_id: None,
            principal: "test-user".to_string(),
            connection_id: "test-conn".to_string(),
            working_directory: Some(wd),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    fn make_ctx_no_principal(wd: PathBuf) -> ExtensionContext {
        ExtensionContext {
            session_id: None,
            principal: String::new(),
            connection_id: "test-conn".to_string(),
            working_directory: Some(wd),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn list_empty_when_no_store() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
        assert_eq!(result["hasMore"], false);
    }

    #[tokio::test]
    async fn list_with_status_filter() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        handler
            .handle(
                "start",
                serde_json::json!({"title": "A", "description": "desc"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle("list", serde_json::json!({"status": "active"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        let result = handler
            .handle("list", serde_json::json!({"status": "paused"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_status_filter_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        handler
            .handle(
                "start",
                serde_json::json!({"title": "A", "description": "d"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle("list", serde_json::json!({"status": "Active"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_pagination() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        for i in 0..3 {
            handler
                .handle(
                    "start",
                    serde_json::json!({"title": format!("G{i}"), "description": "d"}),
                    &ctx,
                )
                .await
                .unwrap();
        }
        let page1 = handler
            .handle("list", serde_json::json!({"limit": 1}), &ctx)
            .await
            .unwrap();
        assert_eq!(page1["items"].as_array().unwrap().len(), 1);
        assert_eq!(page1["hasMore"], true);
        let cursor = page1["nextCursor"].as_str().unwrap();
        let page2 = handler
            .handle(
                "list",
                serde_json::json!({"limit": 1, "cursor": cursor}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(page2["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_items_omit_steps() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        handler
            .handle(
                "start",
                serde_json::json!({"title": "A", "description": "d"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        let item = &result["items"][0];
        assert!(item.get("steps").is_none() || item["steps"].as_array().is_none());
    }

    #[tokio::test]
    async fn get_existing_goal_returns_steps() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start_result = handler
            .handle(
                "start",
                serde_json::json!({"title": "A", "description": "d"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start_result["id"].as_str().unwrap();
        let result = handler
            .handle("get", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert!(result.get("steps").is_some());
        assert_eq!(result["steps"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_nonexistent_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("get", serde_json::json!({"id": "nope"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn get_missing_id_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("get", serde_json::json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn get_empty_id_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("get", serde_json::json!({"id": "  "}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn start_creates_goal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let result = handler
            .handle(
                "start",
                serde_json::json!({"title": "Test", "description": "Desc"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result["id"].as_str().unwrap().starts_with("goal-"));
        assert_eq!(result["title"], "Test");
        assert_eq!(result["status"], "active");
        assert!(result.get("createdAt").is_some());
        assert!(result.get("notification").is_some());
    }

    #[tokio::test]
    async fn start_with_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let result = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D", "sessionId": "sess-1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["sessionId"], "sess-1");
        let id = result["id"].as_str().unwrap();
        let goal = handler
            .handle("get", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(goal["sessionIds"][0], "sess-1");
    }

    #[tokio::test]
    async fn start_empty_title_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle(
                "start",
                serde_json::json!({"title": "  ", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn start_empty_description_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": ""}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn start_no_principal_returns_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx_no_principal(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn start_idempotency_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let r1 = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D", "idempotencyKey": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        let r2 = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D", "idempotencyKey": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(r1["id"], r2["id"]);
    }

    #[tokio::test]
    async fn start_different_idempotency_keys_create_separate() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let r1 = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D", "idempotencyKey": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        let r2 = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D", "idempotencyKey": "k2"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_ne!(r1["id"], r2["id"]);
    }

    #[tokio::test]
    async fn pause_active_goal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        let result = handler
            .handle("pause", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["status"], "paused");
        assert!(result.get("pausedAt").is_some());
    }

    #[tokio::test]
    async fn pause_non_active_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        handler
            .handle("pause", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        let err = handler
            .handle("pause", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn pause_nonexistent_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("pause", serde_json::json!({"id": "nope"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn pause_no_principal_returns_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx_no_principal(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("pause", serde_json::json!({"id": "x"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn resume_paused_goal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        handler
            .handle("pause", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        let result = handler
            .handle("resume", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["status"], "active");
        assert!(result.get("resumedAt").is_some());
    }

    #[tokio::test]
    async fn resume_active_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        let err = handler
            .handle("resume", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn resume_nonexistent_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("resume", serde_json::json!({"id": "nope"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn cancel_active_goal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        let result = handler
            .handle("cancel", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["status"], "cancelled");
        assert!(result.get("cancelledAt").is_some());
    }

    #[tokio::test]
    async fn cancel_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        handler
            .handle("cancel", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        let result = handler
            .handle("cancel", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["status"], "cancelled");
    }

    #[tokio::test]
    async fn cancel_with_reason_stores_in_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let start = handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        handler
            .handle(
                "cancel",
                serde_json::json!({"id": id, "reason": "done"}),
                &ctx,
            )
            .await
            .unwrap();
        let goal = handler
            .handle("get", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(goal["metadata"]["cancellationReason"], "done");
    }

    #[tokio::test]
    async fn cancel_nonexistent_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("cancel", serde_json::json!({"id": "nope"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }

    #[tokio::test]
    async fn cancel_no_principal_returns_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx_no_principal(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("cancel", serde_json::json!({"id": "x"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32002);
    }

    #[tokio::test]
    async fn state_transitions_persist_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());

        let handler1 = GoalHandler::new();
        let start = handler1
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let id = start["id"].as_str().unwrap();
        handler1
            .handle("pause", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();

        let handler2 = GoalHandler::new();
        let goal = handler2
            .handle("get", serde_json::json!({"id": id}), &ctx)
            .await
            .unwrap();
        assert_eq!(goal["status"], "paused");
    }

    #[tokio::test]
    async fn start_then_list_shows_goal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        handler
            .handle(
                "start",
                serde_json::json!({"title": "T", "description": "D"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn capabilities() {
        let handler = GoalHandler::new();
        let caps = handler.capabilities();
        assert_eq!(caps["list"], true);
        assert_eq!(caps["get"], true);
        assert_eq!(caps["start"], true);
        assert_eq!(caps["pause"], true);
        assert_eq!(caps["resume"], true);
        assert_eq!(caps["cancel"], true);
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let handler = GoalHandler::new();
        let err = handler
            .handle("unknown", serde_json::json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }
}
