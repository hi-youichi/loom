use async_trait::async_trait;
use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use tool_core::tool_name::{
    TOOL_WORKFLOW_EVENTS, TOOL_WORKFLOW_FILES, TOOL_WORKFLOW_LIST, TOOL_WORKFLOW_SOURCE,
    TOOL_WORKFLOW_START, TOOL_WORKFLOW_STATUS,
};
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

const DEFAULT_EVENTS_LIMIT: u64 = 50;
const MAX_EVENTS_LIMIT: u64 = 500;

const DEFAULT_SOURCE_PREVIEW_LIMIT: usize = 8_192;
const DEFAULT_LIST_INSTANCES_LIMIT: usize = 20;
const MAX_LIST_INSTANCES_LIMIT: usize = 100;
const LIST_INSTANCES_STATUS_FILTERS: &[&str] = &["completed", "failed", "cancelled"];

use crate::backend::LoomAgentBackend;
use crate::event_bridge::luft_event_to_json;
use crate::instance::{
    build_instance_meta, write_instance_artifacts, InstanceMeta, ReportRef, WorkflowRef,
};
use crate::workflow_resolver::resolve_workflow;

#[derive(Clone)]
pub(crate) struct WorkflowRuntime {
    config_template: agent::agent::AgentConfig,
}

impl WorkflowRuntime {
    fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self { config_template }
    }

    fn working_folder(&self) -> PathBuf {
        self.config_template
            .working_folder
            .clone()
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn instances_root(&self) -> PathBuf {
        self.working_folder().join(".loom").join("instances")
    }

    fn runs_root(&self) -> PathBuf {
        self.working_folder().join(".luft").join("runs")
    }

    #[allow(dead_code)]
    fn workflows_dir(&self) -> PathBuf {
        self.working_folder().join(".loom").join("workflows")
    }

    fn loom_instance_dir(&self, run_dir_name: &str) -> PathBuf {
        self.instances_root().join(run_dir_name)
    }

    fn resolve_instance_path(&self, instance_dir: &str) -> Option<PathBuf> {
        let new_path = self.instances_root().join(instance_dir);
        if new_path.is_dir() {
            return Some(new_path);
        }
        let legacy_path = self.runs_root().join(instance_dir);
        if legacy_path.is_dir() {
            return Some(legacy_path);
        }
        None
    }

    async fn terminal_checkpoint_status(&self, run_dir_name: &str) -> Option<&'static str> {
        let path = self.loom_instance_dir(run_dir_name).join("checkpoint.json");
        let bytes = tokio::fs::read(path).await.ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        match value.get("status").and_then(Value::as_str) {
            Some("completed") => Some("completed"),
            Some("failed") => Some("failed"),
            Some("cancelled") => Some("cancelled"),
            _ => None,
        }
    }

    async fn finalize(
        &self,
        run_dir_name: &str,
        final_status: &str,
        final_report: Option<&Value>,
        display_name: &str,
        is_inline_script: bool,
        workflow_arg: Option<&str>,
    ) -> Result<(), ToolSourceError> {
        let instance_dir = self.loom_instance_dir(run_dir_name);

        let checkpoint_path = instance_dir.join("checkpoint.json");
        let mut last_err = String::new();
        let mut bytes = None;
        for _ in 0..10 {
            match tokio::fs::read(&checkpoint_path).await {
                Ok(b) if !b.is_empty() => {
                    bytes = Some(b);
                    break;
                }
                Ok(_) => last_err = "workflow state is empty".into(),
                Err(e) => last_err = e.to_string(),
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let checkpoint_bytes = bytes.ok_or_else(|| {
            ToolSourceError::ToolError(format!(
                "workflow state missing or empty after retries: {last_err}"
            ))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)
            .map_err(|e| ToolSourceError::ToolError(format!("invalid workflow state: {e}")))?;

        let events: Vec<Value> = tokio::fs::read_to_string(instance_dir.join("events.jsonl"))
            .await
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

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

        let mut meta = build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            run_dir_name.to_string(),
            &checkpoint_bytes,
        );

        if final_status != "unknown" {
            meta.status = final_status.to_string();
        }

        write_instance_artifacts(&instance_dir, &meta, final_report, &raw_agent_outputs).map_err(
            |e| ToolSourceError::ToolError(format!("failed to write instance artifacts: {e}")),
        )?;

        Ok(())
    }

    fn write_minimal_failed_instance(
        &self,
        run_dir_name: &str,
        display_name: &str,
        is_inline_script: bool,
        workflow_arg: Option<&str>,
        error_msg: &str,
    ) {
        let instance_dir = self.loom_instance_dir(run_dir_name);
        if std::fs::create_dir_all(&instance_dir).is_err() {
            return;
        }
        let workflow_ref = WorkflowRef {
            kind: if is_inline_script { "inline" } else { "file" },
            name: Some(display_name.to_string()),
            path: workflow_arg.map(|s| s.to_string()),
        };
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let meta = InstanceMeta {
            schema_version: crate::instance::SCHEMA_VERSION,
            instance_id: run_dir_name.to_string(),
            instance_dir: run_dir_name.to_string(),
            workflow: workflow_ref,
            status: "failed".to_string(),
            created_at: now_secs,
            completed_at: now_secs,
            total_tokens: 0,
            total_elapsed_ms: 0,
            agent_count: 0,
            agents: vec![],
            phase_spans: vec![],
            event_stats: Default::default(),
            report: ReportRef::Empty,
            checkpoint_hash: String::new(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            let _ = std::fs::write(instance_dir.join("instance.json"), json);
        }
        eprintln!(
            "warning: workflow {} failed during finalize: {}",
            run_dir_name, error_msg
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

fn truncate_for_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 16);
    out.push_str(&s[..cut]);
    out.push('…');
    out
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

fn read_json_value(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
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

fn validate_instance_dir_name(name: &str) -> Result<&str, ToolSourceError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(ToolSourceError::InvalidInput(format!(
            "'instance_dir' must be a single path segment, got '{name}'"
        )));
    }
    Ok(name)
}

fn instance_dir_arg<'a>(args: &'a Value, action: &str) -> Result<&'a str, ToolSourceError> {
    let dir = args
        .get("instance_dir")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("'instance_dir' is required for {action}."))
        })?;
    validate_instance_dir_name(dir)?;
    Ok(dir)
}

