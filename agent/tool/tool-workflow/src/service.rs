//! Centralized business logic for the six workflow tools.
//!
//! Each `Workflow*Tool` struct in `tool_*.rs` is a thin `Tool` trait wrapper
//! that delegates here. This module owns all argument parsing, file I/O,
//! Luft engine interaction, and response formatting.

use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use luft_core::params;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};

use crate::backend::LoomAgentBackend;
use crate::common::{
    read_json_value, sanitize_instance_for_public, truncate_for_preview, validate_instance_dir_name,
};
use crate::event_bridge::luft_event_to_json;
use crate::runtime::WorkflowRuntime;
use crate::workflow_resolver::resolve_workflow;

// ── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_CONCURRENCY: usize = 4;

const DEFAULT_SOURCE_PREVIEW_LIMIT: usize = 8_192;

// ── Start ───────────────────────────────────────────────────────────────────

pub(crate) async fn start_workflow(
    runtime: &Arc<WorkflowRuntime>,
    args: Value,
    ctx: Option<&ToolCallContext>,
) -> Result<ToolCallContent, ToolSourceError> {
    let depth = ctx.map(|c| c.depth).unwrap_or(0);
    if depth >= 3 {
        tracing::warn!(
            target: "workflow::service",
            depth,
            "workflow nesting depth exceeded (max 3)",
        );
        return Err(ToolSourceError::ToolError(
            "Workflow nesting depth exceeded (max 3).".to_string(),
        ));
    }

    let resume_from_id = args
        .get("resume_from_id")
        .or_else(|| args.get("instance"))
        .or_else(|| args.get("instance_dir"))
        .and_then(|v| v.as_str());
    let script = args.get("script").and_then(|v| v.as_str());
    let workflow = args.get("workflow").and_then(|v| v.as_str());

    // Validate mutual exclusion: resume vs script/workflow
    if resume_from_id.is_some() && (script.is_some() || workflow.is_some()) {
        tracing::warn!(
            target: "workflow::service",
            "start_workflow rejected: resume_from_id mutually exclusive with script/workflow",
        );
        return Err(ToolSourceError::InvalidInput(
            "`resume_from_id` is mutually exclusive with `script` and `workflow`.".into(),
        ));
    }

    let (lua_source, display_name) = match (script, workflow, resume_from_id) {
        (_, _, Some(id)) => {
            validate_instance_dir_name(id)?;
            tracing::info!(
                target: "workflow::service",
                resume_from_id = %id,
                "starting workflow in resume mode",
            );
            (String::new(), format!("resumed:{id}"))
        }
        (Some(s), _, None) => {
            tracing::info!(
                target: "workflow::service",
                source_len = s.len(),
                "starting workflow from inline script",
            );
            (s.to_string(), "inline script".to_string())
        }
        (None, Some(w), None) => {
            let working_folder = runtime.working_folder();
            let path =
                resolve_workflow(w, &working_folder).map_err(ToolSourceError::InvalidInput)?;
            let source = std::fs::read_to_string(&path)
                .map_err(|e| ToolSourceError::ToolError(format!("Failed to read workflow: {e}")))?;
            let display_name = path
                .file_stem()
                .or_else(|| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "workflow".to_string());
            tracing::info!(
                target: "workflow::service",
                workflow = %w,
                path = %path.display(),
                display_name = %display_name,
                source_len = source.len(),
                "starting workflow from file",
            );
            (source, display_name)
        }
        (None, None, None) => {
            tracing::warn!(
                target: "workflow::service",
                "start_workflow rejected: no script, workflow, or resume_from_id provided",
            );
            return Err(ToolSourceError::InvalidInput(
                "Provide one of: `script`, `workflow`, or `resume_from_id`.".to_string(),
            ));
        }
    };

    let concurrency = params::parse_concurrency(&args)
        .map_err(ToolSourceError::InvalidInput)?
        .unwrap_or(DEFAULT_CONCURRENCY);
    let user_args = params::extract_user_args(&args);
    let lua_source = params::inject_args_globals(&lua_source, user_args.as_ref());

    let base_dir = runtime.instances_root();
    if let Err(e) = std::fs::create_dir_all(&base_dir) {
        tracing::error!(
            target: "workflow::service",
            base_dir = %base_dir.display(),
            error = %e,
            "failed to create instances directory",
        );
        return Err(ToolSourceError::ToolError(format!(
            "Failed to create instances directory: {e}"
        )));
    }

    let config = runtime.config_template.clone();
    let backend = LoomAgentBackend::new(config);

    tracing::debug!(
        target: "workflow::service",
        base_dir = %base_dir.display(),
        concurrency,
        has_user_args = user_args.is_some(),
        "building Luft engine",
    );
    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(&base_dir)
        .concurrency(concurrency)
        .build()
        .map_err(|e| {
            tracing::error!(
                target: "workflow::service",
                error = %e,
                "Luft engine build failed",
            );
            ToolSourceError::ToolError(format!("Workflow engine build failed: {e}"))
        })?;

    let run_handle = if let Some(id) = resume_from_id {
        luft.start_resume(id).await.map_err(|e| {
            tracing::error!(
                target: "workflow::service",
                resume_from_id = %id,
                error = %e,
                "failed to resume workflow",
            );
            ToolSourceError::ToolError(format!("Failed to resume workflow: {e}"))
        })?
    } else {
        luft.start_script(&lua_source).await.map_err(|e| {
            tracing::error!(
                target: "workflow::service",
                error = %e,
                "failed to start workflow",
            );
            ToolSourceError::ToolError(format!("Failed to start workflow: {e}"))
        })?
    };

    let run_dir_name = run_handle.run_dir_name().to_string();
    let is_inline_script = script.is_some();
    let workflow_arg_owned = workflow.map(|s| s.to_string());

    tracing::info!(
        target: "workflow::service",
        instance_dir = %run_dir_name,
        is_inline_script,
        "workflow started successfully",
    );

    let sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
    // Workflow cancel orchestration: the runtime owns an in-memory
    // registry of active runs (instance_dir -> CancellationToken) so
    // `workflow_cancel` (a tool that may be called by any other agent
    // on this same `WorkflowRuntime`) can signal completion. We register
    // the run *before* spawning the finalize task, then either
    // propagate the caller's cancellation into the runtime token, or
    // use the runtime token directly when there is no caller token.
    let runtime_cancel_token = runtime.register_run(run_dir_name.clone());
    let caller_cancel_token = ctx
        .and_then(|c| c.run_cancellation.as_ref())
        .map(|c| c.token());
    let cancel_token = match caller_cancel_token {
        Some(caller_token) => {
            // Mirror: when the caller fires, the runtime token also fires
            // (preserves the registry look-up path for `workflow_cancel`).
            let caller_owned = caller_token.clone();
            let mine = runtime_cancel_token.clone();
            tokio::spawn(async move {
                caller_owned.cancelled().await;
                mine.cancel();
            });
            caller_token
        }
        None => runtime_cancel_token.as_ref().clone(),
    };
    let rt = runtime.clone();
    let dir_for_cleanup = run_dir_name.clone();
    tokio::spawn(async move {
        background_finalize(
            rt.clone(),
            run_handle,
            sender,
            cancel_token,
            display_name,
            is_inline_script,
            workflow_arg_owned,
        )
        .await;
        rt.unregister_run(&dir_for_cleanup);
    });

    let mut payload = json!({
        "instance_dir": run_dir_name,
        "status": "running",
        "note": "Use workflow_status with `instance_dir` to follow progress.",
    });
    if let Some(id) = resume_from_id {
        payload["resumed_from"] = json!(id);
    }
    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    ))
}

