use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

const IDEMPOTENCY_TTL_SECS: u64 = 300;

pub struct WorktreeHandler;

impl WorktreeHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WorktreeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for WorktreeHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => handle_list(params, ctx).await,
            "get" => handle_get(params, ctx).await,
            "validate" => handle_validate(params, ctx).await,
            "preview" => handle_preview(params, ctx).await,
            "bootstrap_status" => handle_bootstrap_status(params, ctx).await,
            "create" => handle_create(params, ctx).await,
            "delete" => handle_delete(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "get": true,
            "validate": true,
            "preview": true,
            "bootstrap_status": true,
            "create": true,
            "delete": true
        })
    }
}

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_detached: bool,
    pub is_dirty: bool,
    pub attention_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationConflict {
    #[serde(rename = "type")]
    pub conflict_type: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStepPlan {
    pub step: u32,
    pub command: String,
    pub estimated_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapStepResult {
    Pending,
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStepStatus {
    pub step: u32,
    pub command: String,
    pub status: BootstrapStepResult,
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapStatusInfo {
    pub status: BootstrapStatus,
    pub steps: Vec<BootstrapStepStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub steps: Vec<BootstrapConfigStep>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BootstrapConfigStep {
    pub command: String,
    #[serde(default = "default_estimated_ms")]
    pub estimated_ms: u32,
}

fn default_estimated_ms() -> u32 {
    30000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapStatusFile {
    pub status: BootstrapStatus,
    pub steps: Vec<BootstrapStepStatus>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdempotencyEntry {
    result: Value,
    timestamp: u64,
    branch: String,
    base_ref: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorktreeBranchRegistry {
    #[serde(default)]
    entries: Vec<WorktreeBranchEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorktreeBranchEntry {
    path: String,
    branch: String,
}

// ── Git CLI helpers ────────────────────────────────────────────────────

async fn run_git(ctx: &ExtensionContext, args: &[&str]) -> Result<String, ExtensionError> {
    let result = loom_git::facade::run_raw(ctx.working_directory.as_deref(), args).await;
    match result {
        Err(e) if matches!(e.kind(), loom_git::GitErrorKind::NotFound) => {
            Err(ExtensionError::not_found(e.message().to_string()))
        }
        Err(e) => Err(ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(e.data())),
        }),
        Ok(out) => Ok(out),
    }
}

fn require_param<T: for<'de> Deserialize<'de>>(
    params: &Value,
    key: &str,
) -> Result<T, ExtensionError> {
    let val = params.get(key).ok_or_else(|| {
        ExtensionError::invalid_params(format!("missing required parameter: {key}"))
    })?;
    serde_json::from_value(val.clone())
        .map_err(|_| ExtensionError::invalid_params(format!("invalid type for parameter: {key}")))
}

fn optional_param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn optional_param_bool(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

// ── Path validation ────────────────────────────────────────────────────

fn validate_path_string(path: &str) -> Result<(), ExtensionError> {
    if path.is_empty() {
        return Err(ExtensionError::invalid_params("path must not be empty"));
    }
    if path.contains("..") {
        return Err(ExtensionError::invalid_params(
            "path traversal (..) is not allowed",
        ));
    }
    Ok(())
}

fn validate_branch_name(branch: &str) -> Result<(), ExtensionError> {
    if branch.is_empty() {
        return Err(ExtensionError::invalid_params("branch must not be empty"));
    }
    if branch.contains("..") || branch.contains(" ") || branch.contains("\\") {
        return Err(ExtensionError::invalid_params(format!(
            "invalid branch name: {branch}"
        )));
    }
    if branch.starts_with('-') || branch.starts_with('.') {
        return Err(ExtensionError::invalid_params(format!(
            "invalid branch name: {branch}"
        )));
    }
    for c in branch.chars() {
        if c.is_ascii_control() || c == ':' || c == '?' || c == '*' || c == '[' || c == ']' {
            return Err(ExtensionError::invalid_params(format!(
                "invalid branch name: {branch}"
            )));
        }
    }
    Ok(())
}

fn sanitize_branch_for_path(branch: &str) -> String {
    branch.replace('/', "-")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Worktree porcelain parser ──────────────────────────────────────────

struct RawWorktreeEntry {
    path: String,
    head: String,
    branch: Option<String>,
    is_detached: bool,
    is_main: bool,
}

fn parse_worktree_porcelain(output: &str) -> Vec<RawWorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<RawWorktreeEntry> = None;
    let mut is_first = true;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(e) = current.take() {
                entries.push(e);
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(e) = current.take() {
                entries.push(e);
            }
            let path = path.to_string();
            current = Some(RawWorktreeEntry {
                path,
                head: String::new(),
                branch: None,
                is_detached: false,
                is_main: is_first,
            });
            is_first = false;
        } else if let Some(e) = current.as_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                e.head = head.to_string();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                let branch_name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                e.branch = Some(branch_name.to_string());
            } else if line == "detached" {
                e.is_detached = true;
            }
        }
    }
    if let Some(e) = current {
        entries.push(e);
    }
    entries
}

async fn check_worktree_dirty(path: &Path) -> bool {
    match loom_git::facade::run_raw(Some(path), &["status", "--porcelain"]).await {
        Ok(output) => !output.trim().is_empty(),
        Err(_) => false,
    }
}

async fn fetch_all_worktrees(ctx: &ExtensionContext) -> Result<Vec<WorktreeInfo>, ExtensionError> {
    let output = run_git(ctx, &["worktree", "list", "--porcelain"]).await?;
    let raw_entries = parse_worktree_porcelain(&output);

    let mut result = Vec::new();
    for entry in raw_entries {
        let path_buf = PathBuf::from(&entry.path);
        let is_dirty = check_worktree_dirty(&path_buf).await;

        let attention_reason = {
            let status_file = path_buf.join(".loomdesk").join("bootstrap-status.json");
            if status_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&status_file) {
                    if let Ok(status) = serde_json::from_str::<BootstrapStatusFile>(&content) {
                        match status.status {
                            BootstrapStatus::Pending => Some("bootstrap_pending".to_string()),
                            BootstrapStatus::Running => Some("bootstrap_running".to_string()),
                            BootstrapStatus::Failed => Some("bootstrap_failed".to_string()),
                            BootstrapStatus::Completed => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        result.push(WorktreeInfo {
            path: entry.path,
            head: entry.head,
            branch: entry.branch,
            is_main: entry.is_main,
            is_detached: entry.is_detached,
            is_dirty,
            attention_reason,
        });
    }
    Ok(result)
}

async fn find_worktree_by_path(
    ctx: &ExtensionContext,
    path: &str,
) -> Result<WorktreeInfo, ExtensionError> {
    let all = fetch_all_worktrees(ctx).await?;
    let target_canonical = PathBuf::from(path).canonicalize().ok();
    for wt in all {
        let wt_canonical = PathBuf::from(&wt.path).canonicalize().ok();
        if wt.path == path {
            return Ok(wt);
        }
        if let (Some(t), Some(w)) = (&target_canonical, &wt_canonical) {
            if t == w {
                return Ok(wt);
            }
        }
    }
    Err(ExtensionError::not_found(format!(
        "worktree not found: {path}"
    )))
}

// ── Main worktree root resolution ──────────────────────────────────────

async fn get_main_worktree_root(ctx: &ExtensionContext) -> Result<PathBuf, ExtensionError> {
    let git_dir_output = run_git(ctx, &["rev-parse", "--git-common-dir"]).await?;
    let git_dir = PathBuf::from(git_dir_output.trim());
    let main_root = git_dir
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(format!(
                "cannot resolve main worktree root from git-common-dir: {git_dir:?}"
            ))),
        })?;
    main_root.canonicalize().map_err(|_| {
        ExtensionError::not_found(format!("main worktree root does not exist: {main_root:?}"))
    })
}

fn compute_worktree_target_path(main_root: &Path, branch: &str) -> PathBuf {
    let sanitized = sanitize_branch_for_path(branch);
    main_root.join(".worktrees").join(sanitized)
}

fn loomdesk_dir(main_root: &Path) -> PathBuf {
    main_root.join(".loomdesk")
}

// ── Bootstrap config ───────────────────────────────────────────────────

fn read_bootstrap_config(main_root: &Path) -> Result<BootstrapConfig, ExtensionError> {
    let config_path = main_root.join(".loomdesk").join("bootstrap.json");
    if !config_path.exists() {
        return Ok(BootstrapConfig { steps: vec![] });
    }
    let content = std::fs::read_to_string(&config_path).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to read bootstrap config: {e}"
        ))),
    })?;
    serde_json::from_str(&content).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to parse bootstrap config: {e}"
        ))),
    })
}

