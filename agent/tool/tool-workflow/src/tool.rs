use async_trait::async_trait;
use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use luft_core::contract::ids::TokenUsage;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use tool_core::{
    BuiltinSkill, Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy,
    ToolSourceError, ToolSpec,
};

const WORKFLOW_SKILL: &str = include_str!("workflow_skill.md");
const REF_TOOL_USAGE: &str = include_str!("references/tool-usage.md");
const REF_DSL_REFERENCE: &str = include_str!("references/dsl-reference.md");
const REF_ARCH_HEADER: &str = include_str!("references/architecture-header.md");
const REF_AGENT_PROMPTS: &str = include_str!("references/agent-prompts.md");
const REF_DECOMPOSITION: &str = include_str!("references/task-decomposition.md");
const REF_ADVERSARIAL: &str = include_str!("references/adversarial-verification.md");
const REF_EXAMPLES: &str = include_str!("references/examples.md");

const DEFAULT_CONCURRENCY: usize = 4;
const MAX_CONCURRENCY: usize = 64;

// T-06: instance-events pagination defaults.
const DEFAULT_EVENTS_LIMIT: u64 = 50;
const MAX_EVENTS_LIMIT: u64 = 500;

use crate::backend::LoomAgentBackend;
use crate::event_bridge::luft_event_to_json;
use crate::instance::{build_instance_meta, write_instance_artifacts, ReportRef, WorkflowRef};
use crate::workflow_resolver::resolve_workflow;

pub struct WorkflowTool {
    config_template: agent::agent::AgentConfig,
}

/// T-04: Default page size for `action=list-instances` when the caller
/// does not specify `limit`. Chosen to fit well under the typical 50 KB
/// per-tool-result budget while still letting a single page scan ~25
/// instances.
const DEFAULT_LIST_INSTANCES_LIMIT: usize = 20;

/// T-04: Upper bound on `limit` for `action=list-instances`. Prevents a
/// caller from asking for thousands of entries in one go, which would
/// blow past the tool-result budget and force a second round-trip anyway.
const MAX_LIST_INSTANCES_LIMIT: usize = 100;

/// T-04: Allowed status filter values for the `list-instances` action.
/// Kept lowercase so callers can pass either case; the parser lowercases
/// incoming strings before matching.
const LIST_INSTANCES_STATUS_FILTERS: &[&str] = &["completed", "failed", "cancelled"];


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

/// T-03: Truncate a string for the `report_preview` field of the
/// compact summary. Always operates on character boundaries (so we
/// don't slice a multi-byte UTF-8 code point). When the string fits
/// inside `max_bytes` it is returned as-is.
fn truncate_for_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Walk back to the previous char boundary so we never slice a
    // multi-byte codepoint mid-sequence.
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 16);
    out.push_str(&s[..cut]);
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// T-04 -- `handle_list_instances` parsers and helpers
// ---------------------------------------------------------------------------
//
// `parse_list_instances_limit`, `parse_list_instances_cursor`, and
// `parse_list_instances_status_filter` are all defensive: they tolerate
// missing keys (return the spec default), explicit `null` (treated the same
// as missing), and valid values, but reject malformed types or out-of-range
// numbers with `ToolSourceError::InvalidInput`. Unknown status strings are
// rejected so we never silently let through typos like "fail" or "Compete".

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
    v.as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
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

fn read_json_value(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Build a list-instances entry record from a clean `instance.json` (T-03+).
///
/// Per design doc (list-instances), the `workflow` field exposes only
/// `{kind, name}` -- the absolute `path` is intentionally omitted to keep
/// the list response lightweight. `source` is supplied by the caller
/// (typically "current" or "legacy") based on which root directory
/// produced the file.
fn build_entry_from_instance_json(
    v: &Value,
    dir_name: &str,
    source: &'static str,
) -> Value {
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
        "source": source,
    })
}

/// Fallback for legacy entries that predate the `instance.json` migration
/// (T-05 will regenerate these on-the-fly). The synthetic `workflow`
/// collapses to `{kind:"file",name:<dir>}` because `checkpoint.json` does
/// not record the original workflow name.
fn build_entry_from_checkpoint(
    ckpt: &Value,
    dir_name: &str,
    source: &'static str,
) -> Value {
    let agent_count = ckpt
        .get("agent_results")
        .and_then(|x| x.as_object())
        .map(|o| o.len() as u64)
        .unwrap_or(0);
    json!({
        "instance_id": ckpt.get("run_id").and_then(|x| x.as_str()).unwrap_or("?"),
        "instance_dir": dir_name,
        "status": ckpt.get("status").and_then(|x| x.as_str()).unwrap_or("unknown"),
        "workflow": {
            "kind": "file",
            "name": dir_name,
        },
        "created_at": ckpt.get("created_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "completed_at": ckpt.get("updated_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "total_tokens": ckpt.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        "agent_count": agent_count,
        "source": source,
    })
}

/// Walk a directory of run/instance directories, preferring `instance.json`
/// for O(1) summaries and falling back to `checkpoint.json` for legacy
/// entries. Directories with neither artifact are skipped silently, per
/// the design doc's "skip the entry silently" requirement.
fn collect_instances_under(root: &Path, source: &'static str, out: &mut Vec<Value>) {
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
            out.push(build_entry_from_instance_json(&inst, &dir_name, source));
            continue;
        }

        let ckpt_path = path.join("checkpoint.json");
        if let Some(ckpt) = read_json_value(&ckpt_path) {
            out.push(build_entry_from_checkpoint(&ckpt, &dir_name, source));
        }
        // else: skip silently -- the design task explicitly requires this.
    }
}


// ---------------------------------------------------------------------------
// T-06 -- `handle_instance_events` argument parsers + matching helpers
// ---------------------------------------------------------------------------

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

/// Match an event against the type filter set. Event is included iff
/// its `type` field is one of the allowed values.
fn event_matches_types(event: &Value, types: &HashSet<&str>) -> bool {
    event
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| types.contains(t))
        .unwrap_or(false)
}

/// Match an event against the agent filter. Event is included iff its
/// `agent_id` exactly equals `agent_id`.
fn event_matches_agent_id(event: &Value, agent_id: &str) -> bool {
    event
        .get("agent_id")
        .and_then(|a| a.as_str())
        .map(|s| s == agent_id)
        .unwrap_or(false)
}