async fn background_finalize(
    runtime: Arc<WorkflowRuntime>,
    run_handle: luft::RunHandle,
    sender: Option<Arc<dyn Fn(Value) + Send + Sync>>,
    cancel_token: CancellationToken,
    display_name: String,
    is_inline_script: bool,
    workflow_arg_owned: Option<String>,
) {
    let run_dir_name = run_handle.run_dir_name().to_string();

    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let cancel_token_for_spawn = cancel_token.clone();
    tokio::spawn(async move {
        cancel_token_for_spawn.cancelled().await;
        let _ = cancel_tx.send(());
    });

    let mut done_rx = run_handle.subscribe();

    let mut cancelled = false;
    tracing::debug!(
        target: "workflow::service",
        instance_dir = %run_dir_name,
        "background_finalize: waiting for workflow completion",
    );
    let cancel_handled = tokio::select! {
        _ = &mut cancel_rx => {
            tracing::info!(
                target: "workflow::service",
                instance_dir = %run_dir_name,
                "background_finalize: cancellation signalled",
            );
            run_handle.cancel();
            cancelled = true;
            true
        }
        _ = done_rx.recv() => {
            false
        }
    };

    let mut join_fut = std::pin::pin!(run_handle.join());

    #[allow(unused_assignments)]
    let mut final_status: Option<&'static str> = None;
    let mut final_report: Option<Value> = None;

    if !cancel_handled {
        loop {
            tokio::select! {
                ev = done_rx.recv() => {
                    match ev {
                        Ok(event) => {
                            if let Some(ref send) = sender {
                                send(luft_event_to_json(&event));
                            }
                            if let LuftAgentEvent::RunDone { report, status, .. } = event {
                                final_status = Some(match status {
                                    luft_core::contract::event::RunStatus::Completed => "completed",
                                    luft_core::contract::event::RunStatus::Failed => "failed",
                                    luft_core::contract::event::RunStatus::Cancelled => "cancelled",
                                    luft_core::contract::event::RunStatus::Partial => "completed",
                                });
                                final_report = Some(report);
                                break;
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => {
                            if final_status.is_none() {
                                final_status = Some(if cancelled { "cancelled" } else { "failed" });
                            }
                            break;
                        }
                    }
                }
                _ = cancel_token.cancelled(), if !cancelled => {
                    cancelled = true;
                }
                result = &mut join_fut => {
                    if final_status.is_none() {
                        final_status = Some(if cancelled { "cancelled" } else { "completed" });
                    }
                    if final_report.is_none() {
                        if let Ok(outcome) = &result {
                            if let Ok(report) = &outcome.result {
                                final_report = Some(report.clone());
                            }
                        }
                    }
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    let status = runtime.terminal_checkpoint_status(&run_dir_name).await;
                    if let Some(status) = status {
                        final_status = Some(status);
                        break;
                    }
                }
            }
        }
    }

    let final_status: &'static str = final_status.unwrap_or("unknown");

    tracing::info!(
        target: "workflow::service",
        instance_dir = %run_dir_name,
        final_status,
        cancelled,
        "background_finalize: workflow reached terminal state",
    );

    if let Err(e) = runtime
        .finalize(
            &run_dir_name,
            final_status,
            final_report.as_ref(),
            &display_name,
            is_inline_script,
            workflow_arg_owned.as_deref(),
        )
        .await
    {
        tracing::error!(
            target: "workflow::service",
            instance_dir = %run_dir_name,
            error = ?e,
            "finalize failed, writing minimal failed instance",
        );
        runtime.write_minimal_failed_instance(
            &run_dir_name,
            &display_name,
            is_inline_script,
            workflow_arg_owned.as_deref(),
            &format!("{e:?}"),
        );
    }
}

// ── Status ──────────────────────────────────────────────────────────────────

pub(crate) async fn read_status(
    runtime: &Arc<WorkflowRuntime>,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("'instance' is required for workflow_status.".into())
        })?;
    validate_instance_dir_name(dir)?;

    tracing::debug!(
        target: "workflow::service",
        instance = %dir,
        "read_status: resolving instance path",
    );

    let new_path = runtime.instances_root().join(dir);
    let legacy_path = runtime.runs_root().join(dir);

    let resolved = if new_path.is_dir() {
        new_path.clone()
    } else if legacy_path.is_dir() {
        legacy_path.clone()
    } else {
        return Err(ToolSourceError::InvalidInput(format!(
            "Instance '{dir}' not found"
        )));
    };

    let instance_json_path = resolved.join("instance.json");
    if instance_json_path.is_file() {
        let raw = std::fs::read_to_string(&instance_json_path).map_err(|e| {
            ToolSourceError::ToolError(format!("Failed to read instance summary: {e}"))
        })?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|e| ToolSourceError::ToolError(format!("Invalid instance summary: {e}")))?;
        let sanitized = sanitize_instance_for_public(value);
        return Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&sanitized).unwrap_or_default(),
        ));
    }

    if resolved == new_path {
        return Ok(running_status(dir));
    }

    Err(ToolSourceError::ToolError(format!(
        "Instance '{dir}' is incomplete (missing instance summary)"
    )))
}