fn read_bootstrap_status(worktree_path: &Path) -> Option<BootstrapStatusFile> {
    let status_file = worktree_path
        .join(".loomdesk")
        .join("bootstrap-status.json");
    let content = std::fs::read_to_string(&status_file).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_bootstrap_status(
    worktree_path: &Path,
    status: &BootstrapStatusFile,
) -> Result<(), ExtensionError> {
    let dir = worktree_path.join(".loomdesk");
    std::fs::create_dir_all(&dir).ok();
    let status_file = dir.join("bootstrap-status.json");
    let content = serde_json::to_string_pretty(status).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to serialize bootstrap status: {e}"
        ))),
    })?;
    std::fs::write(&status_file, content).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to write bootstrap status: {e}"
        ))),
    })?;
    Ok(())
}

async fn run_bootstrap(worktree_path: &Path, config: &BootstrapConfig) -> BootstrapStatusInfo {
    if config.steps.is_empty() {
        return BootstrapStatusInfo {
            status: BootstrapStatus::Completed,
            steps: vec![],
            error: None,
        };
    }

    let mut step_results = Vec::new();
    let mut failed = false;
    let mut error_msg = None;

    for (i, step) in config.steps.iter().enumerate() {
        let step_num = (i + 1) as u32;

        let started = SystemTime::now();
        let (shell, shell_arg) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let output = tokio::process::Command::new(shell)
            .arg(shell_arg)
            .arg(&step.command)
            .current_dir(worktree_path)
            .output()
            .await;

        let duration_ms = started.elapsed().map(|d| d.as_millis() as u32).unwrap_or(0);

        let (status, step_error) = match output {
            Ok(out) if out.status.success() => (BootstrapStepResult::Success, None),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let msg = format!("step {step_num} failed: {stderr}");
                failed = true;
                (BootstrapStepResult::Failed, Some(msg))
            }
            Err(e) => {
                let msg = format!("step {step_num} failed to execute: {e}");
                failed = true;
                (BootstrapStepResult::Failed, Some(msg))
            }
        };

        if let Some(ref msg) = step_error {
            if error_msg.is_none() {
                error_msg = Some(msg.clone());
            }
        }

        step_results.push(BootstrapStepStatus {
            step: step_num,
            command: step.command.clone(),
            status: status.clone(),
            duration_ms: Some(duration_ms),
        });

        if failed {
            break;
        }
    }

    let overall_status = if failed {
        BootstrapStatus::Failed
    } else {
        BootstrapStatus::Completed
    };

    let status_file = BootstrapStatusFile {
        status: overall_status.clone(),
        steps: step_results.clone(),
        error: error_msg.clone(),
    };
    let _ = write_bootstrap_status(worktree_path, &status_file);

    BootstrapStatusInfo {
        status: overall_status,
        steps: step_results,
        error: error_msg,
    }
}

