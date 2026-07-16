use async_trait::async_trait;
use luft::LuftBuilder;
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use serde_json::{json, Value};
use std::future::IntoFuture;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use tool_core::tool_name::{TOOL_WORKFLOW_START, TOOL_WORKFLOW_STATUS};
use tool_core::{
    BuiltinSkill, Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy,
    ToolSourceError, ToolSpec,
};

use crate::backend::LoomAgentBackend;
use crate::event_bridge::luft_event_to_json;
use crate::runtime::WorkflowRuntime;
use crate::workflow_resolver::resolve_workflow;

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

pub struct WorkflowStartTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowStartTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}