/// Build a running-state response with the same InstanceMeta shape as terminal state.
fn running_status(dir: &str) -> ToolCallContent {
    let payload = json!({
        "schema_version": crate::instance::SCHEMA_VERSION,
        "instance_id": dir,
        "instance_dir": dir,
        "workflow": { "kind": "file", "name": dir },
        "status": "running",
        "created_at": 0,
        "completed_at": 0,
        "total_tokens": 0,
        "total_elapsed_ms": 0,
        "agent_count": 0,
        "agents": [],
        "phase_spans": [],
        "event_stats": { "total": 0, "by_type": {} },
        "report": {},
    });
    let sanitized = sanitize_instance_for_public(payload);
    ToolCallContent::Text(serde_json::to_string_pretty(&sanitized).unwrap_or_default())
}

// ── Cancel ──────────────────────────────────────────────────────────────────

/// Look up an active run and signal its cancellation token.
///
/// The runtime owns an in-memory registry mapping `instance_dir` to the
/// `Arc<CancellationToken>` passed into `background_finalize`. If the run
/// is still active, the cancel token is fired (and `run_handle.cancel()`
/// is invoked inside the finalize loop); if not, we report `not_found_or_terminal`.
///
/// The "force" parameter is accepted by the schema but reserved for future use
/// (e.g. forceful kill of the agent task). For now all cancels are graceful —
/// the agent returns a `Cancelled` error after the in-flight LLM call
/// completes (or its turn is interrupted) and the checkpoint is marked
/// "cancelled" instead of "completed".
pub(crate) fn cancel_workflow(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance")
        .or_else(|| args.get("instance_dir"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput(
                "'instance' (or 'instance_dir') is required for workflow_cancel.".into(),
            )
        })?
        .to_string();
    validate_instance_dir_name(&dir)?;

    tracing::info!(
        target: "workflow::service",
        instance = %dir,
        "cancel_workflow: attempting cancellation",
    );

    // Determine an outcome tag. If the runtime registry knows about the
    // instance, cancellation is in-flight (`cancelling`); the runtime's
    // finalize loop will mark the checkpoint as `cancelled` once the
    // agent returns Cancelled. If the registry does not know about it, the
    // run is unknown / terminal — we surface that so the LLM can react.
    let result = if runtime.cancel_run(&dir) {
        tracing::info!(
            target: "workflow::service",
            instance = %dir,
            "cancel_workflow: cancellation signalled",
        );
        json!({
            "instance_dir": dir,
            "result": "cancelling",
            "note": "Cancellation has been signalled; poll workflow_status to observe the terminal state.",
        })
    } else {
        tracing::debug!(
            target: "workflow::service",
            instance = %dir,
            "cancel_workflow: instance not found or already terminal",
        );
        json!({
            "instance_dir": dir,
            "result": "not_found_or_terminal",
            "note": "No active workflow with this identifier — it may already be in a terminal state (completed/failed/cancelled), or the run belongs to a different process.",
        })
    };
    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&result).unwrap_or_default(),
    ))
}