// ── Idempotency ────────────────────────────────────────────────────────

fn save_idempotency(
    main_root: &Path,
    key: &str,
    result: Value,
    branch: &str,
    base_ref: &Option<String>,
) {
    let idem_dir = loomdesk_dir(main_root).join("idempotency");
    std::fs::create_dir_all(&idem_dir).ok();
    let entry = IdempotencyEntry {
        result,
        timestamp: now_secs(),
        branch: branch.to_string(),
        base_ref: base_ref.clone(),
    };
    if let Ok(content) = serde_json::to_string_pretty(&entry) {
        let file_path = idem_dir.join(format!("{key}.json"));
        let _ = std::fs::write(&file_path, content);
    }
}

fn check_idempotency_match(
    main_root: &Path,
    key: &str,
    branch: &str,
    base_ref: &Option<String>,
) -> Result<Option<Value>, ExtensionError> {
    let idem_dir = loomdesk_dir(main_root).join("idempotency");
    let file_path = idem_dir.join(format!("{key}.json"));
    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let entry: IdempotencyEntry = match serde_json::from_str(&content) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let now = now_secs();
    if now - entry.timestamp > IDEMPOTENCY_TTL_SECS {
        let _ = std::fs::remove_file(&file_path);
        return Ok(None);
    }
    if entry.branch != branch || entry.base_ref.as_deref() != base_ref.as_deref() {
        return Err(ExtensionError::invalid_params(format!(
            "idempotency key '{key}' was used with different branch/baseRef"
        )));
    }
    Ok(Some(entry.result))
}

