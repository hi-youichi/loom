//! Centralized business logic for the six workflow tools.
//!
//! Each `Workflow*Tool` struct in `tool_*.rs` is a thin `Tool` trait wrapper
//! that delegates here. This module owns all argument parsing, file I/O,
//! Luft engine interaction, and response formatting.

use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::IntoFuture;
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
const MAX_CONCURRENCY: usize = 64;

const DEFAULT_EVENTS_LIMIT: u64 = 50;
const MAX_EVENTS_LIMIT: u64 = 500;

const DEFAULT_SOURCE_PREVIEW_LIMIT: usize = 8_192;
const DEFAULT_LIST_INSTANCES_LIMIT: usize = 20;
const MAX_LIST_INSTANCES_LIMIT: usize = 100;
const LIST_INSTANCES_STATUS_FILTERS: &[&str] = &["completed", "failed", "cancelled"];

// ── Start ───────────────────────────────────────────────────────────────────

pub(crate) async fn start_workflow(
    runtime: &Arc<WorkflowRuntime>,
    args: Value,
    ctx: Option<&ToolCallContext>,
) -> Result<ToolCallContent, ToolSourceError> {
    let depth = ctx.map(|c| c.depth).unwrap_or(0);
    if depth >= 3 {
        return Err(ToolSourceError::ToolError(
            "Workflow nesting depth exceeded (max 3).".to_string(),
        ));
    }

    let script = args.get("script").and_then(|v| v.as_str());
    let workflow = args.get("workflow").and_then(|v| v.as_str());

    let (lua_source, display_name) = match (script, workflow) {
        (Some(s), _) => (s.to_string(), "inline script".to_string()),
        (None, Some(w)) => {
            let working_folder = runtime.working_folder();
            let path =
                resolve_workflow(w, &working_folder).map_err(ToolSourceError::InvalidInput)?;
            let source = std::fs::read_to_string(&path).map_err(|e| {
                ToolSourceError::ToolError(format!("Failed to read workflow: {e}"))
            })?;
            let display_name = path
                .file_stem()
                .or_else(|| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "workflow".to_string());
            (source, display_name)
        }
        (None, None) => {
            return Err(ToolSourceError::InvalidInput(
                "Either 'script' or 'workflow' must be provided.".to_string(),
            ));
        }
    };

    let concurrency = parse_concurrency(&args)?;
    let user_args = extract_user_args(&args);
    let lua_source = inject_args_globals(&lua_source, user_args.as_ref());

    let base_dir = runtime.instances_root();
    if let Err(e) = std::fs::create_dir_all(&base_dir) {
        return Err(ToolSourceError::ToolError(format!(
            "Failed to create instances directory: {e}"
        )));
    }

    let config = runtime.config_template.clone();
    let backend = LoomAgentBackend::new(config);

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(&base_dir)
        .concurrency(concurrency)
        .build()
        .map_err(|e| {
            ToolSourceError::ToolError(format!("Workflow engine build failed: {e}"))
        })?;

    let run_handle = luft
        .start_script(&lua_source)
        .await
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to start workflow: {e}")))?;

    let run_dir_name = run_handle.run_dir_name().to_string();
    let is_inline_script = script.is_some();
    let workflow_arg_owned = workflow.map(|s| s.to_string());

    let sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
    let cancel_token = ctx
        .and_then(|c| c.run_cancellation.as_ref())
        .map(|c| c.token())
        .unwrap_or_default();
    let rt = runtime.clone();
    tokio::spawn(async move {
        background_finalize(
            rt,
            run_handle,
            sender,
            cancel_token,
            display_name,
            is_inline_script,
            workflow_arg_owned,
        )
        .await;
    });

    let payload = json!({
        "instance_dir": run_dir_name,
        "status": "running",
        "note": "Use workflow_status with `instance_dir` to follow progress.",
    });
    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    ))
}

// ── Resume ──────────────────────────────────────────────────────────────────