// ── List ────────────────────────────────────────────────────────────────────

pub(crate) fn list_instances(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let limit = params::parse_list_limit(args).map_err(ToolSourceError::InvalidInput)? as usize;
    let cursor = params::parse_cursor(args);
    let status_filter = params::parse_status_filter(args).map_err(ToolSourceError::InvalidInput)?;

    tracing::debug!(
        target: "workflow::service",
        limit,
        has_cursor = cursor.is_some(),
        status_filter = ?status_filter,
        "list_instances: scanning instance directories",
    );

    let mut entries: Vec<Value> = Vec::new();
    collect_instances_under(&runtime.instances_root(), &mut entries);
    collect_instances_under(&runtime.runs_root(), &mut entries);

    if let Some(ref sf) = status_filter {
        let want = sf.to_lowercase();
        entries.retain(|e| {
            e.get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase() == want)
                .unwrap_or(false)
        });
    }

    entries.sort_by(|a, b| {
        let ca = a.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
        let cb = b.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
        cb.cmp(&ca).then_with(|| {
            let da = a.get("instance_dir").and_then(|v| v.as_str()).unwrap_or("");
            let db = b.get("instance_dir").and_then(|v| v.as_str()).unwrap_or("");
            db.cmp(da)
        })
    });

    let total_after_filter = entries.len();
    let start_idx = match cursor.as_ref() {
        None => 0,
        Some(c) => {
            let pos = entries.iter().position(|e| {
                e.get("instance_dir")
                    .and_then(|v| v.as_str())
                    .map(|s| s == c)
                    .unwrap_or(false)
            });
            match pos {
                None => {
                    return Err(ToolSourceError::ToolError(format!("cursor not found: {c}")));
                }
                Some(p) => p + 1,
            }
        }
    };

    let page: Vec<Value> = entries
        .iter()
        .skip(start_idx)
        .take(limit)
        .cloned()
        .collect();

    let next_cursor = if page.is_empty() {
        None
    } else if start_idx + page.len() < total_after_filter {
        page.last()
            .and_then(|v| v.get("instance_dir").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };
    let has_more = next_cursor.is_some();

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instances": page,
            "count": page.len(),
            "next_cursor": next_cursor,
            "has_more": has_more,
        }))
        .unwrap_or_default(),
    ))
}