// ── Worktree branch registry ───────────────────────────────────────────

fn read_branch_registry(main_root: &Path) -> WorktreeBranchRegistry {
    let registry_path = loomdesk_dir(main_root).join("worktree-branches.json");
    match std::fs::read_to_string(&registry_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => WorktreeBranchRegistry::default(),
    }
}

fn write_branch_registry(main_root: &Path, registry: &WorktreeBranchRegistry) {
    let dir = loomdesk_dir(main_root);
    std::fs::create_dir_all(&dir).ok();
    if let Ok(content) = serde_json::to_string_pretty(registry) {
        let registry_path = dir.join("worktree-branches.json");
        let _ = std::fs::write(&registry_path, content);
    }
}

fn register_branch(main_root: &Path, worktree_path: &str, branch: &str) {
    let mut registry = read_branch_registry(main_root);
    registry.entries.retain(|e| e.path != worktree_path);
    registry.entries.push(WorktreeBranchEntry {
        path: worktree_path.to_string(),
        branch: branch.to_string(),
    });
    write_branch_registry(main_root, &registry);
}

fn unregister_branch(main_root: &Path, worktree_path: &str) -> Option<String> {
    let mut registry = read_branch_registry(main_root);
    let branch = registry
        .entries
        .iter()
        .find(|e| e.path == worktree_path)
        .map(|e| e.branch.clone());
    registry.entries.retain(|e| e.path != worktree_path);
    write_branch_registry(main_root, &registry);
    branch
}

// ── Method handlers ────────────────────────────────────────────────────

async fn handle_list(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let pagination: PaginationParams = serde_json::from_value(params.clone())
        .map_err(|e| ExtensionError::invalid_params(format!("invalid pagination params: {e}")))?;

    let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
    let cursor_data: Option<serde_json::Map<String, Value>> = pagination
        .decode_cursor()
        .map_err(|_| ExtensionError::invalid_params("invalid cursor"))?;
    let offset = cursor_data
        .and_then(|m| m.get("offset").and_then(|v| v.as_u64()))
        .map(|v| v as usize)
        .unwrap_or(0);

    let all_worktrees = fetch_all_worktrees(ctx).await?;
    let result = PaginatedResult::from_slice(all_worktrees, offset, limit);
    Ok(result.to_json())
}

async fn handle_get(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let path: String = require_param(&params, "path")?;
    validate_path_string(&path)?;

    let wt_info = find_worktree_by_path(ctx, &path).await?;
    Ok(serde_json::to_value(wt_info).unwrap_or(Value::Null))
}