pub(crate) async fn resume_workflow(
    runtime: &Arc<WorkflowRuntime>,
    args: Value,
    ctx: Option<&ToolCallContext>,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance_dir")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("instance").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("'instance_dir' is required.".into())
        })?
        .to_string();
    validate_instance_dir_name(&dir)?;

    let base_dir = runtime.instances_root();
    let config = runtime.config_template.clone();
    let backend = LoomAgentBackend::new(config);

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(&base_dir)
        .concurrency(DEFAULT_CONCURRENCY)
        .build()
        .map_err(|e| {
            ToolSourceError::ToolError(format!("Workflow engine build failed: {e}"))
        })?;

    let run_handle = luft
        .start_resume(&dir)
        .await
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to resume workflow: {e}")))?;

    let run_dir_name = run_handle.run_dir_name().to_string();
    let sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
    let cancel_token = ctx
        .and_then(|c| c.run_cancellation.as_ref())
        .map(|c| c.token())
        .unwrap_or_default();
    let rt = runtime.clone();
    let dir_clone = dir.clone();
    tokio::spawn(async move {
        background_finalize(
            rt,
            run_handle,
            sender,
            cancel_token,
            dir_clone,
            false,
            None,
        )
        .await;
    });

    let payload = json!({
        "instance_dir": run_dir_name,
        "resumed_from": &dir,
        "status": "running",
        "note": "Use workflow_status with `instance_dir` to follow progress.",
    });
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
    let mut done_rx = run_handle.subscribe();

    #[allow(unused_assignments)]
    let mut final_status: Option<&'static str> = None;
    let mut final_report: Option<Value> = None;
    let mut cancelled = false;

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
                run_handle.cancel();
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
    let final_status: &'static str = final_status.unwrap_or("unknown");

    match run_handle.into_future().await {
        Ok(outcome) => {
            if final_report.is_none() {
                if let Ok(report) = outcome.result {
                    final_report = Some(report);
                }
            }
        }
        Err(e) => {
            eprintln!("warning: workflow task drain failed: {e}");
        }
    }

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
        runtime.write_minimal_failed_instance(
            &run_dir_name,
            &display_name,
            is_inline_script,
            workflow_arg_owned.as_deref(),
            &format!("{e:?}"),
        );
    }
}

fn parse_concurrency(args: &Value) -> Result<usize, ToolSourceError> {
    let Some(v) = args.get("concurrency") else {
        return Ok(DEFAULT_CONCURRENCY);
    };
    let n = v.as_u64().ok_or_else(|| {
        ToolSourceError::InvalidInput(format!("'concurrency' must be a positive integer, got {v}"))
    })?;
    if !(1..=MAX_CONCURRENCY as u64).contains(&n) {
        return Err(ToolSourceError::InvalidInput(format!(
            "'concurrency' must be between 1 and {MAX_CONCURRENCY}, got {n}"
        )));
    }
    Ok(n as usize)
}

fn extract_user_args(args: &Value) -> Option<Value> {
    let v = args.get("args")?;
    if v.is_null() {
        return None;
    }
    Some(v.clone())
}

fn inject_args_globals(lua_source: &str, user_args: Option<&Value>) -> String {
    let Some(args) = user_args else {
        return lua_source.to_string();
    };
    let lua_expr = crate::json_to_lua::json_to_lua(args);
    format!("_G._args = {lua_expr}\n{lua_source}")
}

// ── Status ──────────────────────────────────────────────────────────────────

pub(crate) async fn read_status(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = args
        .get("instance")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("'instance' is required for workflow_status.".into())
        })?;
    validate_instance_dir_name(dir)?;

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
        let value: Value = serde_json::from_str(&raw).map_err(|e| {
            ToolSourceError::ToolError(format!("Invalid instance summary: {e}"))
        })?;
        let sanitized = sanitize_instance_for_public(value);
        return Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&sanitized).unwrap_or_default(),
        ));
    }

    let checkpoint_path = resolved.join("checkpoint.json");

    if checkpoint_path.is_file() {
        return runtime.rebuild_summary(dir, &checkpoint_path).await;
    }

    if resolved == new_path {
        return Ok(running_status(dir, &checkpoint_path));
    }

    Err(ToolSourceError::ToolError(format!(
        "Instance '{dir}' is incomplete (missing checkpoint)"
    )))
}