fn build_entry_from_instance_json(v: &Value, dir_name: &str) -> Value {
    let wf = v.get("workflow");
    let kind = wf
        .and_then(|w| w.get("kind"))
        .and_then(|x| x.as_str())
        .unwrap_or("file")
        .to_string();
    let name = wf
        .and_then(|w| w.get("name"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    json!({
        "instance_id": v.get("instance_id").and_then(|x| x.as_str()).unwrap_or("?"),
        "instance_dir": dir_name,
        "status": v.get("status").and_then(|x| x.as_str()).unwrap_or("unknown"),
        "workflow": {
            "kind": kind,
            "name": name,
        },
        "created_at": v.get("created_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "completed_at": v.get("completed_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "total_tokens": v.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        "agent_count": v.get("agent_count").and_then(|x| x.as_u64()).unwrap_or(0),
    })
}

fn collect_instances_under(root: &Path, out: &mut Vec<Value>) {
    if !root.exists() {
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let inst_path = path.join("instance.json");
        if let Some(inst) = read_json_value(&inst_path) {
            out.push(build_entry_from_instance_json(&inst, &dir_name));
        }
    }
}

// ── Events ──────────────────────────────────────────────────────────────────

pub(crate) fn read_events(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("'instance' is required for workflow_events.".into())
        })?;
    validate_instance_dir_name(dir)?;
    let filter = params::EventsFilter::from_args(args);

    tracing::debug!(
        target: "workflow::service",
        instance = %dir,
        has_type_filter = filter.types.as_ref().is_some_and(|t| !t.is_empty()),
        offset = filter.offset,
        events_limit = filter.events_limit,
        "read_events: reading event stream",
    );

    let base_dir = runtime.instances_root();
    let db_exists = base_dir.join("luft.db").exists();
    let owned_dir = dir.to_string();
    let mut all_events: Vec<Value> = if db_exists {
        std::thread::scope(|s| {
            let h = s.spawn(move || {
                luft::query::get_events(&owned_dir, &base_dir)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|e| serde_json::to_value(e).ok())
                    .collect::<Vec<_>>()
            });
            h.join().unwrap_or_default()
        })
    } else {
        Vec::new()
    };

    if all_events.is_empty() {
        if let Some(path) = runtime.resolve_instance_path(dir) {
            if let Ok(raw) = std::fs::read_to_string(path.join("events.jsonl")) {
                all_events = raw
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect();
            }
        }
    }

    let mut filtered_count: u64 = 0;
    let mut returned: usize = 0;
    let mut events: Vec<Value> = Vec::new();
    for val in &all_events {
        if !filter.matches(val) {
            continue;
        }
        filtered_count += 1;
        if filtered_count > filter.offset && (returned as u64) < filter.events_limit {
            events.push(val.clone());
            returned += 1;
        }
    }

    let next_offset = if filter.offset + (returned as u64) < filtered_count {
        Some(filter.offset + returned as u64)
    } else {
        None
    };

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instance_dir": dir,
            "offset": filter.offset,
            "events_limit": filter.events_limit,
            "total_matching": filtered_count,
            "next_offset": next_offset,
            "events": events,
        }))
        .unwrap_or_default(),
    ))
}