async fn handle_validate(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let branch: String = require_param(&params, "branch")?;
    validate_branch_name(&branch)?;

    let base_ref: Option<String> = optional_param_str(&params, "baseRef");

    let base_to_check = base_ref.as_deref().unwrap_or("HEAD");
    let rev_check = run_git(ctx, &["rev-parse", "--verify", base_to_check]).await;
    if rev_check.is_err() {
        return Err(ExtensionError::not_found(format!(
            "baseRef '{base_to_check}' does not exist"
        )));
    }

    let mut conflicts = Vec::new();

    let all_worktrees = fetch_all_worktrees(ctx).await?;
    for wt in &all_worktrees {
        if let Some(ref wt_branch) = wt.branch {
            if wt_branch == &branch {
                conflicts.push(ValidationConflict {
                    conflict_type: "branch_exists".to_string(),
                    detail: format!("Branch '{branch}' already exists and is checked out in another worktree ({})", wt.path),
                });
            }
        }
    }

    let existing_branch = run_git(
        ctx,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .await;
    if existing_branch.is_ok() {
        let already_in_conflicts = conflicts.iter().any(|c| c.conflict_type == "branch_exists");
        if !already_in_conflicts {
            conflicts.push(ValidationConflict {
                conflict_type: "branch_exists".to_string(),
                detail: format!("Branch '{branch}' already exists"),
            });
        }
    }

    let valid = conflicts.is_empty();
    Ok(serde_json::json!({
        "valid": valid,
        "conflicts": conflicts,
    }))
}

async fn handle_preview(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let branch: String = require_param(&params, "branch")?;
    validate_branch_name(&branch)?;

    let base_ref: Option<String> = optional_param_str(&params, "baseRef");
    let run_bootstrap: bool = optional_param_bool(&params, "runBootstrap", false);

    let base_to_check = base_ref.as_deref().unwrap_or("HEAD");
    let base_commit = run_git(ctx, &["rev-parse", "--verify", base_to_check])
        .await
        .map_err(|_| {
            ExtensionError::not_found(format!("baseRef '{base_to_check}' does not exist"))
        })?;

    let main_root = get_main_worktree_root(ctx).await?;

    let mut warnings = Vec::new();

    let all_worktrees = fetch_all_worktrees(ctx).await?;
    for wt in &all_worktrees {
        if let Some(ref wt_branch) = wt.branch {
            if wt_branch == &branch {
                warnings.push(format!(
                    "Branch '{branch}' already exists and is checked out in another worktree ({})",
                    wt.path
                ));
            }
        }
    }

    let existing_branch = run_git(
        ctx,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .await;
    if existing_branch.is_ok() && warnings.is_empty() {
        warnings.push(format!("Branch '{branch}' already exists"));
    }

    let mut bootstrap_plan = Vec::new();
    if run_bootstrap {
        let config = read_bootstrap_config(&main_root)?;
        for (i, step) in config.steps.iter().enumerate() {
            bootstrap_plan.push(BootstrapStepPlan {
                step: (i + 1) as u32,
                command: step.command.clone(),
                estimated_ms: step.estimated_ms,
            });
        }
    }

    let target_path = compute_worktree_target_path(&main_root, &branch);

    Ok(serde_json::json!({
        "targetPath": target_path.to_string_lossy(),
        "baseCommit": base_commit.trim(),
        "bootstrapPlan": bootstrap_plan,
        "warnings": warnings,
    }))
}

async fn handle_bootstrap_status(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let path: String = require_param(&params, "path")?;
    validate_path_string(&path)?;

    let _wt_info = find_worktree_by_path(ctx, &path).await?;

    let worktree_path = PathBuf::from(&path);
    match read_bootstrap_status(&worktree_path) {
        Some(status_file) => Ok(serde_json::json!({
            "path": path,
            "status": status_file.status,
            "steps": status_file.steps,
            "error": status_file.error,
        })),
        None => Ok(serde_json::json!({
            "path": path,
            "status": "completed",
            "steps": [],
            "error": null,
        })),
    }
}

async fn handle_create(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "worktree", "create")?;

    let branch: String = require_param(&params, "branch")?;
    validate_branch_name(&branch)?;

    let base_ref: Option<String> = optional_param_str(&params, "baseRef");
    let should_run_bootstrap: bool = optional_param_bool(&params, "runBootstrap", false);
    let idempotency_key: Option<String> = optional_param_str(&params, "idempotencyKey");

    let main_root = get_main_worktree_root(ctx).await?;

    if let Some(ref key) = idempotency_key {
        if let Some(existing) = check_idempotency_match(&main_root, key, &branch, &base_ref)? {
            return Ok(existing);
        }
    }

    let base_to_check = base_ref.as_deref().unwrap_or("HEAD");
    let base_exists = run_git(ctx, &["rev-parse", "--verify", base_to_check]).await;
    if base_exists.is_err() {
        return Err(ExtensionError::invalid_params(format!(
            "baseRef '{base_to_check}' does not exist"
        )));
    }

    let existing_branch = run_git(
        ctx,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .await;
    if existing_branch.is_ok() {
        let all_worktrees = fetch_all_worktrees(ctx).await?;
        let checked_out = all_worktrees
            .iter()
            .any(|wt| wt.branch.as_deref() == Some(&branch));
        if checked_out {
            return Err(ExtensionError::conflict(format!(
                "branch '{branch}' is already checked out in another worktree"
            )));
        }
    }

    let target_path = compute_worktree_target_path(&main_root, &branch);

    let mut args = vec!["worktree", "add"];
    let target_path_str = target_path.to_string_lossy().to_string();
    if let Some(ref br) = base_ref {
        args.push("-b");
        args.push(&branch);
        args.push(&target_path_str);
        args.push(br);
    } else {
        let existing_branch_check = run_git(
            ctx,
            &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
        )
        .await;
        if existing_branch_check.is_ok() {
            args.push(&target_path_str);
            args.push(&branch);
        } else {
            args.push("-b");
            args.push(&branch);
            args.push(&target_path_str);
        }
    }

    if let Err(e) = run_git(ctx, &args).await {
        if matches!(e.code, -32003) {
            return Err(ExtensionError::not_found("not a git repository"));
        }
        return Err(e);
    }

    register_branch(&main_root, &target_path_str, &branch);

    let bootstrap_status_info = if should_run_bootstrap {
        let config = read_bootstrap_config(&main_root)?;
        Some(run_bootstrap(&target_path, &config).await)
    } else {
        None
    };

    let all_worktrees = fetch_all_worktrees(ctx).await?;
    let new_wt = all_worktrees
        .into_iter()
        .find(|wt| wt.path == target_path_str)
        .ok_or_else(|| ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(
                "worktree created but not found in list".into(),
            )),
        })?;

    let mut result = serde_json::to_value(&new_wt).unwrap_or(Value::Null);
    if let Some(bs) = bootstrap_status_info {
        result["bootstrapStatus"] = serde_json::to_value(&bs).unwrap_or(Value::Null);
    }

    if let Some(ref key) = idempotency_key {
        save_idempotency(&main_root, key, result.clone(), &branch, &base_ref);
    }

    Ok(result)
}