impl WorkflowTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self { config_template }
    }

    fn instances_dir(&self) -> PathBuf {
        self.config_template
            .working_folder
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join(".loom")
            .join("instances")
    }

    /// T-04: Legacy location for historical runs that predate the
    /// `.loom/instances/` migration. `handle_list_instances` walks BOTH
    /// this root and `instances_dir()` so legacy entries remain visible
    /// after upgrades. The new instance layout (`loom-instance_*`) uses
    /// `.loom/instances/`, but pre-T-02 Luft runs are still under
    /// `.luft/runs/`. Source-tagged as "legacy" in the listing so callers
    /// can tell them apart from current entries.
    fn runs_dir(&self) -> PathBuf {
        self.config_template
            .working_folder
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join(".luft")
            .join("runs")
    }

    fn workflows_dir(&self) -> PathBuf {
        self.config_template
            .working_folder
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join(".loom")
            .join("workflows")
    }

    /// T-03: Resolve the per-instance artefact directory under the new
    /// `.loom/instances/` layout (the post-T-02 path). Distinct name from
    /// `instances_dir()` so T-02's planned rename of the legacy
    /// `runs_dir()` does not collide with this helper — T-02 owns the
    /// rename and the new path layout.
    fn loom_instance_dir(&self, run_dir_name: &str) -> PathBuf {
        self.config_template
            .working_folder
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join(".loom")
            .join("instances")
            .join(run_dir_name)
    }

    /// T-03: Build the [`crate::instance::InstanceMeta`] for one
    /// finished run, persist the artefacts (`instance.json`,
    /// optional `report.json`, optional `agent-outputs/<aid>.txt`) and
    /// emit the curated compact JSON summary the `execute` action
    /// returns to the LLM.
    ///
    /// Always best-effort: any I/O failure is logged via `eprintln!`
    /// (so it surfaces in the CLI's stderr stream) and the function
    /// still produces a summary derived from whatever it managed to
    /// read. Only the "checkpoint missing" case is fatal — without it
    /// there is nothing meaningful to summarise.
    fn finalize_execute(
        &self,
        run_dir_name: &str,
        final_status: &str,
        final_report: Option<&Value>,
        display_name: &str,
        is_inline_script: bool,
        workflow_arg: Option<&str>,
    ) -> Result<String, ToolSourceError> {
        let instance_dir = self.loom_instance_dir(run_dir_name);

        let checkpoint_path = instance_dir.join("checkpoint.json");
        let checkpoint_bytes = std::fs::read(&checkpoint_path).map_err(|e| {
            ToolSourceError::ToolError(format!(
                "checkpoint missing after execute at {}: {e}",
                checkpoint_path.display()
            ))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)
            .map_err(|e| ToolSourceError::ToolError(format!("invalid checkpoint JSON: {e}")))?;

        let events: Vec<Value> = std::fs::read_to_string(instance_dir.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        // Raw outputs of agents whose reply exceeds the inline limit
        // (2048 bytes). We only collect these for the per-agent file
        // backing step; the inlined `output_preview` is always derived
        // inside `instance.rs` from the event payload itself.
        let raw_agent_outputs: Vec<(String, String)> = events
            .iter()
            .filter_map(|ev| {
                if ev.get("type").and_then(|v| v.as_str()) != Some("agent_done") {
                    return None;
                }
                let aid = ev.get("agent_id").and_then(|v| v.as_str())?.to_string();
                let out = ev.get("output")?;
                let raw = match out {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if raw.len() as u64 > crate::instance::AGENT_OUTPUT_INLINE_LIMIT as u64 {
                    Some((aid, raw))
                } else {
                    None
                }
            })
            .collect();

        let workflow_src = std::fs::read_to_string(instance_dir.join("workflow.lua")).ok();

        let workflow_ref = WorkflowRef {
            kind: if is_inline_script { "inline" } else { "file" },
            name: Some(display_name.to_string()),
            path: workflow_arg.map(|s| s.to_string()),
        };

        let meta = build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            run_dir_name.to_string(),
            &checkpoint_bytes,
        );

        if let Err(e) =
            write_instance_artifacts(&instance_dir, &meta, final_report, &raw_agent_outputs)
        {
            eprintln!(
                "warning: failed to write instance artefacts to {}: {e}",
                instance_dir.display()
            );
        }

        // Status preference: the runtime's verdict (passed in by the
        // caller) wins over `meta.status`. The meta-derived status is
        // useful for "run_done never observed" cases; once we have a
        // runtime verdict we want it reflected exactly.
        let status = if final_status == "unknown" {
            meta.status.clone()
        } else {
            final_status.to_string()
        };

        let (report_ref, report_preview): (Option<String>, Option<String>) = match &meta.report {
            ReportRef::Inline(v) => (None, Some(truncate_for_preview(&v.to_string(), 800))),
            ReportRef::File { r#ref, preview, .. } => (Some(r#ref.clone()), Some(preview.clone())),
            ReportRef::Empty => (None, None),
        };

        let summary = json!({
            "instance_id": meta.instance_id,
            "instance_dir": meta.instance_dir,
            "status": status,
            "workflow": {
                "kind": meta.workflow.kind,
                "name": meta.workflow.name,
                "path": meta.workflow.path,
            },
            "agent_count": meta.agent_count,
            "total_tokens": meta.total_tokens,
            "total_elapsed_ms": meta.total_elapsed_ms,
            "report_ref": report_ref,
            "report_preview": report_preview,
        });

        Ok(serde_json::to_string_pretty(&summary).unwrap_or_default())
    }

    /// T-03: New `execute` action handler. Structurally mirrors
    /// [`Self::handle_run`] but:
    /// 1. Writes Luft's per-run dir under `.loom/instances/` (the
    ///    post-T-02 layout) instead of `.luft/runs/`.
    /// 2. After the run terminates, builds the clean-layer
    ///    `InstanceMeta`, persists the artefacts (`instance.json`,
    ///    `report.json`, `agent-outputs/<aid>.txt`) and returns the
    ///    curated compact JSON summary.
    ///
    /// This is a distinct fn (not a rename of `handle_run`) so the
    /// pre-merge code keeps both surfaces working: `action: "run"`
    /// continues to behave exactly as before, while `action: "execute"`
    /// drives the new wiring. T-02's rename of `handle_run` →
    /// `handle_execute` will produce a name collision that the merge
    /// step resolves by keeping this version.
    async fn handle_execute(
        &self,
        args: &Value,
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
                let working_folder = self
                    .config_template
                    .working_folder
                    .as_deref()
                    .unwrap_or_else(|| Path::new("."));

                let path =
                    resolve_workflow(w, working_folder).map_err(ToolSourceError::InvalidInput)?;

                let source = std::fs::read_to_string(&path).map_err(|e| {
                    ToolSourceError::ToolError(format!("Failed to read workflow: {e}"))
                })?;

                (source, path.display().to_string())
            }
            (None, None) => {
                return Err(ToolSourceError::InvalidInput(
                    "Either 'script' or 'workflow' must be provided.".to_string(),
                ));
            }
        };

        let concurrency = parse_concurrency(args)?;
        let user_args = extract_user_args(args);
        let lua_source = inject_args_globals(&lua_source, user_args.as_ref());

        // T-03: use the post-T-02 path layout for Luft's base dir.
        // Ensure the parent directory exists so Luft's create_dir_all
        // inside build() succeeds and there is no race with the
        // artefact writer.
        let base_dir = self
            .config_template
            .working_folder
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join(".loom")
            .join("instances");
        if let Err(e) = std::fs::create_dir_all(&base_dir) {
            return Err(ToolSourceError::ToolError(format!(
                "Failed to create instances directory {}: {e}",
                base_dir.display()
            )));
        }
        let backend = LoomAgentBackend::new(self.config_template.clone());

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

        let mut forward_rx = run_handle.subscribe();
        let mut done_rx = run_handle.subscribe();

        let sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
        tokio::spawn(async move {
            while let Ok(ev) = forward_rx.recv().await {
                if let Some(ref s) = sender {
                    s(luft_event_to_json(&ev));
                }
            }
        });

        let cancel_token = CancellationToken::new();
        let parent_cancelled = ctx.and_then(|c| c.run_cancellation.clone());
        if let Some(rc) = parent_cancelled {
            let ct = cancel_token.clone();
            tokio::spawn(async move {
                rc.token().cancelled().await;
                ct.cancel();
            });
        }

        // Snapshot what we need before consuming run_handle.
        let run_dir_name = run_handle.run_dir_name().to_string();
        let is_inline_script = script.is_some();
        let workflow_arg_owned = workflow.map(|s| s.to_string());

        // Outcome slots, populated either by RunDone (success / fail)
        // or by the cancellation / channel-closed escape paths. The
        // `#[allow(unused_assignments)]` on `final_status` suppresses
        // a false-positive warning: `Ok(_)` arms in both the select
        // and cancellation branches either `continue` (loop body
        // reassigns on the next iteration) or `break` only after
        // assigning. The default `None` is never observed at read-time
        // because we only read after the `loop` exits via a `break`
        // arm that did the assignment.
        #[allow(unused_assignments)]
        let mut final_status: Option<&'static str> = None;
        let mut final_report: Option<Value> = None;

        let mut cancelled = false;
        loop {
            if cancelled {
                match done_rx.recv().await {
                    Ok(LuftAgentEvent::RunDone { .. }) => {
                        final_status = Some("cancelled");
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => {
                        final_status = Some("cancelled");
                        break;
                    }
                }
            }

            tokio::select! {
                ev = done_rx.recv() => {
                    match ev {
                        Ok(LuftAgentEvent::RunDone { report, status, .. }) => {
                            final_status = Some(match status {
                                luft_core::contract::event::RunStatus::Completed => "completed",
                                luft_core::contract::event::RunStatus::Failed => "failed",
                                luft_core::contract::event::RunStatus::Cancelled => "cancelled",
                                luft_core::contract::event::RunStatus::Partial => "completed",
                            });
                            final_report = Some(report);
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => {
                            final_status = Some("failed");
                            break;
                        }
                    }
                }
                _ = cancel_token.cancelled(), if !cancelled => {
                    cancelled = true;
                    run_handle.cancel();
                }
            }
        }

        // After the loop, every reachable exit writes final_status.
        // The `take().unwrap()` here would crash on an impossible
        // path; instead we fall back to "unknown" so a missing signal
        // still produces a usable summary.
        let mut final_status: &'static str = final_status.unwrap_or("unknown");

        // Drain the underlying task so checkpoint.json + events.jsonl
        // are fully flushed to disk before we read them. `IntoFuture`
        // is implemented on RunHandle; the JoinFuture returns
        // Result<RunOutcome, LuftError>. We only need `outcome.result`
        // here — the runtime verdict (final_status) was already
        // captured from the RunDone event above. Failures from the
        // drain are best-effort: the script may have crashed before
        // reporting, leaving us with no script-level report.
        match run_handle.into_future().await {
            Ok(outcome) => {
                // Only adopt the script-level report if we have not
                // already captured one via RunDone (the two sources
                // usually agree; preference given to the early
                // RunDone signal so cancellation vs failure
                // attribution is unambiguous).
                if final_report.is_none() {
                    if let Ok(report) = outcome.result {
                        final_report = Some(report);
                        if final_status == "unknown" {
                            final_status = "completed";
                        }
                    } else if final_status == "unknown" {
                        final_status = "failed";
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: workflow task drain failed: {e}");
                if final_status == "unknown" {
                    final_status = "failed";
                }
            }
        }

        let summary = self.finalize_execute(
            &run_dir_name,
            final_status,
            final_report.as_ref(),
            &display_name,
            is_inline_script,
            workflow_arg_owned.as_deref(),
        )?;

        // Suppress unused-variable lint when TokenUsage is not directly
        // referenced (we already extracted `report` and `status`; the
        // token usage is read by the clean-layer meta from the
        // checkpoint). Keep the import explicit so future maintenance
        // can re-introduce a status-specific output without an extra
        // import churn.
        let _ = std::marker::PhantomData::<TokenUsage>;

        Ok(ToolCallContent::Text(summary))
    }

    async fn handle_list_workflows(&self) -> Result<ToolCallContent, ToolSourceError> {
        let dir = self.workflows_dir();
        let mut workflows = Vec::new();

        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "lua") {
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        let modified = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());

                        let first_line = std::fs::read_to_string(&path)
                            .ok()
                            .and_then(|s| s.lines().next().map(|l| l.to_string()));

                        workflows.push(json!({
                            "name": name,
                            "size_bytes": size,
                            "modified": modified,
                            "preview": first_line,
                        }));
                    }
                }
            }
        }

        workflows.sort_by(|a, b| {
            let an = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let bn = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            bn.cmp(an)
        });

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json!({
                "workflows": workflows,
                "directory": dir.display().to_string(),
                "count": workflows.len(),
            }))
            .unwrap_or_default(),
        ))
    }

    async fn handle_list_instances(
        &self,
        args: &Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        // T-04: parse pagination/filter inputs up front. Each parser
        // returns Err on invalid input so we fail fast before any I/O.
        let limit = parse_list_instances_limit(args)?;
        let cursor = parse_list_instances_cursor(args);
        let status_filter = parse_list_instances_status_filter(args)?;

        // T-04: walk BOTH roots. Current instances live under
        // `instances_dir()`; legacy Luft runs under `runs_dir()`. Each is
        // source-tagged so callers can tell them apart. Entries from both
        // directories are merged into a single sorted page.
        let mut entries: Vec<Value> = Vec::new();
        collect_instances_under(&self.instances_dir(), "current", &mut entries);
        collect_instances_under(&self.runs_dir(), "legacy", &mut entries);

        // Step 3: case-insensitive exact match on the canonical lowercase
        // status. We lowercase both sides so legacy mixed-case statuses
        // ("Completed", "FAILED") still match.
        if let Some(ref sf) = status_filter {
            let want = sf.to_lowercase();
            entries.retain(|e| {
                e.get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_lowercase() == want)
                    .unwrap_or(false)
            });
        }

        // Step 4: sort by (created_at desc, instance_dir desc). The
        // instance_dir tiebreak keeps ordering stable when many entries
        // share the same created_at (e.g. checkpoint fallback at zero).
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

        // Step 5: cursor lookup. The cursor is the `instance_dir` of the
        // *last* entry from the previous page. Skip every entry whose
        // `instance_dir` exactly matches the cursor, then start with the
        // next one. A cursor that does not match any entry is an error:
        // returning a stale cursor silently would mask bugs in the
        // caller's pagination logic.
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
                        return Err(ToolSourceError::ToolError(format!(
                            "cursor not found: {c}"
                        )));
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

        // next_cursor is the instance_dir of the last returned entry, but
        // ONLY when there are more entries beyond the current page. When
        // the page exhausts the remaining list we surface null so callers
        // know to stop. An empty page also clears next_cursor (rather than
        // pointing past the end of the list).
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

    async fn handle_instance_summary(
        &self,
        instance_dir: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        // T-05: a single path segment. Reject anything that looks like a
        // nested path so we never escape the instances_dir() tree.
        if instance_dir.is_empty() || instance_dir.contains('/') || instance_dir.contains('\\') {
            return Err(ToolSourceError::InvalidInput(format!(
                "'instance_dir' must be a single path segment, got '{instance_dir}'"
            )));
        }

        // Preferred location is the post-migration `.loom/instances/`.
        // Fall back to the historical `.luft/runs/` so legacy artefacts
        // (T-02 in flight) and older fixtures still resolve.
        let new_path = self.instances_dir().join(instance_dir);
        let legacy_path = self.runs_dir().join(instance_dir);
        let resolved = if new_path.is_dir() {
            new_path
        } else if legacy_path.is_dir() {
            legacy_path
        } else {
            return Err(ToolSourceError::InvalidInput(format!(
                "Instance directory '{instance_dir}' not found in {} or {}",
                self.instances_dir().display(),
                self.runs_dir().display()
            )));
        };

        let instance_json_path = resolved.join("instance.json");

        // Fast path: a pre-built meta is already on disk. Surface it as-is.
        if instance_json_path.is_file() {
            let raw = std::fs::read_to_string(&instance_json_path).map_err(|e| {
                ToolSourceError::ToolError(format!(
                    "Failed to read instance.json at {}: {e}",
                    instance_json_path.display()
                ))
            })?;
            let value: Value = serde_json::from_str(&raw).map_err(|e| {
                ToolSourceError::ToolError(format!(
                    "Invalid instance.json at {}: {e}",
                    instance_json_path.display()
                ))
            })?;
            return Ok(ToolCallContent::Text(
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.clone()),
            ));
        }

        // Slow path: synthesise the meta from the raw checkpoint+events.
        // The events read falls back to "absent" rather than hard-fail so
        // a partial on-disk state (e.g. just checkpoint.json) still
        // produces a usable summary.
        let checkpoint_path = resolved.join("checkpoint.json");
        let checkpoint_bytes = std::fs::read(&checkpoint_path).map_err(|e| {
            ToolSourceError::ToolError(format!(
                "Failed to read checkpoint.json at {}: {e}",
                checkpoint_path.display()
            ))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes).map_err(|e| {
            ToolSourceError::ToolError(format!(
                "Invalid checkpoint.json at {}: {e}",
                checkpoint_path.display()
            ))
        })?;

        let events_path = resolved.join("events.jsonl");
        let events: Vec<Value> = match std::fs::read_to_string(&events_path) {
            Ok(raw) => raw
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            Err(_) => Vec::new(),
        };

        let workflow_src = std::fs::read_to_string(resolved.join("workflow.lua")).ok();

        // Synthesised meta always reports `kind="legacy"`: we lost the
        // original binding when the run pre-dated T-03's writer, and
        // there is no source path to recover for an inline workflow.
        let workflow_ref = crate::instance::WorkflowRef {
            kind: "legacy",
            name: Some(instance_dir.to_string()),
            path: None,
        };

        let meta = crate::instance::build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            instance_dir.to_string(),
            &checkpoint_bytes,
        );

        let pretty = serde_json::to_string_pretty(&meta).map_err(|e| {
            ToolSourceError::ToolError(format!("Failed to serialise InstanceMeta: {e}"))
        })?;

        // Best-effort persist: a write failure (read-only filesystem,
        // perms, etc.) should still return the freshly-built meta so the
        // caller is not penalised. Subsequent calls will just rebuild.
        if let Err(e) = std::fs::write(&instance_json_path, &pretty) {
            tracing::warn!(
                instance_dir = %instance_dir,
                path = %instance_json_path.display(),
                error = %e,
                "failed to persist instance.json; returning in-memory InstanceMeta anyway",
            );
        }

        Ok(ToolCallContent::Text(pretty))
    }


    /// T-06: paginated, filtered event-stream reader. Walks
    /// `<instances_dir>/<dir>/events.jsonl` line-by-line, applies the
    /// caller-supplied `types` and `agent_id` filters, and returns a
    /// single page of matching events. Pagination state lives in
    /// `(offset, events_limit)` and the response surfaces `next_offset`
    /// when more matching events exist past the current page.
    async fn handle_instance_events(
        &self,
        instance_dir: &str,
        offset: u64,
        events_limit: u64,
        types: Option<Vec<String>>,
        agent_id: Option<String>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        // T-06: Pinned to the post-migration `.loom/instances/` root.
        // Legacy `.luft/runs/<dir>/` reads go through `instance-summary`
        // (which already rebuilds an InstanceMeta on-demand) so we do
        // not duplicate the directory-resolution logic here.
        let path = self.instances_dir().join(instance_dir);
        let events_path = path.join("events.jsonl");

        let mut filtered_count: u64 = 0;
        let mut returned: usize = 0;
        let mut events: Vec<Value> = Vec::new();

        if let Ok(file) = std::fs::File::open(&events_path) {
            let types_set: Option<HashSet<&str>> =
                types.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());

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

        // next_offset points at the *unconsumed* matching event after
        // the last one we returned. We compute it from filtered_count
        // (after the filter, before the limit) so callers can iterate
        // deterministically. When `returned < events_limit` we ran
        // out of matching events, so `next_offset` is null.
        let next_offset = if offset + (returned as u64) < filtered_count {
            Some(offset + returned as u64)
        } else {
            None
        };

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json!({
                "instance_dir": instance_dir,
                "offset": offset,
                "events_limit": events_limit,
                "total_matching": filtered_count,
                "next_offset": next_offset,
                "events": events,
            }))
            .unwrap_or_default(),
        ))
    }

    /// T-07: read-only viewer for the Lua source of a finished
    /// instance. Resolves the per-instance directory under
    /// `.loom/instances/` and returns the contents of `workflow.lua`
    /// alongside the resolved path so callers can pivot into editing
    /// tools without re-deriving the location. Missing files fail
    /// loudly: the handler never falls back to a synthetic source,
    /// since this surface is specifically for *showing the script
    /// that ran*.
    async fn handle_instance_source(
        &self,
        instance_dir: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path = self.instances_dir().join(instance_dir);

        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "Instance directory '{instance_dir}' not found in {}",
                self.instances_dir().display()
            )));
        }

        let source = std::fs::read_to_string(path.join("workflow.lua")).map_err(|e| {
            ToolSourceError::ToolError(format!(
                "Failed to read workflow.lua from instance '{}': {e}",
                path.display()
            ))
        })?;

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json!({
                "instance_dir": instance_dir,
                "instance_path": path.display().to_string(),
                "workflow_source": source,
            }))
            .unwrap_or_default(),
        ))
    }

    fn with_deprecation(
        content: ToolCallContent,
        old_action: &str,
        new_action: &str,
    ) -> ToolCallContent {
        let ToolCallContent::Text(text) = content else {
            return content;
        };

        let deprecation = format!("{old_action} is now {new_action}; update your calls.");
        let mut value =
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "result": text }));

        if let Value::Object(object) = &mut value {
            object.insert("deprecation".to_string(), Value::String(deprecation));
        } else {
            value = json!({
                "result": value,
                "deprecation": deprecation,
            });
        }

        ToolCallContent::Text(serde_json::to_string_pretty(&value).unwrap_or_default())
    }
}