// ── Source ──────────────────────────────────────────────────────────────────

pub(crate) fn read_source(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("'instance' is required for workflow_source.".into())
        })?;
    validate_instance_dir_name(dir)?;
    let resolved = runtime
        .resolve_instance_path(dir)
        .ok_or_else(|| ToolSourceError::InvalidInput(format!("Instance '{dir}' not found")))?;

    tracing::debug!(
        target: "workflow::service",
        instance = %dir,
        resolved_path = %resolved.display(),
        "read_source: reading workflow.lua",
    );

    let source = std::fs::read_to_string(resolved.join("workflow.lua"))
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to read workflow source: {e}")))?;

    let (preview, truncated) = if source.len() > DEFAULT_SOURCE_PREVIEW_LIMIT {
        (
            truncate_for_preview(&source, DEFAULT_SOURCE_PREVIEW_LIMIT),
            true,
        )
    } else {
        (source.clone(), false)
    };

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instance_dir": dir,
            "workflow_source": preview,
            "truncated": truncated,
        }))
        .unwrap_or_default(),
    ))
}

// ── Files ───────────────────────────────────────────────────────────────────

pub(crate) fn list_workflow_files(
    runtime: &WorkflowRuntime,
) -> Result<ToolCallContent, ToolSourceError> {
    let root = runtime.workflows_dir();
    tracing::debug!(
        target: "workflow::service",
        workflows_dir = %root.display(),
        "list_workflow_files: scanning for .lua files",
    );
    let mut workflows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|v| v.to_str()) != Some("lua") {
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            workflows.push(json!({ "name": name }));
        }
    }
    workflows.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .cmp(&right.get("name").and_then(Value::as_str))
    });

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "workflows": workflows,
            "count": workflows.len(),
        }))
        .unwrap_or_default(),
    ))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn runtime_with(folder: &Path) -> Arc<WorkflowRuntime> {
        let cfg = AgentConfig {
            working_folder: Some(folder.to_path_buf()),
            ..Default::default()
        };
        Arc::new(WorkflowRuntime::new(cfg))
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(f)
    }

    #[test]
    fn list_entry_from_instance_json_preserves_workflow() {
        let inst = json!({
            "instance_id": "r2",
            "status": "failed",
            "workflow": {"kind": "inline", "name": "demo"},
            "created_at": 5,
            "completed_at": 7,
            "total_tokens": 9,
            "agent_count": 2,
        });
        let e = build_entry_from_instance_json(&inst, "loom-instance_r2");
        assert_eq!(e["workflow"]["kind"], "inline");
        assert_eq!(e["workflow"]["name"], "demo");
        assert_eq!(e["agent_count"], 2);
    }

    #[test]
    fn collect_instances_under_finds_instance_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root: PathBuf = tmp.path().join(".loom").join("instances");
        let dir = root.join("done");
        std::fs::create_dir_all(&dir).unwrap();
        let inst = json!({"instance_id": "rd", "status": "completed", "instance_dir": "done"});
        std::fs::write(
            dir.join("instance.json"),
            serde_json::to_string_pretty(&inst).unwrap(),
        )
        .unwrap();
        let mut out = Vec::new();
        collect_instances_under(&root, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["instance_dir"], "done");
    }

    #[test]
    fn collect_instances_under_handles_missing_root() {
        let mut out = Vec::new();
        collect_instances_under(std::path::Path::new("/nonexistent/7c1a9f"), &mut out);
        assert!(out.is_empty());
    }

    // -- events helpers tests moved to luft_service::params --

    // -- status handler --

    #[test]
    fn status_returns_sanitized_view_when_instance_json_present() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_x";
        let dir_path = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();
        let raw = json!({
            "schema_version": 1,
            "instance_id": "run-x",
            "instance_dir": instance_dir,
            "workflow": {"kind": "file", "name": "wf", "path": "/secret/abs"},
            "agents": [
                {"agent_id": "a", "output_ref": "agent-outputs/a.txt", "output_size": 4096}
            ],
            "report": {"ref": "report.json", "preview": "hi", "value_type": "object", "size_bytes": 5},
            "checkpoint_hash": "deadbeef",
            "status": "completed",
        });
        std::fs::write(
            dir_path.join("instance.json"),
            serde_json::to_string_pretty(&raw).unwrap(),
        )
        .unwrap();

        let rt = runtime_with(tmp.path());
        let result = block_on(read_status(&rt, &json!({"instance": instance_dir}))).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v["workflow"].get("path").is_none());
        assert!(v["agents"][0].get("output_ref").is_none());
        assert!(v["report"].get("ref").is_none());
        assert!(v.get("checkpoint_hash").is_none());
        assert_eq!(v["status"], "completed");
    }

    #[test]
    fn status_returns_running_when_only_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_running";
        let dir_path = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();

        let rt = runtime_with(tmp.path());
        let result = block_on(read_status(&rt, &json!({"instance": instance_dir}))).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["status"], "running");
        assert_eq!(v["instance_dir"], instance_dir);
        assert_eq!(v["agent_count"], 0);
        assert!(v["agents"].is_array());
        assert!(v["phase_spans"].is_array());
        assert!(v["report"].is_object());
    }

    #[test]
    fn status_errors_on_legacy_dir_without_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "legacy-blank";
        let dir_path = tmp.path().join(".luft").join("runs").join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();

        let rt = runtime_with(tmp.path());
        let err = block_on(read_status(&rt, &json!({"instance": instance_dir}))).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("corrupt") || msg.contains("incomplete"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn status_errors_on_unknown_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let err = block_on(read_status(&rt, &json!({"instance": "does-not-exist"}))).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found"));
    }

    #[test]
    fn status_errors_on_missing_instance_param() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let err = block_on(read_status(&rt, &json!({}))).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("'instance' is required"));
    }

    // -- source handler --

    #[test]
    fn source_returns_short_workflow_preview_untruncated() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_short";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("workflow.lua"), "function main() end").unwrap();

        let rt = runtime_with(tmp.path());
        let result = read_source(&rt, &json!({"instance": instance_dir})).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["workflow_source"], "function main() end");
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn source_truncates_long_workflow_to_preview_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_long";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = "x".repeat(DEFAULT_SOURCE_PREVIEW_LIMIT + 40);
        std::fs::write(dir.join("workflow.lua"), &big).unwrap();

        let rt = runtime_with(tmp.path());
        let result = read_source(&rt, &json!({"instance": instance_dir})).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let preview = v["workflow_source"].as_str().unwrap();
        assert!(preview.ends_with('…'));
        assert_eq!(v["truncated"], true);
        assert!(preview.len() <= DEFAULT_SOURCE_PREVIEW_LIMIT + '…'.len_utf8());
    }

    #[test]
    fn source_errors_when_missing_workflow_lua() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_empty";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();

        let rt = runtime_with(tmp.path());
        let err = read_source(&rt, &json!({"instance": instance_dir})).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("Failed to read workflow source"));
    }

    #[test]
    fn source_errors_on_unknown_instance_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let err = read_source(&rt, &json!({"instance": "no-such-thing"})).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found"));
    }

    // -- files handler --

    #[test]
    fn files_lists_lua_definitions() {
        let tmp = tempfile::tempdir().unwrap();
        let wf_dir = tmp.path().join(".loom").join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(wf_dir.join("alpha.lua"), "-- alpha\nfunction main() end\n").unwrap();
        std::fs::write(
            wf_dir.join("beta.lua"),
            "\n\n-- beta\nfunction main() end\n",
        )
        .unwrap();
        let ignored = wf_dir.join("ignore.txt");
        std::fs::write(ignored, "nope").unwrap();

        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let list = v["workflows"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["name"], "alpha.lua");
        assert_eq!(list[1]["name"], "beta.lua");
    }

    #[test]
    fn files_returns_empty_when_no_workflows_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 0);
        assert!(v["workflows"].is_array());
    }

    #[test]
    fn files_excludes_non_lua_files() {
        let tmp = tempfile::tempdir().unwrap();
        let wf_dir = tmp.path().join(".loom").join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(wf_dir.join("readme.md"), "# docs").unwrap();
        std::fs::write(wf_dir.join("greet.lua"), "-- greet\nfunction main() end").unwrap();

        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let list = v["workflows"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "greet.lua");
    }
}