/// Build a running-state response with the same InstanceMeta shape as terminal state.
fn running_status(dir: &str, checkpoint_path: &Path) -> ToolCallContent {
    let ckpt = read_json_value(checkpoint_path);
    let instance_id = ckpt
        .as_ref()
        .and_then(|c| c.get("run_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(dir);
    let created_at = ckpt
        .as_ref()
        .and_then(|c| c.get("created_at"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let payload = json!({
        "schema_version": crate::instance::SCHEMA_VERSION,
        "instance_id": instance_id,
        "instance_dir": dir,
        "workflow": { "kind": "file", "name": dir },
        "status": "running",
        "created_at": created_at,
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

// ── List ────────────────────────────────────────────────────────────────────

pub(crate) fn list_instances(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let limit = parse_list_instances_limit(args)?;
    let cursor = parse_list_instances_cursor(args);
    let status_filter = parse_list_instances_status_filter(args)?;

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

fn parse_list_instances_limit(args: &Value) -> Result<usize, ToolSourceError> {
    let Some(v) = args.get("limit") else {
        return Ok(DEFAULT_LIST_INSTANCES_LIMIT);
    };
    if v.is_null() {
        return Ok(DEFAULT_LIST_INSTANCES_LIMIT);
    }
    let n = v.as_u64().ok_or_else(|| {
        ToolSourceError::InvalidInput(format!("'limit' must be a positive integer, got {v}"))
    })?;
    if !(1..=MAX_LIST_INSTANCES_LIMIT as u64).contains(&n) {
        return Err(ToolSourceError::InvalidInput(format!(
            "'limit' must be between 1 and {MAX_LIST_INSTANCES_LIMIT}, got {n}"
        )));
    }
    Ok(n as usize)
}

fn parse_list_instances_cursor(args: &Value) -> Option<String> {
    let v = args.get("cursor")?;
    if v.is_null() {
        return None;
    }
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
}

fn parse_list_instances_status_filter(args: &Value) -> Result<Option<String>, ToolSourceError> {
    let v = match args.get("status_filter") {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    let s = v.as_str().ok_or_else(|| {
        ToolSourceError::InvalidInput(format!("'status_filter' must be a string, got {v}"))
    })?;
    let lower = s.to_lowercase();
    if !LIST_INSTANCES_STATUS_FILTERS.contains(&lower.as_str()) {
        return Err(ToolSourceError::InvalidInput(format!(
            "'status_filter' must be one of completed|failed|cancelled, got {s}"
        )));
    }
    Ok(Some(lower))
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

fn build_entry_from_checkpoint(ckpt: &Value, dir_name: &str) -> Option<Value> {
    let status = ckpt
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let is_terminal = matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "failed" | "cancelled"
    );
    if !is_terminal {
        return None;
    }
    let agent_count = ckpt
        .get("agent_results")
        .and_then(|x| x.as_object())
        .map(|o| o.len() as u64)
        .unwrap_or(0);
    Some(json!({
        "instance_id": ckpt.get("run_id").and_then(|x| x.as_str()).unwrap_or("?"),
        "instance_dir": dir_name,
        "status": status,
        "workflow": {
            "kind": "file",
            "name": dir_name,
        },
        "created_at": ckpt.get("created_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "completed_at": ckpt.get("updated_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "total_tokens": ckpt.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        "agent_count": agent_count,
    }))
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
            continue;
        }

        let ckpt_path = path.join("checkpoint.json");
        if let Some(ckpt) = read_json_value(&ckpt_path) {
            if let Some(entry) = build_entry_from_checkpoint(&ckpt, &dir_name) {
                out.push(entry);
            }
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
    let path = runtime
        .resolve_instance_path(dir)
        .ok_or_else(|| ToolSourceError::InvalidInput(format!("Instance '{dir}' not found")))?;
    let events_path = path.join("events.jsonl");
    let offset = parse_events_offset(args);
    let events_limit = parse_events_limit(args);
    let types = parse_events_types(args);
    let agent_id = parse_events_agent_id(args);

    let mut filtered_count: u64 = 0;
    let mut returned: usize = 0;
    let mut events: Vec<Value> = Vec::new();

    if let Ok(file) = std::fs::File::open(&events_path) {
        let types_set: Option<HashSet<&str>> = types
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());

        let reader = std::io::BufReader::new(file);
        use std::io::BufRead as _;
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let val: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(set) = &types_set {
                if !event_matches_types(&val, set) {
                    continue;
                }
            }
            if let Some(aid) = &agent_id {
                if !event_matches_agent_id(&val, aid) {
                    continue;
                }
            }

            filtered_count += 1;
            if filtered_count > offset && (returned as u64) < events_limit {
                events.push(val);
                returned += 1;
            }
        }
    }

    let next_offset = if offset + (returned as u64) < filtered_count {
        Some(offset + returned as u64)
    } else {
        None
    };

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instance_dir": dir,
            "offset": offset,
            "events_limit": events_limit,
            "total_matching": filtered_count,
            "next_offset": next_offset,
            "events": events,
        }))
        .unwrap_or_default(),
    ))
}

fn parse_events_offset(args: &Value) -> u64 {
    args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0)
}

fn parse_events_limit(args: &Value) -> u64 {
    let raw = args
        .get("events_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_EVENTS_LIMIT);
    raw.clamp(1, MAX_EVENTS_LIMIT)
}

fn parse_events_types(args: &Value) -> Option<Vec<String>> {
    let v = args.get("types")?;
    if v.is_null() {
        return None;
    }
    let arr = v.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|t| t.as_str().map(|s| s.to_string()))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn parse_events_agent_id(args: &Value) -> Option<String> {
    let v = args.get("agent_id")?;
    if v.is_null() {
        return None;
    }
    v.as_str().map(|s| s.to_string())
}

fn event_matches_types(event: &Value, types: &HashSet<&str>) -> bool {
    event
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| types.contains(t))
        .unwrap_or(false)
}

fn event_matches_agent_id(event: &Value, agent_id: &str) -> bool {
    event
        .get("agent_id")
        .and_then(|a| a.as_str())
        .map(|s| s == agent_id)
        .unwrap_or(false)
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

    let source = std::fs::read_to_string(resolved.join("workflow.lua")).map_err(|e| {
        ToolSourceError::ToolError(format!("Failed to read workflow source: {e}"))
    })?;

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
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(f)
    }

    // -- start helpers --

    #[test]
    fn concurrency_default_when_missing() {
        let args = json!({});
        assert_eq!(parse_concurrency(&args).unwrap(), DEFAULT_CONCURRENCY);
    }

    #[test]
    fn concurrency_explicit_value() {
        assert_eq!(parse_concurrency(&json!({"concurrency": 8})).unwrap(), 8);
    }

    #[test]
    fn concurrency_at_bounds() {
        assert_eq!(parse_concurrency(&json!({"concurrency": 1})).unwrap(), 1);
        assert_eq!(parse_concurrency(&json!({"concurrency": 64})).unwrap(), 64);
    }

    #[test]
    fn concurrency_rejects_zero() {
        assert!(parse_concurrency(&json!({"concurrency": 0})).is_err());
    }

    #[test]
    fn concurrency_rejects_over_max() {
        assert!(parse_concurrency(&json!({"concurrency": 65})).is_err());
    }

    #[test]
    fn concurrency_rejects_non_integer() {
        assert!(parse_concurrency(&json!({"concurrency": "fast"})).is_err());
        assert!(parse_concurrency(&json!({"concurrency": 4.5})).is_err());
        assert!(parse_concurrency(&json!({"concurrency": -1})).is_err());
    }

    #[test]
    fn extract_user_args_missing() {
        let args = json!({});
        assert!(extract_user_args(&args).is_none());
    }

    #[test]
    fn extract_user_args_null_treated_as_missing() {
        let args = json!({"args": null});
        assert!(extract_user_args(&args).is_none());
    }

    #[test]
    fn extract_user_args_object() {
        let args = json!({"args": {"topic": "rust", "n": 5}});
        let v = extract_user_args(&args).unwrap();
        assert_eq!(v["topic"], "rust");
        assert_eq!(v["n"], 5);
    }

    #[test]
    fn inject_no_args_returns_source_unchanged() {
        let src = "function main() report({ok=true}) end";
        assert_eq!(inject_args_globals(src, None), src);
    }

    #[test]
    fn inject_prepends_global_assignment() {
        let src = "function main() report({ok=true}) end";
        let args = json!({"topic": "rust"});
        let out = inject_args_globals(src, Some(&args));
        assert!(out.starts_with("_G._args = {topic = \"rust\"}\n"));
        assert!(out.ends_with(src));
    }

    // -- list helpers --

    #[test]
    fn list_limit_default_when_missing() {
        assert_eq!(parse_list_instances_limit(&json!({})).unwrap(), 20);
    }

    #[test]
    fn list_limit_explicit_value() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": 50})).unwrap(), 50);
    }

    #[test]
    fn list_limit_at_bounds() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": 1})).unwrap(), 1);
        assert_eq!(parse_list_instances_limit(&json!({"limit": 100})).unwrap(), 100);
    }

    #[test]
    fn list_limit_rejects_zero() {
        assert!(parse_list_instances_limit(&json!({"limit": 0})).is_err());
    }

    #[test]
    fn list_limit_rejects_over_max() {
        assert!(parse_list_instances_limit(&json!({"limit": 101})).is_err());
    }

    #[test]
    fn list_limit_rejects_non_integer() {
        assert!(parse_list_instances_limit(&json!({"limit": "fast"})).is_err());
        assert!(parse_list_instances_limit(&json!({"limit": 4.5})).is_err());
        assert!(parse_list_instances_limit(&json!({"limit": -1})).is_err());
    }

    #[test]
    fn list_limit_null_treated_as_default() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": null})).unwrap(), 20);
    }

    #[test]
    fn list_cursor_missing_returns_none() {
        assert!(parse_list_instances_cursor(&json!({})).is_none());
    }

    #[test]
    fn list_cursor_null_returns_none() {
        assert!(parse_list_instances_cursor(&json!({"cursor": null})).is_none());
    }

    #[test]
    fn list_cursor_empty_string_returns_none() {
        assert!(parse_list_instances_cursor(&json!({"cursor": ""})).is_none());
    }

    #[test]
    fn list_cursor_nonempty_returns_some() {
        assert_eq!(
            parse_list_instances_cursor(&json!({"cursor": "abc"})).unwrap(),
            "abc"
        );
    }

    #[test]
    fn list_status_filter_missing_returns_none() {
        assert!(parse_list_instances_status_filter(&json!({})).unwrap().is_none());
    }

    #[test]
    fn list_status_filter_null_returns_none() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": null})).unwrap().is_none());
    }

    #[test]
    fn list_status_filter_terminal_lowercased() {
        let f = parse_list_instances_status_filter(&json!({"status_filter": "FAILED"}))
            .unwrap()
            .unwrap();
        assert_eq!(f, "failed");
    }

    #[test]
    fn list_status_filter_rejects_unknown() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": "running"})).is_err());
    }

    #[test]
    fn list_status_filter_rejects_non_string() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": 5})).is_err());
    }

    fn sample_checkpoint(run_id: &str, status: &str, created_at: u64) -> Value {
        json!({
            "run_id": run_id,
            "status": status,
            "created_at": created_at,
            "updated_at": created_at + 1,
            "agent_results": {},
            "total_tokens": 100,
        })
    }

    #[test]
    fn list_entry_from_checkpoint_skips_non_terminal() {
        assert!(build_entry_from_checkpoint(
            &sample_checkpoint("r", "running", 1),
            "r"
        )
        .is_none());
    }

    #[test]
    fn list_entry_from_checkpoint_keeps_terminal() {
        let e = build_entry_from_checkpoint(
            &sample_checkpoint("r1", "completed", 2),
            "loom-instance_r1",
        )
        .unwrap();
        assert_eq!(e["instance_id"], "r1");
        assert_eq!(e["status"], "completed");
        assert_eq!(e["instance_dir"], "loom-instance_r1");
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

    fn write_checkpoint(dir: &Path, run_id: &str, status: &str, created_at: u64) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("checkpoint.json"),
            serde_json::to_vec_pretty(&sample_checkpoint(run_id, status, created_at)).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn collect_instances_under_skips_non_terminal_and_keeps_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let root: PathBuf = tmp.path().join(".luft").join("runs");
        write_checkpoint(&root.join("done"), "rd", "completed", 1);
        write_checkpoint(&root.join("alive"), "ra", "running", 2);
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

    // -- events helpers --

    #[test]
    fn events_offset_default_zero() {
        assert_eq!(parse_events_offset(&json!({})), 0);
    }

    #[test]
    fn events_offset_explicit() {
        assert_eq!(parse_events_offset(&json!({"offset": 7})), 7);
    }

    #[test]
    fn events_limit_default_50() {
        assert_eq!(parse_events_limit(&json!({})), 50);
    }

    #[test]
    fn events_limit_clamps_above_max() {
        assert_eq!(parse_events_limit(&json!({"events_limit": 10_000})), 500);
    }

    #[test]
    fn events_limit_clamps_below_min() {
        assert_eq!(parse_events_limit(&json!({"events_limit": 0})), 1);
    }

    #[test]
    fn events_types_missing_is_none() {
        assert!(parse_events_types(&json!({})).is_none());
    }

    #[test]
    fn events_types_null_is_none() {
        assert!(parse_events_types(&json!({"types": null})).is_none());
    }

    #[test]
    fn events_types_empty_array_is_none() {
        assert!(parse_events_types(&json!({"types": []})).is_none());
    }

    #[test]
    fn events_types_collects_strings() {
        let t = parse_events_types(&json!({"types": ["a", "b"]})).unwrap();
        assert_eq!(t, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn events_agent_id_missing_is_none() {
        assert!(parse_events_agent_id(&json!({})).is_none());
    }

    #[test]
    fn events_agent_id_null_is_none() {
        assert!(parse_events_agent_id(&json!({"agent_id": null})).is_none());
    }

    #[test]
    fn events_agent_id_returns_some() {
        assert_eq!(
            parse_events_agent_id(&json!({"agent_id": "aid"})).unwrap(),
            "aid"
        );
    }

    fn ev(value: Value) -> Value {
        value
    }

    #[test]
    fn event_matches_types_filters_by_set() {
        let mut set = HashSet::new();
        set.insert("run_done");
        assert!(event_matches_types(&ev(json!({"type": "run_done"})), &set));
        assert!(!event_matches_types(&ev(json!({"type": "agent_started"})), &set));
    }

    #[test]
    fn event_matches_agent_id_compares_strictly() {
        assert!(event_matches_agent_id(&ev(json!({"agent_id": "a"})), "a"));
        assert!(!event_matches_agent_id(&ev(json!({"agent_id": "b"})), "a"));
        assert!(!event_matches_agent_id(&ev(json!({"type": "x"})), "a"));
    }

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
            msg.contains("corrupt") || msg.contains("missing checkpoint"),
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
        std::fs::write(
            wf_dir.join("alpha.lua"),
            "-- alpha\nfunction main() end\n",
        )
        .unwrap();
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