#[async_trait]
impl Tool for WorkflowTool {
    fn name(&self) -> &str {
        "workflow"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workflow".to_string(),
            description: Some(
                "Execute or inspect multi-agent workflows stored under .loom/.\n\n\
                 Actions:\n\
                 - execute (default): Run a workflow. Provide `script` (inline Lua) or `workflow` (name/path). Returns an instance summary; full report is written to .loom/instances/<dir>/report.json when large.\n\
                 - list-workflows: List .lua files in .loom/workflows/.\n\
                 - list-instances: List past instances (paginated). Start here when debugging.\n\
                 - instance-summary: Get the curated summary of one instance — status, agents, phase spans, event stats. Read this BEFORE instance-events.\n\
                 - instance-events: Page through the raw event stream with type/agent filters. Use after instance-summary.\n\
                 - instance-source: Get the workflow.lua that an instance executed.\n\n\
                 For the full action guide and the Lua DSL reference, load the `workflow` skill."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["execute", "list-workflows", "list-instances", "instance-summary", "instance-events", "instance-source"],
                        "description": "Action to perform. Default: 'execute'.",
                        "default": "execute"
                    },
                    "script": {
                        "type": "string",
                        "description": "(execute) Inline Lua source."
                    },
                    "workflow": {
                        "type": "string",
                        "description": "(execute) Name or path of a .lua workflow file."
                    },
                    "args": {
                        "type": "object",
                        "description": "(execute) Exposed as `_G._args` inside the script; declare `function main()` (no parameters).",
                        "additionalProperties": true
                    },
                    "concurrency": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 64,
                        "default": 4,
                        "description": "(execute) Maximum number of concurrent agents. Default: 4."
                    },
                    "instance_dir": {
                        "type": "string",
                        "description": "(instance-*) Instance directory name from list-instances."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "(list-instances) Max instances to return. Default: 20, max: 100."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "(list-instances) Opaque cursor from a previous page's `next_cursor`. Omit for the first page."
                    },
                    "status_filter": {
                        "type": "string",
                        "enum": ["completed", "failed", "cancelled"],
                        "description": "(list-instances) Restrict to entries with this status. Case-insensitive. Omit for all."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "(instance-events) Skip the first N matching events."
                    },
                    "events_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "default": 50,
                        "description": "(instance-events) Page size for returned events (clamped to 500)."  
                    },
                    "types": {
                        "type": ["array", "null"],
                        "items": {"type": "string"},
                        "description": "(instance-events) Restrict returned events to those whose `type` field is in this set. Default: no type filter."
                    },
                    "agent_id": {
                        "type": ["string", "null"],
                        "description": "(instance-events) Restrict returned events to those with this `agent_id`. Default: no agent filter."
                    }
                }
            }),
            output_hint: Some(ToolOutputHint::preferred(
                ToolOutputStrategy::FileRefWithExcerpt,
            )),
        }
    }

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let action = args
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or("execute");

        match action {
            "execute" => self.handle_execute(&args, ctx).await,
            "list-workflows" => self.handle_list_workflows().await,
            "list-instances" => self.handle_list_instances(&args).await,
            "instance-summary" => {
                let instance_dir = args
                    .get("instance_dir")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ToolSourceError::InvalidInput(
                            "'instance_dir' is required for action='instance-summary'."
                                .to_string(),
                        )
                    })?;
                self.handle_instance_summary(instance_dir).await
            }
            // Legacy action aliases are accepted for one minor release. They
            // deliberately remain outside the advertised JSON-schema enum.
            "run" => Ok(Self::with_deprecation(
                self.handle_execute(&args, ctx).await?,
                "run",
                "execute",
            )),
            "list-runs" => Ok(Self::with_deprecation(
                self.handle_list_instances(&args).await?,
                "list-runs",
                "list-instances",
            )),
            "run-status" => {
                let instance_dir = args
                    .get("instance_dir")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ToolSourceError::InvalidInput(
                            "'instance_dir' is required for action='instance-summary'."
                                .to_string(),
                        )
                    })?;
                Ok(Self::with_deprecation(
                    self.handle_instance_summary(instance_dir).await?,
                    "run-status",
                    "instance-summary",
                ))
            }
            "instance-events" => {
                let instance_dir = args
                    .get("instance_dir")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ToolSourceError::InvalidInput(
                            "'instance_dir' is required for action='instance-events'."
                                .to_string(),
                        )
                    })?
                    .to_string();
                let offset = parse_events_offset(&args);
                let events_limit = parse_events_limit(&args);
                let types = parse_events_types(&args);
                let agent_id = parse_events_agent_id(&args);
                self.handle_instance_events(
                    &instance_dir,
                    offset,
                    events_limit,
                    types,
                    agent_id,
                )
                .await
            }
            "instance-source" => {
                let instance_dir = args
                    .get("instance_dir")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ToolSourceError::InvalidInput(
                            "'instance_dir' is required for action='instance-source'."
                                .to_string(),
                        )
                    })?
                    .to_string();
                self.handle_instance_source(&instance_dir).await
            }
            other => Err(ToolSourceError::InvalidInput(format!(
                "Unknown action '{other}'. Valid: execute, list-workflows, list-instances, instance-summary, instance-events, instance-source."
            ))),
        }
    }

    fn builtin_skill(&self) -> Option<BuiltinSkill> {
        Some(BuiltinSkill {
            name: "workflow".to_string(),
            description: "Lua DSL reference for writing multi-agent workflows".to_string(),
            content: WORKFLOW_SKILL.to_string(),
            triggers: vec![
                "workflow".to_string(),
                "multi-agent".to_string(),
                "lua script".to_string(),
                "list-instances".to_string(),
                "instance-summary".to_string(),
                "debug workflow".to_string(),
                "workflow failed".to_string(),
                "workflow status".to_string(),
            ],
            requires_tools: vec!["workflow".to_string()],
            references: vec![
                (
                    "references/tool-usage.md".to_string(),
                    REF_TOOL_USAGE.to_string(),
                ),
                (
                    "references/dsl-reference.md".to_string(),
                    REF_DSL_REFERENCE.to_string(),
                ),
                (
                    "references/architecture-header.md".to_string(),
                    REF_ARCH_HEADER.to_string(),
                ),
                (
                    "references/agent-prompts.md".to_string(),
                    REF_AGENT_PROMPTS.to_string(),
                ),
                (
                    "references/task-decomposition.md".to_string(),
                    REF_DECOMPOSITION.to_string(),
                ),
                (
                    "references/adversarial-verification.md".to_string(),
                    REF_ADVERSARIAL.to_string(),
                ),
                (
                    "references/examples.md".to_string(),
                    REF_EXAMPLES.to_string(),
                ),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;
    use serde_json::json;

    #[test]
    fn concurrency_default_when_missing() {
        let args = json!({"action": "execute"});
        assert_eq!(parse_concurrency(&args).unwrap(), DEFAULT_CONCURRENCY);
    }

    #[test]
    fn concurrency_explicit_value() {
        let args = json!({"action": "execute", "concurrency": 8});
        assert_eq!(parse_concurrency(&args).unwrap(), 8);
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
        let args = json!({"action": "execute"});
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

    #[test]
    fn inject_with_complex_args() {
        let src = "local _ = _G._args";
        let args = json!({
            "items": [1, 2, 3],
            "opts": {"depth": 2, "recursive": false},
            "note": "hi\nthere"
        });
        let out = inject_args_globals(src, Some(&args));
        assert!(out.contains("items = {1, 2, 3}"));
        assert!(out.contains("recursive = false"));
        assert!(out.contains(r#"note = "hi\nthere""#));
        assert!(out.ends_with(src));
    }

    #[test]
    fn inject_real_workflow_with_args() {
        // Mimics a realistic Lua workflow reading its args via _G._args
        let src = r#"
local args = _G._args
local topic = args.topic
local tags = args.tags
report({topic = topic, tag_count = #tags})
"#;
        let args = json!({
            "topic": "rust async",
            "tags": ["tokio", "async-trait"]
        });
        let out = inject_args_globals(src, Some(&args));
        assert!(out.starts_with("_G._args = "));
        assert!(out.contains(r#"topic = "rust async""#));
        assert!(out.contains(r#"tags = {"tokio", "async-trait"}"#));
        assert!(out.ends_with(src.trim_start()));
    }

    // --- T-03 wiring tests ----------------------------------------------

    use crate::instance::ReportRef as _ReportRef;
    use std::fs;
    use tempfile::TempDir;

    /// T-03: `truncate_for_preview` is pure and just byte-truncates for
    /// inline summary display. The integrity property we care about is
    /// that we never slice a multi-byte UTF-8 codepoint in the middle.
    #[test]
    fn truncate_short_input_returned_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_for_preview(s, 800), s);
    }

    #[test]
    fn truncate_long_input_with_ellipsis() {
        let s: String = "a".repeat(1500);
        let out = truncate_for_preview(&s, 800);
        assert!(out.ends_with('…'));
        assert!(out.len() > 800);
        // The byte content before the ellipsis should be valid UTF-8
        // by construction (we only ever push UTF-8 boundary-safe cuts)
        // and should reflect the prefix of the input.
        assert!(out.starts_with(&"a".repeat(800)));
    }

    #[test]
    fn truncate_preserves_multibyte_boundaries() {
        // String with multi-byte chars; cut must land on a char boundary.
        let emoji = "🦀".repeat(2000);
        let out = truncate_for_preview(&emoji, 100);
        // Each emoji is 4 bytes, so 100 bytes = 25 emojis then '…'.
        // Validate by reconstructing: strip trailing '…' and count chars.
        let prefix = out.trim_end_matches('…');
        for ch in prefix.chars() {
            // Each char must be the original emoji; if truncation had
            // sliced in the middle we'd get a 1-3 byte chunk that
            // wouldn't round-trip to a valid char.
            assert_eq!(ch, '🦀');
        }
    }

    #[test]
    fn truncate_zero_width_is_just_ellipsis() {
        // Edge case: max=0 should not panic.
        let out = truncate_for_preview("xxxx", 0);
        assert_eq!(out, "…");
    }

    /// T-03: `loom_instance_dir` derives the new `.loom/instances/`
    /// layout from the configured working folder. This is the path
    /// computation that avoids scanning the directory for instances —
    /// we read straight from the configured base.
    #[test]
    fn loom_instance_dir_path_is_under_instances_layout() {
        let tmp = TempDir::new().unwrap();
        let cfg = agent::agent::AgentConfig {
            working_folder: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let tool = WorkflowTool::new(cfg);

        let p = tool.loom_instance_dir("loom-instance_123");
        assert!(p.ends_with(".loom/instances/loom-instance_123"));
        assert!(p.starts_with(tmp.path()));
    }

    /// T-03: `finalize_execute` reads the post-T-02 instance layout,
    /// writes the clean-layer artefacts, and returns a structured
    /// JSON summary that matches the data on disk.
    ///
    /// The test simulates a finished run by hand-writing a
    /// checkpoint.json, events.jsonl, workflow.lua under the same
    /// `.loom/instances/<run_dir>/` path Luft would have produced.
    #[test]
    fn finalize_execute_writes_artefacts_and_returns_summary() {
        let tmp = TempDir::new().unwrap();
        let cfg = agent::agent::AgentConfig {
            working_folder: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let tool = WorkflowTool::new(cfg);

        // Layout under test: <tmp>/.loom/instances/loom-instance_9999/
        let run_dir_name = "loom-instance_9999";
        let instance_dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(run_dir_name);
        fs::create_dir_all(&instance_dir).unwrap();

        // Synthetic checkpoint.json capturing a tiny, completed run.
        let checkpoint = json!({
            "run_id": "loom-instance_9999",
            "status": "completed",
            "created_at": 1730000000_u64,
            "updated_at": 1730000010_u64,
            "total_tokens": 4200_u64,
            "agent_results": {
                "a1": {"status": "done", "tokens": 4200_u64, "elapsed_ms": 950_u64},
            },
        });
        let checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
        fs::write(instance_dir.join("checkpoint.json"), &checkpoint_bytes).unwrap();

        // Events.jsonl must contain BOTH `agent_started` (so
        // instance.rs's `build_agent_summaries` finds the agent) AND
        // `agent_done` (so the per-agent output / timing is recorded),
        // AND `run_done` (so `build_report_ref` locates the report).
        // The output is intentionally short — `finalize_execute` keeps
        // small outputs inline (no per-agent file backing for this one).
        let events = [
            json!({"type":"agent_started","run_id":"loom-instance_9999","phase_id":0,
                   "agent_id":"a1","prompt_preview":"say hi","model":null,
                   "description":null,"role":null,"name":null,"agent_seq":0}),
            json!({"type":"agent_done","run_id":"loom-instance_9999","agent_id":"a1",
                   "status":"Ok","tokens":{"input":100,"output":10,"cache_read":0,"cache_write":0},
                   "elapsed_ms":950_u64,"name":null,"agent_seq":0,"output":"hello world",
                   "findings":[],"prompt":"say hi","retry_count":0}),
            // A report that is large enough to push `build_report_ref`
            // into the `File` shape — a single property bag padding out
            // past `REPORT_INLINE_LIMIT` (800 bytes).
        ];
        // Build a large run-done report value that exceeds 800 bytes so
        // `build_report_ref` returns `ReportRef::File` and the
        // matching report.json on disk is exercised.
        let large_report = serde_json::json!({
            "ok": true,
            "verdict": "approved",
            "notes": "x".repeat(900),
        });
        let events_text = events
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
            + &json!({
                "type":"run_done",
                "run_id":"loom-instance_9999",
                "status":"completed",
                "report": large_report.clone()
            })
            .to_string()
            + "\n";
        fs::write(instance_dir.join("events.jsonl"), events_text).unwrap();

        fs::write(
            instance_dir.join("workflow.lua"),
            "function main() report({ok=true}) end",
        )
        .unwrap();

        // Caller already saw the RunDone report; pass it through.
        let final_report = json!({"ok": true, "verdict": "approved"});

        let summary_text = tool
            .finalize_execute(
                run_dir_name,
                "completed",
                Some(&final_report),
                "inline script",
                true,
                None,
            )
            .expect("finalize_execute should succeed");

        let summary: Value = serde_json::from_str(&summary_text).unwrap();

        // --- Summary structure ---
        assert_eq!(summary["status"], "completed");
        assert_eq!(summary["workflow"]["kind"], "inline");
        assert_eq!(summary["workflow"]["name"], "inline script");
        assert!(!summary["instance_id"].as_str().unwrap().is_empty());
        assert!(
            summary["instance_dir"]
                .as_str()
                .unwrap()
                .ends_with("loom-instance_9999"),
            "summary instance_dir must point to the post-T-02 path; got {:?}",
            summary["instance_dir"]
        );
        assert_eq!(summary["agent_count"], 1);
        assert_eq!(summary["total_tokens"], 4200);
        assert!(summary["total_elapsed_ms"].as_u64().unwrap() >= 950);
        // Final report is large enough for a backing file; the
        // reference must be present, the inline preview may be None.
        assert_eq!(
            summary["report_ref"].as_str().map(|s| s.to_string()),
            Some("report.json".to_string())
        );

        // --- Artefact files on disk ---
        let instance_json_path = instance_dir.join("instance.json");
        assert!(
            instance_json_path.exists(),
            "instance.json must be written next to checkpoint.json"
        );
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&instance_json_path).unwrap()).unwrap();
        assert_eq!(on_disk["schema_version"], 1);
        assert_eq!(on_disk["instance_id"], summary["instance_id"]);
        assert_eq!(on_disk["status"], "completed");

        let report_path = instance_dir.join("report.json");
        assert!(
            report_path.exists(),
            "report.json must be backed to disk when final_report is provided"
        );
        let rpt: Value = serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
        assert_eq!(rpt["verdict"], "approved");
    }

    /// T-03: when `finalize_execute` is called but no checkpoint.json
    /// has been written yet (engine crashed mid-run), the function must
    /// fail with a structured ToolError that names the missing path so
    /// the LLM can self-diagnose.
    #[test]
    fn finalize_execute_missing_checkpoint_returns_tool_error() {
        let tmp = TempDir::new().unwrap();
        let cfg = agent::agent::AgentConfig {
            working_folder: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let tool = WorkflowTool::new(cfg);

        // Layout exists but no checkpoint.json yet.
        let instance_dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join("loom-instance_deadbeef");
        fs::create_dir_all(&instance_dir).unwrap();

        let res = tool.finalize_execute(
            "loom-instance_deadbeef",
            "failed",
            None,
            "inline script",
            true,
            None,
        );
        let err = res.expect_err("missing checkpoint must be a ToolError, not Ok");
        let s = format!("{err:?}");
        assert!(
            s.contains("checkpoint missing"),
            "error must name what is missing; got {s:?}"
        );
        assert!(
            s.contains("loom-instance_deadbeef"),
            "error must name the instance dir; got {s:?}"
        );
    }

    /// T-03: an agent_done event with output longer than the inline
    /// limit (`AGENT_OUTPUT_INLINE_LIMIT` = 2048) must be backed up to
    /// `<instance_dir>/agent-outputs/<aid>.txt` so the small-cap
    /// summary stays scannable.
    #[test]
    fn finalize_execute_writes_large_agent_outputs_to_disk() {
        let tmp = TempDir::new().unwrap();
        let cfg = agent::agent::AgentConfig {
            working_folder: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let tool = WorkflowTool::new(cfg);

        let run_dir_name = "loom-instance_big";
        let instance_dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(run_dir_name);
        fs::create_dir_all(&instance_dir).unwrap();

        let checkpoint = json!({
            "run_id": run_dir_name,
            "status": "completed",
            "created_at": 1730000000_u64,
            "updated_at": 1730000010_u64,
            "total_tokens": 12000_u64,
            "agent_results": {
                "a_big": {"status": "done", "tokens": 12000_u64, "elapsed_ms": 5000_u64},
            },
        });
        let checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
        fs::write(instance_dir.join("checkpoint.json"), &checkpoint_bytes).unwrap();

        // Build an output that overflows 2048 bytes; the JSON-encoded
        // string of an `output` field will then exceed the limit.
        let big_output: String = "abcdefghij".repeat(300); // 3000 bytes
                                                           // Both `agent_started` AND `agent_done` are required:
                                                           // `build_agent_summaries` iterates `agent_started` events to
                                                           // discover the per-agent slot, then locates the matching
                                                           // `agent_done` for output / timing. Without the started
                                                           // event, the agent never gets an `AgentSummary`, hence no
                                                           // `output_ref` and no per-agent file.
        let events = [
            json!({"type":"agent_started","run_id":run_dir_name,"phase_id":0,
                   "agent_id":"a_big","prompt_preview":"say big","model":null,
                   "description":null,"role":null,"name":null,"agent_seq":0}),
            json!({"type":"agent_done","run_id":run_dir_name,"agent_id":"a_big",
                   "status":"Ok","tokens":{"input":100,"output":10,"cache_read":0,"cache_write":0},
                   "elapsed_ms":5000_u64,"name":null,"agent_seq":0,"output":big_output.clone(),
                   "findings":[],"prompt":"say big","retry_count":0}),
        ];
        let events_text = events
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(instance_dir.join("events.jsonl"), events_text).unwrap();
        fs::write(instance_dir.join("workflow.lua"), "").unwrap();

        let _ = tool
            .finalize_execute(run_dir_name, "completed", None, "inline script", true, None)
            .expect("ok");

        let back = instance_dir.join("agent-outputs").join("a_big.txt");
        assert!(
            back.exists(),
            "large agent output must be backed to agent-outputs/<aid>.txt; did not find {}",
            back.display()
        );
        let text = fs::read_to_string(&back).unwrap();
        assert_eq!(text, big_output);
    }

    // `_ReportRef` import kept to keep the report-file decision
    // explicit. Future readers searching for "ReportRef" in tests
    // will land on this crate's `instance.rs` even if all assertions
    // here exercise the wrapped paths on disk.
    #[allow(dead_code)]
    fn _force_use() {
        let _: _ReportRef = _ReportRef::Empty;
    }

    // ---------------------------------------------------------------------
    // handle_instance_source (action='instance-source') tests (T-07)
    // ---------------------------------------------------------------------

    fn tool_with_working_folder_for_tests(folder: &Path) -> WorkflowTool {
        let mut cfg = AgentConfig::default();
        cfg.working_folder = Some(folder.to_path_buf());
        WorkflowTool::new(cfg)
    }

    #[tokio::test]
    async fn handle_instance_source_returns_workflow_source() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_dir = tmp.path().join(".loom").join("instances").join("abc-123");
        std::fs::create_dir_all(&instance_dir).expect("mkdir");
        let lua_src = "function main() report({ok = true}) end\n";
        std::fs::write(instance_dir.join("workflow.lua"), lua_src).expect("write workflow.lua");

        let tool = tool_with_working_folder_for_tests(tmp.path());
        let result = tool
            .handle_instance_source("abc-123")
            .await
            .expect("handler should succeed");

        let ToolCallContent::Text(text) = result else {
            panic!("expected ToolCallContent::Text variant");
        };
        let parsed: Value = serde_json::from_str(&text).expect("output is JSON");
        assert_eq!(parsed["instance_dir"], "abc-123");
        assert_eq!(parsed["workflow_source"], lua_src);
        let path = parsed["instance_path"].as_str().unwrap_or("");
        assert!(
            path.contains(".loom") && path.contains("instances") && path.contains("abc-123"),
            "instance_path should contain .loom/instances/abc-123, got: {path}"
        );
    }

    #[tokio::test]
    async fn handle_instance_source_missing_dir_is_invalid_input() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = tool_with_working_folder_for_tests(tmp.path());

        let err = tool
            .handle_instance_source("does-not-exist")
            .await
            .expect_err("should fail");
        match err {
            ToolSourceError::InvalidInput(msg) => {
                assert!(msg.contains("does-not-exist"));
                assert!(msg.contains(".loom"));
                assert!(msg.contains("instances"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_instance_source_missing_workflow_lua_is_tool_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_dir = tmp.path().join(".loom").join("instances").join("partial");
        std::fs::create_dir_all(&instance_dir).expect("mkdir");

        let tool = tool_with_working_folder_for_tests(tmp.path());
        let err = tool
            .handle_instance_source("partial")
            .await
            .expect_err("should fail");
        match err {
            ToolSourceError::ToolError(msg) => {
                assert!(msg.contains("workflow.lua"));
                assert!(msg.contains("partial"));
            }
            other => panic!("expected ToolError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_dispatches_instance_source_action() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_dir = tmp.path().join(".loom").join("instances").join("disp-1");
        std::fs::create_dir_all(&instance_dir).expect("mkdir");
        let lua_src = "-- hello from dispatched instance\nreport({ok = true})\n";
        std::fs::write(instance_dir.join("workflow.lua"), lua_src).expect("write");

        let tool = tool_with_working_folder_for_tests(tmp.path());
        let args = json!({
            "action": "instance-source",
            "instance_dir": "disp-1",
        });
        let result = tool.call(args, None).await.expect("call should succeed");
        let ToolCallContent::Text(text) = result else {
            panic!("expected ToolCallContent::Text variant");
        };
        let parsed: Value = serde_json::from_str(&text).expect("output is JSON");
        assert_eq!(parsed["instance_dir"], "disp-1");
        assert_eq!(parsed["workflow_source"], lua_src);
    }

    #[tokio::test]
    async fn call_instance_source_without_instance_dir_is_invalid_input() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = tool_with_working_folder_for_tests(tmp.path());
        let args = json!({ "action": "instance-source" });

        let err = tool.call(args, None).await.expect_err("should fail");
        match err {
            ToolSourceError::InvalidInput(msg) => {
                assert!(
                    msg.contains("instance_dir"),
                    "error should mention instance_dir, got: {msg}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn instances_dir_is_loom_instances_subdir_of_working_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = tool_with_working_folder_for_tests(tmp.path());
        let expected = tmp.path().join(".loom").join("instances");
        assert_eq!(tool.instances_dir(), expected);
    }

}
