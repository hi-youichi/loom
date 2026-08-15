use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::auth;
use super::boundary;
use super::progress::ProgressUpdate;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoReviewStartRequest {
    pub session_id: String,
    #[serde(default)]
    pub options: AutoReviewOptions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoReviewOptions {
    #[serde(default)]
    pub trigger: ReviewTrigger,
    #[serde(default)]
    pub create_review_session: bool,
    #[serde(default)]
    pub severity_filter: Option<Vec<ReviewSeverity>>,
    #[serde(default)]
    pub focus_areas: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for AutoReviewOptions {
    fn default() -> Self {
        Self {
            trigger: ReviewTrigger::OnTurnComplete,
            create_review_session: false,
            severity_filter: None,
            focus_areas: Vec::new(),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ReviewTrigger {
    #[default]
    OnTurnComplete,
    ManualOnly,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReviewSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReviewStartResponse {
    pub session_id: String,
    pub active: bool,
    pub trigger: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoReviewStopRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReviewStopResponse {
    pub session_id: String,
    pub active: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoReviewStatusRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReviewStatusResponse {
    pub session_id: String,
    pub active: bool,
    pub trigger: String,
    pub last_review: Option<ReviewSummary>,
    pub pending_review: Option<PendingReview>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub review_id: String,
    pub reviewed_turn_index: u32,
    pub review_session_id: Option<String>,
    pub status: ReviewStatus,
    pub severity: SeverityCounts,
    pub files_reviewed: Vec<String>,
    pub summary: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeverityCounts {
    pub critical: u32,
    pub error: u32,
    pub warning: u32,
    pub info: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReview {
    pub turn_index: u32,
    pub status: ReviewStatus,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReviewResultParams {
    pub session_id: String,
    pub review_id: String,
    pub reviewed_turn_index: u32,
    pub review_session_id: Option<String>,
    pub status: ReviewStatus,
    pub severity: SeverityCounts,
    pub files_reviewed: Vec<String>,
    pub inline_comments: Vec<InlineComment>,
    pub summary: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineComment {
    pub file: String,
    pub line: u32,
    pub severity: ReviewSeverity,
    pub rule: String,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReviewProgressParams {
    pub operation_id: String,
    pub domain: String,
    pub method: String,
    pub status: super::progress::ProgressStatus,
    pub message: Option<String>,
    pub percent: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct SessionReviewState {
    pub options: AutoReviewOptions,
    pub active: bool,
    pub pending_review: Option<PendingReview>,
    pub last_review: Option<ReviewSummary>,
    pub cancellation: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub admitted_turns: std::collections::HashSet<u32>,
    pub latest_turn_index: Option<u32>,
}

pub type ReviewStateStore = Arc<RwLock<HashMap<String, SessionReviewState>>>;

pub trait AutoReviewSessionStore: Send + Sync {
    fn exists(&self, session_id: &str) -> Result<bool, String>;
    fn can_access(&self, principal: &str, session_id: &str) -> Result<bool, String>;
}

pub trait AutoReviewNotificationSink: Send + Sync {
    fn notify(&self, method: &str, params: Value) -> Result<(), String>;
}

struct ContextSessionStore;
impl AutoReviewSessionStore for ContextSessionStore {
    fn exists(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
    fn can_access(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(false)
    }
}

struct NoopNotificationSink;
impl AutoReviewNotificationSink for NoopNotificationSink {
    fn notify(&self, _: &str, _: Value) -> Result<(), String> {
        Ok(())
    }
}

fn trigger_name(trigger: &ReviewTrigger) -> &'static str {
    match trigger {
        ReviewTrigger::OnTurnComplete => "on_turn_complete",
        ReviewTrigger::ManualOnly => "manual_only",
    }
}

pub struct AutoReviewHandler {
    states: ReviewStateStore,
    sessions: Arc<dyn AutoReviewSessionStore>,
    notifications: Arc<dyn AutoReviewNotificationSink>,
}

impl AutoReviewHandler {
    pub fn new() -> Self {
        Self::with_dependencies(
            Arc::new(ContextSessionStore),
            Arc::new(NoopNotificationSink),
        )
    }

    pub fn with_dependencies(
        sessions: Arc<dyn AutoReviewSessionStore>,
        notifications: Arc<dyn AutoReviewNotificationSink>,
    ) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            sessions,
            notifications,
        }
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid parameters"))
    }

    fn session_id(id: &str) -> Result<(), ExtensionError> {
        if id.trim().is_empty() {
            Err(ExtensionError::invalid_params(
                "sessionId must not be empty",
            ))
        } else {
            Ok(())
        }
    }

    fn options(options: &AutoReviewOptions) -> Result<(), ExtensionError> {
        if options.focus_areas.len() > 32
            || options
                .focus_areas
                .iter()
                .any(|area| area.trim().is_empty() || area.len() > 128)
        {
            return Err(ExtensionError::invalid_params("invalid focusAreas"));
        }
        if options.model.as_ref().is_some_and(|model| {
            model.trim().is_empty()
                || model.len() > 128
                || !model
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
        }) {
            return Err(ExtensionError::invalid_params("invalid model"));
        }
        Ok(())
    }

    fn encode<T: Serialize>(value: T) -> Result<Value, ExtensionError> {
        serde_json::to_value(value).map_err(|_| Self::internal("response serialization failed"))
    }

    fn authorized(
        &self,
        ctx: &ExtensionContext,
        id: &str,
        write: bool,
    ) -> Result<(), ExtensionError> {
        if let Some(context_session_id) = &ctx.session_id {
            if context_session_id != id {
                return Err(ExtensionError::forbidden("session binding mismatch"));
            }
        }
        if write {
            auth::check_capability(ctx, "auto-review", "start")
                .and_then(|_| auth::check_server_policy(ctx, "auto-review", "start"))?;
        } else if ctx.principal.trim().is_empty() {
            return Err(ExtensionError::forbidden("authentication required"));
        }
        if !self
            .sessions
            .can_access(&ctx.principal, id)
            .map_err(|_| Self::internal("session authorization failed"))?
        {
            return Err(ExtensionError::forbidden("session access denied"));
        }
        if !self
            .sessions
            .exists(id)
            .map_err(|_| Self::internal("session lookup failed"))?
        {
            return Err(ExtensionError::not_found("session does not exist"));
        }
        Ok(())
    }

    async fn start(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: AutoReviewStartRequest = Self::parse(params)?;
        Self::session_id(&request.session_id)?;
        Self::options(&request.options)?;
        self.authorized(ctx, &request.session_id, true)?;
        let operation_id = Uuid::new_v4().to_string();
        self.notifications
            .notify(
                "_loomdesk.dev/auto-review/progress",
                Self::encode(ProgressUpdate::started(
                    &operation_id,
                    "auto-review",
                    "start",
                ))?,
            )
            .map_err(|_| Self::internal("progress notification failed"))?;
        let mut states = self.states.write().await;
        let state =
            states
                .entry(request.session_id.clone())
                .or_insert_with(|| SessionReviewState {
                    options: request.options.clone(),
                    active: false,
                    pending_review: None,
                    last_review: None,
                    cancellation: None,
                    admitted_turns: Default::default(),
                    latest_turn_index: None,
                });
        if !state.active {
            state.options = request.options.clone();
            state.active = true;
        }
        let response = Self::encode(AutoReviewStartResponse {
            session_id: request.session_id,
            active: state.active,
            trigger: trigger_name(&state.options.trigger).into(),
            message: "Auto-review enabled. Will review code changes after each turn.".into(),
        })?;
        drop(states);
        self.notifications
            .notify(
                "_loomdesk.dev/auto-review/progress",
                Self::encode(ProgressUpdate::completed(
                    &operation_id,
                    "auto-review",
                    "start",
                ))?,
            )
            .map_err(|_| Self::internal("progress notification failed"))?;
        Ok(response)
    }

    async fn stop(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: AutoReviewStopRequest = Self::parse(params)?;
        Self::session_id(&request.session_id)?;
        self.authorized(ctx, &request.session_id, true)?;
        let mut states = self.states.write().await;
        if let Some(state) = states.get_mut(&request.session_id) {
            state.active = false;
            if let Some(flag) = &state.cancellation {
                flag.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(pending) = &mut state.pending_review {
                pending.status = ReviewStatus::Cancelled;
            } else {
                state.cancellation = None;
            }
        }
        Self::encode(AutoReviewStopResponse {
            session_id: request.session_id,
            active: false,
            message: "Auto-review disabled.".into(),
        })
    }

    async fn status(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: AutoReviewStatusRequest = Self::parse(params)?;
        Self::session_id(&request.session_id)?;
        self.authorized(ctx, &request.session_id, false)?;
        let states = self.states.read().await;
        let state = states.get(&request.session_id);
        Self::encode(AutoReviewStatusResponse {
            session_id: request.session_id,
            active: state.is_some_and(|v| v.active),
            trigger: state
                .map(|v| trigger_name(&v.options.trigger).into())
                .unwrap_or_else(|| "on_turn_complete".into()),
            last_review: state.and_then(|v| v.last_review.clone()),
            pending_review: state.and_then(|v| v.pending_review.clone()),
        })
    }

    pub async fn admit_turn(
        &self,
        session_id: &str,
        turn_index: u32,
    ) -> Result<bool, ExtensionError> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(session_id)
            .ok_or_else(|| ExtensionError::not_found("session does not exist"))?;
        if !state.active
            || state.options.trigger != ReviewTrigger::OnTurnComplete
            || !state.admitted_turns.insert(turn_index)
        {
            return Ok(false);
        }
        if state
            .latest_turn_index
            .is_some_and(|latest| turn_index < latest)
        {
            return Ok(false);
        }
        state.latest_turn_index = Some(turn_index);
        state.pending_review = Some(PendingReview {
            turn_index,
            status: ReviewStatus::InProgress,
            started_at: Utc::now(),
        });
        state.cancellation = Some(Arc::new(std::sync::atomic::AtomicBool::new(false)));
        Ok(true)
    }

    pub async fn record_result(
        &self,
        mut result: AutoReviewResultParams,
    ) -> Result<(), ExtensionError> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(&result.session_id)
            .ok_or_else(|| ExtensionError::not_found("session does not exist"))?;
        if state
            .cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
        {
            result.status = ReviewStatus::Cancelled;
        }
        if state
            .pending_review
            .as_ref()
            .is_some_and(|pending| pending.turn_index != result.reviewed_turn_index)
        {
            return Ok(());
        }
        if state.pending_review.is_none()
            && state
                .last_review
                .as_ref()
                .is_some_and(|review| review.reviewed_turn_index == result.reviewed_turn_index)
        {
            return Ok(());
        }
        if !matches!(
            result.status,
            ReviewStatus::Completed | ReviewStatus::Failed | ReviewStatus::Cancelled
        ) {
            return Err(ExtensionError::invalid_params("result status is not final"));
        }
        if result.review_id.len() > 256
            || result.summary.len() > 16_384
            || result.files_reviewed.len() > 4_096
            || result.files_reviewed.iter().any(|file| file.len() > 1_024)
            || result.inline_comments.len() > 10_000
            || result.inline_comments.iter().any(|comment| {
                comment.file.len() > 1_024
                    || comment.rule.len() > 512
                    || comment.message.len() > 8_192
                    || comment
                        .suggestion
                        .as_ref()
                        .is_some_and(|value| value.len() > 8_192)
            })
        {
            return Err(Self::internal("review result exceeds size limits"));
        }
        if let Some(filter) = &state.options.severity_filter {
            result
                .inline_comments
                .retain(|comment| filter.contains(&comment.severity));
            result.severity = SeverityCounts {
                critical: result
                    .inline_comments
                    .iter()
                    .filter(|comment| comment.severity == ReviewSeverity::Critical)
                    .count() as u32,
                error: result
                    .inline_comments
                    .iter()
                    .filter(|comment| comment.severity == ReviewSeverity::Error)
                    .count() as u32,
                warning: result
                    .inline_comments
                    .iter()
                    .filter(|comment| comment.severity == ReviewSeverity::Warning)
                    .count() as u32,
                info: result
                    .inline_comments
                    .iter()
                    .filter(|comment| comment.severity == ReviewSeverity::Info)
                    .count() as u32,
            };
        }
        state.last_review = Some(ReviewSummary {
            review_id: result.review_id.clone(),
            reviewed_turn_index: result.reviewed_turn_index,
            review_session_id: result.review_session_id.clone(),
            status: result.status.clone(),
            severity: result.severity.clone(),
            files_reviewed: result.files_reviewed.clone(),
            summary: result.summary.clone(),
            generated_at: result.generated_at,
        });
        state.pending_review = None;
        state.cancellation = None;
        let params = Self::encode(result)?;
        drop(states);
        self.notifications
            .notify("_loomdesk.dev/auto-review/result", params)
            .map_err(|_| Self::internal("result notification failed"))
    }

    pub fn validate_changed_path(
        path: &str,
        working_directory: Option<&Path>,
    ) -> Result<PathBuf, ExtensionError> {
        if working_directory.is_none() {
            return Err(ExtensionError::directory_boundary_violation(path));
        }
        boundary::validate_path(path, working_directory)
    }
}

impl Default for AutoReviewHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for AutoReviewHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "start" => self.start(params, ctx).await,
            "stop" => self.stop(params, ctx).await,
            "status" => self.status(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"start": true, "stop": true, "status": true})
    }
}
