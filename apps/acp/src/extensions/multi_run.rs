use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth;
use super::pagination::{encode_cursor, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_CONCURRENCY: u8 = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MultiRunStatus {
    Pending,
    Running,
    Completed,
    PartiallyCompleted,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiRunCreateRequest {
    pub name: String,
    pub runs: Vec<MultiRunEntry>,
    #[serde(default = "default_concurrency")]
    pub concurrency: u8,
    #[serde(default)]
    pub stop_on_error: bool,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiRunEntry {
    pub label: String,
    pub prompt: PromptPayload,
    #[serde(default)]
    pub config: MultiRunSessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptPayload {
    pub text: String,
    #[serde(default)]
    pub context: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiRunSessionConfig {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiRunCancelRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiRunStatusRequest {
    pub id: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRunRunStatus {
    pub label: String,
    pub session_id: String,
    pub status: MultiRunStatus,
    pub stop_reason: Option<String>,
    pub error: Option<MultiRunError>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRunError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRunChangedParams {
    pub id: String,
    pub status: MultiRunStatus,
    pub completed_runs: u32,
    pub failed_runs: u32,
    pub total_runs: u32,
    pub last_run_label: Option<String>,
    pub last_run_status: Option<MultiRunStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRunProgressParams {
    pub operation_id: String,
    pub domain: String,
    pub method: String,
    pub status: String,
    pub message: Option<String>,
    pub percent: Option<u8>,
}

pub trait MultiRunCoordinator: Send + Sync {
    fn create_session(
        &self,
        ctx: &ExtensionContext,
        config: &MultiRunSessionConfig,
        metadata: Value,
    ) -> Result<String, ExtensionError>;
    fn prompt(
        &self,
        session_id: &str,
        prompt: &PromptPayload,
    ) -> Result<MultiRunExecution, ExtensionError>;
    fn cancel(&self, session_id: &str) -> Result<(), ExtensionError>;
}

#[derive(Debug, Clone)]
pub struct MultiRunExecution {
    pub status: MultiRunStatus,
    pub stop_reason: Option<String>,
    pub error: Option<MultiRunError>,
}

pub trait MultiRunPublisher: Send + Sync {
    fn publish_changed(
        &self,
        ctx: &ExtensionContext,
        params: MultiRunChangedParams,
    ) -> Result<(), ExtensionError>;
    fn publish_progress(
        &self,
        ctx: &ExtensionContext,
        params: MultiRunProgressParams,
    ) -> Result<(), ExtensionError>;
}

#[derive(Default)]
struct NoopPublisher;

impl MultiRunPublisher for NoopPublisher {
    fn publish_changed(
        &self,
        _: &ExtensionContext,
        _: MultiRunChangedParams,
    ) -> Result<(), ExtensionError> {
        Ok(())
    }
    fn publish_progress(
        &self,
        _: &ExtensionContext,
        _: MultiRunProgressParams,
    ) -> Result<(), ExtensionError> {
        Ok(())
    }
}

struct LocalCoordinator;

impl MultiRunCoordinator for LocalCoordinator {
    fn create_session(
        &self,
        _: &ExtensionContext,
        _: &MultiRunSessionConfig,
        _: Value,
    ) -> Result<String, ExtensionError> {
        Ok(format!("sess_{}", uuid::Uuid::new_v4().simple()))
    }

    fn prompt(&self, _: &str, _: &PromptPayload) -> Result<MultiRunExecution, ExtensionError> {
        Ok(MultiRunExecution {
            status: MultiRunStatus::Completed,
            stop_reason: Some("end_turn".into()),
            error: None,
        })
    }

    fn cancel(&self, _: &str) -> Result<(), ExtensionError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MultiRunRecord {
    id: String,
    owner: String,
    name: String,
    stop_on_error: bool,
    runs: Vec<MultiRunRunStatus>,
    created_at: String,
    completed_at: Option<String>,
    fingerprint: String,
}

#[derive(Default)]
struct Store {
    records: HashMap<String, MultiRunRecord>,
    idempotency: HashMap<(String, String), String>,
}

pub struct MultiRunHandler {
    store: Arc<RwLock<Store>>,
    coordinator: Arc<dyn MultiRunCoordinator>,
    publisher: Arc<dyn MultiRunPublisher>,
}

impl MultiRunHandler {
    pub fn new() -> Self {
        Self::with_services(Arc::new(LocalCoordinator), Arc::new(NoopPublisher))
    }

    pub fn with_services(
        coordinator: Arc<dyn MultiRunCoordinator>,
        publisher: Arc<dyn MultiRunPublisher>,
    ) -> Self {
        Self {
            store: Arc::new(RwLock::new(Store::default())),
            coordinator,
            publisher,
        }
    }
}

impl Default for MultiRunHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn default_concurrency() -> u8 {
    1
}
fn default_limit() -> u32 {
    DEFAULT_LIMIT as u32
}
fn internal_error() -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: None,
    }
}
fn required_id(value: &str) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        Err(ExtensionError::invalid_params("id must not be empty"))
    } else {
        Ok(())
    }
}
fn object(params: Value) -> Result<Value, ExtensionError> {
    if params.is_object() {
        Ok(params)
    } else {
        Err(ExtensionError::invalid_params("params must be an object"))
    }
}

fn validate_create(request: &MultiRunCreateRequest) -> Result<(), ExtensionError> {
    if request.name.trim().is_empty()
        || request.runs.is_empty()
        || request.concurrency == 0
        || request.concurrency > MAX_CONCURRENCY
    {
        return Err(ExtensionError::invalid_params("invalid create parameters"));
    }
    let mut labels = HashSet::new();
    for run in &request.runs {
        if run.label.trim().is_empty()
            || !labels.insert(&run.label)
            || run.prompt.text.trim().is_empty()
        {
            return Err(ExtensionError::invalid_params(
                "labels and prompt.text must be nonempty and labels unique",
            ));
        }
        for value in [&run.config.mode, &run.config.model, &run.config.agent] {
            if value.as_ref().is_some_and(|v| v.trim().is_empty()) {
                return Err(ExtensionError::invalid_params(
                    "config values must not be empty",
                ));
            }
        }
    }
    if request
        .idempotency_key
        .as_ref()
        .is_some_and(|v| v.trim().is_empty())
    {
        return Err(ExtensionError::invalid_params(
            "idempotencyKey must not be empty",
        ));
    }
    Ok(())
}

fn aggregate(record: &MultiRunRecord) -> MultiRunStatus {
    let completed = record
        .runs
        .iter()
        .filter(|r| r.status == MultiRunStatus::Completed)
        .count();
    let failed = record
        .runs
        .iter()
        .filter(|r| r.status == MultiRunStatus::Failed)
        .count();
    let cancelled = record
        .runs
        .iter()
        .filter(|r| r.status == MultiRunStatus::Cancelled)
        .count();
    let terminal = completed + failed + cancelled;
    if terminal < record.runs.len() {
        if record
            .runs
            .iter()
            .any(|r| r.status == MultiRunStatus::Running)
        {
            MultiRunStatus::Running
        } else {
            MultiRunStatus::Pending
        }
    } else if failed == record.runs.len() {
        MultiRunStatus::Failed
    } else if failed > 0 {
        MultiRunStatus::PartiallyCompleted
    } else if cancelled > 0 {
        MultiRunStatus::Cancelled
    } else {
        MultiRunStatus::Completed
    }
}

fn counts(record: &MultiRunRecord) -> (u32, u32, u32) {
    (
        record
            .runs
            .iter()
            .filter(|r| r.status == MultiRunStatus::Completed)
            .count() as u32,
        record
            .runs
            .iter()
            .filter(|r| r.status == MultiRunStatus::Failed)
            .count() as u32,
        record
            .runs
            .iter()
            .filter(|r| r.status == MultiRunStatus::Cancelled)
            .count() as u32,
    )
}

fn complete_if_terminal(record: &mut MultiRunRecord, now: &str) {
    if record.runs.iter().all(|r| {
        matches!(
            r.status,
            MultiRunStatus::Completed | MultiRunStatus::Failed | MultiRunStatus::Cancelled
        )
    }) && record.completed_at.is_none()
    {
        record.completed_at = Some(now.into());
    }
}
fn snapshot(record: &MultiRunRecord) -> Value {
    let (completed, failed, _) = counts(record);
    serde_json::json!({"id":record.id,"name":record.name,"status":aggregate(record),"totalRuns":record.runs.len(),"completedRuns":completed,"failedRuns":failed,"sessionIds":record.runs.iter().map(|r|r.session_id.clone()).collect::<Vec<_>>(),"createdAt":record.created_at})
}

#[async_trait]
impl ExtensionHandler for MultiRunHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "create" => self.create(params, ctx),
            "cancel" => self.cancel(params, ctx),
            "status" => self.status(params, ctx),
            _ => Err(ExtensionError::method_not_found()),
        }
    }
    fn capabilities(&self) -> Value {
        serde_json::json!({"create":true,"cancel":true,"status":true})
    }
}

impl MultiRunHandler {
    fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_capability(ctx, "multi-run", "create")?;
        auth::check_server_policy(ctx, "multi-run", "create")?;
        let request: MultiRunCreateRequest = serde_json::from_value(object(params)?)
            .map_err(|e| ExtensionError::invalid_params(e.to_string()))?;
        validate_create(&request)?;
        let fingerprint = serde_json::to_string(&request).map_err(|_| internal_error())?;
        if let Some(key) = &request.idempotency_key {
            let store = self.store.read().map_err(|_| internal_error())?;
            if let Some(id) = store.idempotency.get(&(ctx.principal.clone(), key.clone())) {
                let record = store.records.get(id).ok_or_else(internal_error)?;
                if record.fingerprint == fingerprint {
                    return Ok(snapshot(record));
                }
                return Err(ExtensionError::conflict(
                    "idempotency key conflicts with another request",
                ));
            }
        }
        let id = format!("mr_{}", uuid::Uuid::new_v4().simple());
        let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
        self.publisher.publish_progress(
            ctx,
            MultiRunProgressParams {
                operation_id: operation_id.clone(),
                domain: "multi-run".into(),
                method: "create".into(),
                status: "started".into(),
                message: None,
                percent: Some(0),
            },
        )?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut runs = Vec::with_capacity(request.runs.len());
        for (index, entry) in request.runs.iter().enumerate() {
            let session_id = match self.coordinator.create_session(
                ctx,
                &entry.config,
                serde_json::json!({"openchamber":{"multirun":id}}),
            ) {
                Ok(value) => value,
                Err(error) => {
                    let failed = MultiRunRunStatus {
                        label: entry.label.clone(),
                        session_id: String::new(),
                        status: MultiRunStatus::Failed,
                        stop_reason: None,
                        error: Some(MultiRunError {
                            code: error.message.clone(),
                            message: error.message,
                        }),
                        started_at: None,
                        completed_at: Some(now.clone()),
                    };
                    runs.push(failed);
                    break;
                }
            };
            let execution = self.coordinator.prompt(&session_id, &entry.prompt)?;
            runs.push(MultiRunRunStatus {
                label: entry.label.clone(),
                session_id,
                status: execution.status,
                stop_reason: execution.stop_reason,
                error: execution.error,
                started_at: Some(now.clone()),
                completed_at: Some(now.clone()),
            });
            let percent = (((index + 1) * 100) / request.runs.len()) as u8;
            self.publisher.publish_progress(
                ctx,
                MultiRunProgressParams {
                    operation_id: operation_id.clone(),
                    domain: "multi-run".into(),
                    method: "create".into(),
                    status: "in_progress".into(),
                    message: None,
                    percent: Some(percent),
                },
            )?;
            if request.stop_on_error
                && runs
                    .last()
                    .is_some_and(|r| r.status == MultiRunStatus::Failed)
            {
                break;
            }
        }
        while runs.len() < request.runs.len() {
            let entry = &request.runs[runs.len()];
            runs.push(MultiRunRunStatus {
                label: entry.label.clone(),
                session_id: String::new(),
                status: MultiRunStatus::Cancelled,
                stop_reason: Some("stop_on_error".into()),
                error: None,
                started_at: None,
                completed_at: Some(now.clone()),
            });
        }
        let mut record = MultiRunRecord {
            id: id.clone(),
            owner: ctx.principal.clone(),
            name: request.name,
            stop_on_error: request.stop_on_error,
            runs,
            created_at: now.clone(),
            completed_at: None,
            fingerprint,
        };
        complete_if_terminal(&mut record, &now);
        let result = snapshot(&record);
        {
            let mut store = self.store.write().map_err(|_| internal_error())?;
            if let Some(key) = request.idempotency_key {
                store
                    .idempotency
                    .insert((ctx.principal.clone(), key), id.clone());
            }
            store.records.insert(id.clone(), record.clone());
        }
        let (completed, failed, _) = counts(&record);
        self.publisher.publish_progress(
            ctx,
            MultiRunProgressParams {
                operation_id,
                domain: "multi-run".into(),
                method: "create".into(),
                status: if failed > 0 {
                    "failed".into()
                } else {
                    "completed".into()
                },
                message: None,
                percent: Some(100),
            },
        )?;
        self.publisher.publish_changed(
            ctx,
            MultiRunChangedParams {
                id,
                status: aggregate(&record),
                completed_runs: completed,
                failed_runs: failed,
                total_runs: record.runs.len() as u32,
                last_run_label: None,
                last_run_status: None,
            },
        )?;
        Ok(result)
    }

    fn cancel(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_capability(ctx, "multi-run", "cancel")?;
        auth::check_server_policy(ctx, "multi-run", "cancel")?;
        let request: MultiRunCancelRequest = serde_json::from_value(object(params)?)
            .map_err(|e| ExtensionError::invalid_params(e.to_string()))?;
        required_id(&request.id)?;
        let mut store = self.store.write().map_err(|_| internal_error())?;
        let record = store
            .records
            .get_mut(&request.id)
            .ok_or_else(|| ExtensionError::not_found("multi-run not found"))?;
        if record.owner != ctx.principal {
            return Err(ExtensionError::forbidden(
                "multi-run is owned by another principal",
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let mut changed = false;
        for run in &mut record.runs {
            if matches!(
                run.status,
                MultiRunStatus::Pending | MultiRunStatus::Running
            ) {
                if !run.session_id.is_empty() {
                    self.coordinator.cancel(&run.session_id)?;
                }
                run.status = MultiRunStatus::Cancelled;
                run.stop_reason = Some("cancelled".into());
                run.completed_at = Some(now.clone());
                changed = true;
            }
        }
        complete_if_terminal(record, &now);
        let snapshot_record = record.clone();
        drop(store);
        let (completed, failed, cancelled) = counts(&snapshot_record);
        if changed {
            self.publisher.publish_changed(
                ctx,
                MultiRunChangedParams {
                    id: snapshot_record.id.clone(),
                    status: aggregate(&snapshot_record),
                    completed_runs: completed,
                    failed_runs: failed,
                    total_runs: snapshot_record.runs.len() as u32,
                    last_run_label: None,
                    last_run_status: None,
                },
            )?;
        }
        Ok(
            serde_json::json!({"id":snapshot_record.id,"status":aggregate(&snapshot_record),"cancelledRuns":cancelled,"completedRuns":completed}),
        )
    }

    fn status(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_capability(ctx, "multi-run", "status")?;
        auth::check_server_policy(ctx, "multi-run", "status")?;
        let request: MultiRunStatusRequest = serde_json::from_value(object(params)?)
            .map_err(|e| ExtensionError::invalid_params(e.to_string()))?;
        required_id(&request.id)?;
        if request.limit == 0 || request.limit as usize > MAX_LIMIT {
            return Err(ExtensionError::invalid_params("invalid limit"));
        }
        let pagination = PaginationParams {
            cursor: request.cursor.clone(),
            limit: Some(request.limit as usize),
        };
        let cursor: Option<serde_json::Value> = pagination.decode_cursor()?;
        let offset = cursor
            .map(|v| {
                let value = v
                    .get("offset")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ExtensionError::invalid_params("invalid cursor"))?;
                usize::try_from(value).map_err(|_| ExtensionError::invalid_params("invalid cursor"))
            })
            .transpose()?
            .unwrap_or(0);
        let store = self.store.read().map_err(|_| internal_error())?;
        let record = store
            .records
            .get(&request.id)
            .ok_or_else(|| ExtensionError::not_found("multi-run not found"))?;
        if record.owner != ctx.principal {
            return Err(ExtensionError::forbidden(
                "multi-run is owned by another principal",
            ));
        }
        if offset > record.runs.len() {
            return Err(ExtensionError::invalid_params("cursor is out of range"));
        }
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        let end = offset.saturating_add(limit).min(record.runs.len());
        let next_cursor =
            (end < record.runs.len()).then(|| encode_cursor(serde_json::json!({"offset":end})));
        let (completed, failed, _) = counts(record);
        Ok(
            serde_json::json!({"id":record.id,"name":record.name,"status":aggregate(record),"totalRuns":record.runs.len(),"completedRuns":completed,"failedRuns":failed,"createdAt":record.created_at,"completedAt":record.completed_at,"runs":record.runs[offset..end],"nextCursor":next_cursor,"hasMore":end < record.runs.len()}),
        )
    }
}
