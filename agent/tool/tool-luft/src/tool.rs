use async_trait::async_trait;
use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::backend::LoomAgentBackend;
use crate::event_bridge::luft_event_to_json;
use crate::workflow_resolver::resolve_workflow;

pub struct LuftTool {
    config_template: agent::agent::AgentConfig,
}

impl LuftTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self { config_template }
    }

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
            .join(".luft")
            .join("workflows")
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

    async fn handle_list_runs(&self) -> Result<ToolCallContent, ToolSourceError> {
        let dir = self.runs_dir();
        let mut runs = Vec::new();

        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    let checkpoint_path = path.join("checkpoint.json");
                    let checkpoint: Value = std::fs::read(&checkpoint_path)
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .unwrap_or(json!(null));

                    let run_id = checkpoint
                        .get("run_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string();
                    let status = checkpoint
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let created_at = checkpoint
                        .get("created_at")
                        .and_then(|v| v.as_u64());
                    let total_tokens = checkpoint
                        .get("total_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let agent_count = checkpoint
                        .get("agent_results")
                        .and_then(|v| v.as_object())
                        .map(|o| o.len())
                        .unwrap_or(0);

                    let dir_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();

                    runs.push(json!({
                        "run_dir": dir_name,
                        "run_id": run_id,
                        "status": status,
                        "created_at": created_at,
                        "total_tokens": total_tokens,
                        "agents": agent_count,
                    }));
                }
            }
        }

        runs.sort_by(|a, b| {
            let av = a.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
            let bv = b.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
            bv.cmp(&av)
        });

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json!({
                "runs": runs,
                "count": runs.len(),
            }))
            .unwrap_or_default(),
        ))
    }

    async fn handle_run_status(
        &self,
        run_dir: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path = self.runs_dir().join(run_dir);

        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "Run directory '{run_dir}' not found in {}",
                self.runs_dir().display()
            )));
        }

        let checkpoint: Value = std::fs::read_to_string(path.join("checkpoint.json"))
            .map_err(|e| ToolSourceError::ToolError(format!("Failed to read checkpoint: {e}")))
            .and_then(|s| {
                serde_json::from_str(&s)
                    .map_err(|e| ToolSourceError::ToolError(format!("Invalid checkpoint JSON: {e}")))
            })?;

        let events: Vec<Value> = std::fs::read_to_string(path.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        let workflow_src = std::fs::read_to_string(path.join("workflow.lua")).ok();

        let result = json!({
            "checkpoint": checkpoint,
            "events": events,
            "event_count": events.len(),
            "workflow_source": workflow_src,
        });

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    async fn handle_run(
        &self,
        args: &Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let depth = ctx.map(|c| c.depth).unwrap_or(0);
        if depth >= 3 {
            return Err(ToolSourceError::ToolError(
                "Luft workflow nesting depth exceeded (max 3).".to_string(),
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

                let source = std::fs::read_to_string(&path)
                    .map_err(|e| ToolSourceError::ToolError(format!("Failed to read workflow: {e}")))?;

                (source, path.display().to_string())
            }
            (None, None) => {
                return Err(ToolSourceError::InvalidInput(
                    "Either 'script' or 'workflow' must be provided.".to_string(),
                ));
            }
        };

        let base_dir = self.runs_dir();
        let backend = LoomAgentBackend::new(self.config_template.clone());

        let luft = LuftBuilder::new()
            .backend(backend)
            .base_dir(&base_dir)
            .concurrency(4)
            .build()
            .map_err(|e| ToolSourceError::ToolError(format!("Luft build failed: {e}")))?;

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

        let mut cancelled = false;
        loop {
            if cancelled {
                match done_rx.recv().await {
                    Ok(LuftAgentEvent::RunDone { .. }) => {
                        return Ok(ToolCallContent::Text("Workflow cancelled.".to_string()));
                    }
                    Ok(_) => continue,
                    Err(_) => {
                        return Ok(ToolCallContent::Text(
                            "Workflow cancelled (event channel closed).".to_string(),
                        ));
                    }
                }
            }

            tokio::select! {
                ev = done_rx.recv() => {
                    match ev {
                        Ok(LuftAgentEvent::RunDone { report, status, total_tokens, .. }) => {
                            let text = match &report {
                                Value::Null | Value::Bool(_) => {
                                    let mut obj = json!({
                                        "status": format!("{:?}", status),
                                        "workflow": display_name,
                                        "tokens": total_tokens,
                                    });
                                    if matches!(status, luft_core::contract::event::RunStatus::Failed) {
                                        obj["error"] = json!("Workflow failed. Use action='run-status' with the latest run_dir to see details.");
                                    }
                                    serde_json::to_string_pretty(&obj).unwrap_or_default()
                                }
                                _ => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            };
                            return Ok(ToolCallContent::Text(text));
                        }
                        Ok(_) => {}
                        Err(_) => {
                            return Err(ToolSourceError::ToolError(
                                "Luft event channel closed unexpectedly.".to_string(),
                            ));
                        }
                    }
                }
                _ = cancel_token.cancelled(), if !cancelled => {
                    cancelled = true;
                    run_handle.cancel();
                }
            }
        }
    }
}

#[async_trait]
impl Tool for LuftTool {
    fn name(&self) -> &str {
        "luft"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "luft".to_string(),
            description: Some(
                "Execute or inspect multi-agent workflows using the Luft engine.\n\
                 Actions:\n\
                 - run (default): Execute a workflow. Provide `script` (inline Lua) or `workflow` (name/path).\n\
                 - list-workflows: List available .lua workflow files.\n\
                 - list-runs: List past workflow execution runs with status.\n\
                 - run-status: Query detailed status of a specific run (checkpoint + events).\n\n\
                 Lua primitives: agent(opts), parallel(items, mapFn), \
                 pipeline{items=, stages=, max_inflight=}, phase(name, planned?), \
                 phase_begin(name), phase_end(span), workflow(path, args?), \
                 report(value), log(msg, level?), budget(time_ms, rounds), \
                 json.encode(value), json.decode(string)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["run", "list-workflows", "list-runs", "run-status"],
                        "description": "Action to perform. Default: 'run'.",
                        "default": "run"
                    },
                    "script": {
                        "type": "string",
                        "description": "(action=run) Inline Lua script."
                    },
                    "workflow": {
                        "type": "string",
                        "description": "(action=run) Name or path of a .lua workflow file."
                    },
                    "args": {
                        "type": "object",
                        "description": "(action=run) Arguments passed to the workflow's main(args) function.",
                        "additionalProperties": true
                    },
                    "run_dir": {
                        "type": "string",
                        "description": "(action=run-status) Run directory name to query (from list-runs)."
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
            .and_then(|v| v.as_str())
            .unwrap_or("run");

        match action {
            "run" => self.handle_run(&args, ctx).await,
            "list-workflows" => self.handle_list_workflows().await,
            "list-runs" => self.handle_list_runs().await,
            "run-status" => {
                let run_dir = args
                    .get("run_dir")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolSourceError::InvalidInput(
                            "'run_dir' is required for action='run-status'.".to_string(),
                        )
                    })?;
                self.handle_run_status(run_dir).await
            }
            other => Err(ToolSourceError::InvalidInput(format!(
                "Unknown action '{other}'. Valid: run, list-workflows, list-runs, run-status."
            ))),
        }
    }
}