fn is_terminal_checkpoint(path: &Path) -> Option<bool> {
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let status = value.get("status").and_then(Value::as_str)?;
    let lower = status.to_ascii_lowercase();
    Some(matches!(lower.as_str(), "completed" | "failed" | "cancelled"))
}

pub fn sanitize_instance_for_public(mut value: Value) -> Value {
    if let Some(wf) = value.get_mut("workflow").and_then(|v| v.as_object_mut()) {
        wf.remove("path");
    }
    if let Some(agents) = value.get_mut("agents").and_then(|v| v.as_array_mut()) {
        for a in agents {
            if let Some(obj) = a.as_object_mut() {
                obj.remove("output_ref");
            }
        }
    }
    if let Some(report) = value.get_mut("report").and_then(|v| v.as_object_mut()) {
        if report.contains_key("ref") && report.contains_key("preview") {
            report.remove("ref");
        }
    }
    if let Some(obj) = value.as_object_mut() {
        obj.remove("checkpoint_hash");
    }
    value
}

pub struct WorkflowStartTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowStartTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowStartTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_START
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_START.to_string(),
            description: Some(
                "Start a Lua workflow in the background. Returns immediately with \
                 { instance_dir, status: 'running' }. Poll `workflow_status` to follow \
                 progress, or call `workflow_events` / `workflow_source` to inspect \
                 the resulting instance.\n\n\
                 Provide one of:\n\
                 - script: inline Lua source.\n\
                 - workflow: name or path of a .lua workflow file.\n\n\
                 Other inputs:\n\
                 - args (object): exposed to the script as `_G._args`.\n\
                 - concurrency (1..=64): maximum concurrent agents (default 4).\n\n\
                 This tool never blocks waiting for the workflow to finish — use \
                 `workflow_status` to wait. To wait several seconds before checking \
                 status, run a shell tool (`sleep 5` or PowerShell \
                 `Start-Sleep -Seconds 5`) and then call `workflow_status` with the \
                 returned `instance_dir`."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "Inline Lua source for the workflow."
                    },
                    "workflow": {
                        "type": "string",
                        "description": "Name or path of a .lua workflow file."
                    },
                    "args": {
                        "type": "object",
                        "description": "Exposed as `_G._args` inside the script.",
                        "additionalProperties": true
                    },
                    "concurrency": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 64,
                        "default": 4,
                        "description": "Maximum number of concurrent agents. Default: 4."
                    }
                },
                "oneOf": [
                    {"required": ["script"]},
                    {"required": ["workflow"]}
                ]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
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
                let working_folder = self.runtime.working_folder();
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

        let base_dir = self.runtime.instances_root();
        if let Err(e) = std::fs::create_dir_all(&base_dir) {
            return Err(ToolSourceError::ToolError(format!(
                "Failed to create instances directory: {e}"
            )));
        }

        let config = self.runtime.config_template.clone();
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
        let runtime = self.runtime.clone();
        tokio::spawn(async move {
            background_finalize(
                runtime,
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

    fn builtin_skill(&self) -> Option<BuiltinSkill> {
        Some(BuiltinSkill {
            name: "workflow".to_string(),
            description: "Lua DSL reference for writing multi-agent workflows".to_string(),
            content: WORKFLOW_SKILL.to_string(),
            triggers: vec![
                "workflow".to_string(),
                "multi-agent".to_string(),
                "lua script".to_string(),
                "workflow_list".to_string(),
                "workflow_status".to_string(),
                "debug workflow".to_string(),
                "workflow failed".to_string(),
                "workflow status".to_string(),
            ],
            requires_tools: vec![
                TOOL_WORKFLOW_START.to_string(),
                TOOL_WORKFLOW_STATUS.to_string(),
            ],
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
        if cancelled {
            match done_rx.recv().await {
                Ok(event) => {
                    if let Some(ref send) = sender {
                        send(luft_event_to_json(&event));
                    }
                    if let LuftAgentEvent::RunDone { report, .. } = event {
                        final_report = Some(report);
                    }
                    final_status = Some("cancelled");
                    break;
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => {
                    final_status = Some("cancelled");
                    break;
                }
            }
        }
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
                        final_status = Some("failed");
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

pub struct WorkflowStatusTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowStatusTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowStatusTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_STATUS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_STATUS.to_string(),
            description: Some(
                "Read the current state of a workflow instance. Returns status=running \
                 while it is active, or a complete terminal summary after it finishes. \
                 The summary includes agent results, token usage, phase timing, event \
                 statistics, and a bounded report preview. Internal references are \
                 never returned. Sleep between status checks instead of polling in a \
                 tight loop. Use `workflow_events` for detailed execution events or \
                 `workflow_source` for the executed Lua source."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    }
                },
                "required": ["instance_dir"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let dir = instance_dir_arg(&args, "workflow_status")?;
        let new_path = self.runtime.instances_root().join(dir);
        let legacy_path = self.runtime.runs_root().join(dir);

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
            let should_rebuild = if resolved == new_path {
                is_terminal_checkpoint(&checkpoint_path).unwrap_or(false)
            } else {
                true
            };
            if should_rebuild {
                return self.runtime.rebuild_summary(dir, &checkpoint_path).await;
            }
        }

        if resolved == new_path {
            let payload = json!({
                "instance_dir": dir,
                "status": "running",
            });
            return Ok(ToolCallContent::Text(
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            ));
        }

        Err(ToolSourceError::ToolError(format!(
            "Instance '{dir}' is incomplete (missing checkpoint)"
        )))
    }
}

impl WorkflowRuntime {
    async fn rebuild_summary(
        &self,
        instance_dir: &str,
        checkpoint_path: &Path,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let resolved = if checkpoint_path.starts_with(self.instances_root()) {
            self.instances_root().join(instance_dir)
        } else {
            self.runs_root().join(instance_dir)
        };
        let checkpoint_bytes = std::fs::read(checkpoint_path).map_err(|e| {
            ToolSourceError::ToolError(format!("Failed to read workflow state: {e}"))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)
            .map_err(|e| ToolSourceError::ToolError(format!("Invalid workflow state: {e}")))?;
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
        let workflow_ref = WorkflowRef {
            kind: "legacy",
            name: Some(instance_dir.to_string()),
            path: None,
        };
        let meta = build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            instance_dir.to_string(),
            &checkpoint_bytes,
        );
        let pretty = serde_json::to_string_pretty(&sanitize_instance_for_public(
            serde_json::to_value(&meta).map_err(|e| {
                ToolSourceError::ToolError(format!("Failed to serialise InstanceMeta: {e}"))
            })?,
        ))
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to serialise summary: {e}")))?;
        Ok(ToolCallContent::Text(pretty))
    }
}

pub struct WorkflowListTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowListTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowListTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_LIST
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_LIST.to_string(),
            description: Some(
                "List completed workflow instances with optional status filtering. \
                 Results are paginated by `limit` and opaque `cursor`, and include \
                 instance identifiers, status, workflow names, timestamps, token \
                 totals, and agent counts."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Max instances to return. Default: 20, max: 100."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Opaque cursor from a previous page's `next_cursor`."
                    },
                    "status_filter": {
                        "type": "string",
                        "enum": ["completed", "failed", "cancelled"],
                        "description": "Restrict to entries with this status. Case-insensitive."
                    }
                }
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let limit = parse_list_instances_limit(&args)?;
        let cursor = parse_list_instances_cursor(&args);
        let status_filter = parse_list_instances_status_filter(&args)?;

        let mut entries: Vec<Value> = Vec::new();
        collect_instances_under(&self.runtime.instances_root(), &mut entries);
        collect_instances_under(&self.runtime.runs_root(), &mut entries);

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
}

pub struct WorkflowEventsTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowEventsTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowEventsTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_EVENTS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_EVENTS.to_string(),
            description: Some(
                "Paginated, filtered access to the structured event stream of a \
                 workflow instance. Filters: `types` (array of event-type strings) \
                 and `agent_id`. Pagination: `offset` (skip N matching events) and \
                 `events_limit` (page size, 1..=500)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Skip the first N matching events."
                    },
                    "events_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "default": 50,
                        "description": "Page size (clamped to 500)."
                    },
                    "types": {
                        "type": ["array", "null"],
                        "items": {"type": "string"},
                        "description": "Restrict returned events to those whose `type` field is in this set."
                    },
                    "agent_id": {
                        "type": ["string", "null"],
                        "description": "Restrict returned events to those with this `agent_id`."
                    }
                },
                "required": ["instance_dir"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let dir = instance_dir_arg(&args, "workflow_events")?;
        let path = self
            .runtime
            .resolve_instance_path(dir)
            .ok_or_else(|| ToolSourceError::InvalidInput(format!("Instance '{dir}' not found")))?;
        let events_path = path.join("events.jsonl");
        let offset = parse_events_offset(&args);
        let events_limit = parse_events_limit(&args);
        let types = parse_events_types(&args);
        let agent_id = parse_events_agent_id(&args);

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
}