async fn handle_delete(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "worktree", "delete")?;

    let path: String = require_param(&params, "path")?;
    validate_path_string(&path)?;
    let force: bool = optional_param_bool(&params, "force", false);

    let wt_info = find_worktree_by_path(ctx, &path).await?;

    if wt_info.is_main {
        return Err(ExtensionError::forbidden("cannot delete the main worktree"));
    }

    if !force && wt_info.is_dirty {
        return Err(ExtensionError::invalid_params(
            "worktree has uncommitted changes; use force=true to delete anyway",
        ));
    }

    let main_root = get_main_worktree_root(ctx).await?;

    let path_str = path.clone();
    let remove_args: Vec<&str> = if force {
        vec!["worktree", "remove", "--force", &path_str]
    } else {
        vec!["worktree", "remove", &path_str]
    };

    run_git(ctx, &remove_args).await.map_err(|e| {
        if matches!(e.code, -32003) {
            ExtensionError::not_found(format!("worktree not found: {path_str}"))
        } else {
            ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(format!(
                    "git worktree remove failed: {}",
                    e.data
                        .as_ref()
                        .and_then(|d| d.as_str())
                        .unwrap_or("unknown")
                ))),
            }
        }
    })?;

    let _ = run_git(ctx, &["worktree", "prune"]).await;

    let owned_branch = unregister_branch(&main_root, &path);
    let mut branch_cleaned = false;

    if let Some(ref branch) = owned_branch {
        let checked_elsewhere = {
            let remaining = fetch_all_worktrees(ctx).await;
            match remaining {
                Ok(wts) => wts.iter().any(|wt| wt.branch.as_deref() == Some(branch)),
                Err(_) => false,
            }
        };

        if !checked_elsewhere {
            let delete_result = run_git(ctx, &["branch", "-D", branch]).await;
            branch_cleaned = delete_result.is_ok();
        }
    }

    Ok(serde_json::json!({
        "path": path,
        "deleted": true,
        "branchCleaned": branch_cleaned,
    }))
}