pub struct WorkflowSourceTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowSourceTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowSourceTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_SOURCE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_SOURCE.to_string(),
            description: Some(
                "Preview the Lua source backing a workflow instance. The source \
                 is truncated to a bounded preview; no path is exposed."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    }
                },
                "required": ["instance_dir"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let dir = instance_dir_arg(&args, "workflow_source")?;
        let resolved = self
            .runtime
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
}

pub struct WorkflowFilesTool {
    runtime: Arc<WorkflowRuntime>,
}

impl WorkflowFilesTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowFilesTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_FILES
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_FILES.to_string(),
            description: Some(
                "List the available Lua workflow definitions. Returns each workflow's \
                 name, size, and first non-empty line. Pass a returned name to \
                 `workflow_start`."
                    .to_string(),
            ),
            input_schema: json!({"type": "object", "properties": {}}),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        _args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let root = self.runtime.workflows_dir();
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
                let source = std::fs::read_to_string(&path).unwrap_or_default();
                let first_line = source
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or_default();
                workflows.push(json!({
                    "name": name,
                    "size_bytes": source.len(),
                    "first_line": truncate_for_preview(first_line, 200),
                }));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;

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
        assert!(out.starts_with(&"a".repeat(800)));
    }

    #[test]
    fn truncate_preserves_multibyte_boundaries() {
        let emoji = "🦀".repeat(2000);
        let out = truncate_for_preview(&emoji, 100);
        let prefix = out.trim_end_matches('…');
        for ch in prefix.chars() {
            assert_eq!(ch, '🦀');
        }
    }

    #[test]
    fn truncate_zero_width_is_just_ellipsis() {
        assert_eq!(truncate_for_preview("xxxx", 0), "…");
    }

    fn runtime_with(folder: &Path) -> Arc<WorkflowRuntime> {
        let cfg = AgentConfig {
            working_folder: Some(folder.to_path_buf()),
            ..Default::default()
        };
        Arc::new(WorkflowRuntime::new(cfg))
    }

    #[test]
    fn runtime_paths_match_loom_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        assert_eq!(
            rt.instances_root(),
            tmp.path().join(".loom").join("instances")
        );
        assert_eq!(rt.runs_root(), tmp.path().join(".luft").join("runs"));
        assert_eq!(
            rt.workflows_dir(),
            tmp.path().join(".loom").join("workflows")
        );
        assert_eq!(
            rt.loom_instance_dir("loom-instance_abc"),
            tmp.path()
                .join(".loom")
                .join("instances")
                .join("loom-instance_abc")
        );
    }

    #[test]
    fn sanitize_strips_internal_file_refs() {
        let raw = json!({
            "schema_version": 1,
            "instance_id": "run-1",
            "instance_dir": "loom-instance_1",
            "workflow": {"kind": "file", "name": "wf", "path": "/abs/path/wf.lua"},
            "agents": [
                {"agent_id": "a", "output_ref": "agent-outputs/a.txt", "output_size": 4096}
            ],
            "report": {"ref": "report.json", "preview": "hello", "value_type": "object", "size_bytes": 4096},
            "checkpoint_hash": "deadbeef",
        });
        let cleaned = sanitize_instance_for_public(raw);
        assert!(cleaned["workflow"].get("path").is_none());
        assert!(cleaned["agents"][0].get("output_ref").is_none());
        assert!(cleaned["report"].get("ref").is_none());
        assert_eq!(cleaned["report"]["preview"].as_str().unwrap(), "hello");
        assert!(cleaned.get("checkpoint_hash").is_none());
    }

    #[test]
    fn sanitize_keeps_inline_report_content() {
        let raw = json!({
            "schema_version": 1,
            "report": {"ok": true, "verdict": "approved"},
            "workflow": {"kind": "inline", "name": "script"},
        });
        let cleaned = sanitize_instance_for_public(raw);
        assert_eq!(cleaned["report"]["ok"], true);
        assert_eq!(cleaned["report"]["verdict"], "approved");
    }

    #[test]
    fn six_tool_names_and_specs_match_constants() {
        let cfg = AgentConfig::default();
        let start = WorkflowStartTool::new(cfg.clone());
        let status = WorkflowStatusTool::new(cfg.clone());
        let list = WorkflowListTool::new(cfg.clone());
        let events = WorkflowEventsTool::new(cfg.clone());
        let source = WorkflowSourceTool::new(cfg.clone());
        let files = WorkflowFilesTool::new(cfg);

        assert_eq!(start.name(), TOOL_WORKFLOW_START);
        assert_eq!(status.name(), TOOL_WORKFLOW_STATUS);
        assert_eq!(list.name(), TOOL_WORKFLOW_LIST);
        assert_eq!(events.name(), TOOL_WORKFLOW_EVENTS);
        assert_eq!(source.name(), TOOL_WORKFLOW_SOURCE);
        assert_eq!(files.name(), TOOL_WORKFLOW_FILES);

        assert_eq!(start.spec().name, TOOL_WORKFLOW_START);
        assert_eq!(status.spec().name, TOOL_WORKFLOW_STATUS);
        assert_eq!(list.spec().name, TOOL_WORKFLOW_LIST);
        assert_eq!(events.spec().name, TOOL_WORKFLOW_EVENTS);
        assert_eq!(source.spec().name, TOOL_WORKFLOW_SOURCE);
        assert_eq!(files.spec().name, TOOL_WORKFLOW_FILES);

        for tool in [
            &start as &dyn Tool,
            &status,
            &list,
            &events,
            &source,
            &files,
        ] {
            let hint = tool
                .spec()
                .output_hint
                .as_ref()
                .and_then(|h| h.preferred_strategy)
                .expect("output_hint must be set");
            assert!(matches!(hint, ToolOutputStrategy::Inline));
        }
    }

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
        let tool = WorkflowStatusTool {
            runtime: rt.clone(),
        };
        let result = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap();
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
        let tool = WorkflowStatusTool { runtime: rt };
        let result = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["status"], "running");
    }

    #[test]
    fn status_errors_on_legacy_dir_without_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "legacy-blank";
        let dir_path = tmp.path().join(".luft").join("runs").join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();

        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool { runtime: rt };
        let err = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("corrupt") || msg.contains("missing checkpoint"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn status_errors_on_unknown_instance_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool { runtime: rt };
        let err = block_on(tool.call(json!({"instance_dir": "does-not-exist"}), None)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found"));
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(f)
    }
}
