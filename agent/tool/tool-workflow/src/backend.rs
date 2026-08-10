use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use luft_core::contract::backend::{
    AgentBackend, AgentCapabilities, AgentResult, AgentStatus, AgentTask, BackendError, LogRef,
    RunContext,
};
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use luft_core::contract::ids::TokenUsage;
use serde_json::Value;

use agent::agent::{Agent, AgentConfig, AgentEvent as LoomAgentEvent};
use tool_core::Tool;

use crate::event_bridge::map_loom_event_to_delta;
use crate::workflow_validate_schema::WorkflowValidateSchemaTool;

const POST_SUBMISSION_GRACE: Duration = Duration::from_secs(10);

pub struct LoomAgentBackend {
    config_template: AgentConfig,
}

impl LoomAgentBackend {
    pub fn new(config_template: AgentConfig) -> Self {
        Self { config_template }
    }
}

#[async_trait]
impl AgentBackend for LoomAgentBackend {
    fn id(&self) -> &'static str {
        "loom"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            streaming: true,
            mcp_injection: true,
            workflow_validate_schema: true,
            session_resume: true,
            models: vec![],
        }
    }

    async fn run(&self, task: AgentTask, ctx: RunContext) -> Result<AgentResult, BackendError> {
        tracing::info!(
            target: "workflow::backend",
            agent_id = %task.agent_id,
            workdir = %task.workdir.display(),
            has_output_schema = task.output_schema.is_some(),
            has_allowlist = task.allowlist.is_some(),
            "agent task starting",
        );

        let mut config = self.config_template.clone();

        let session_id = task.session_id.clone();
        if session_id.is_some() {
            config.thread_id = session_id.clone();
            config.resume_mode = true;
            tracing::debug!(
                target: "workflow::backend",
                agent_id = %task.agent_id,
                session_id = ?session_id,
                "resuming from prior session",
            );
        }

        if let Some(ref model) = task.model {
            config.model = Some(model.clone());
            tracing::debug!(
                target: "workflow::backend",
                agent_id = %task.agent_id,
                model = %model,
                "using overridden model",
            );
        }

        config.working_folder = Some(
            task.workdir_override
                .clone()
                .unwrap_or_else(|| task.workdir.clone()),
        );

        let output_slot = Arc::new(Mutex::new(None::<Value>));
        let submit_notify = Arc::new(tokio::sync::Notify::new());

        if let Some(ref schema) = task.output_schema {
            tracing::debug!(
                target: "workflow::backend",
                agent_id = %task.agent_id,
                "injecting workflow_validate_schema tool with output schema",
            );
            let tool: Arc<dyn Tool> = Arc::new(WorkflowValidateSchemaTool::new(
                schema.clone(),
                output_slot.clone(),
                submit_notify.clone(),
            ));
            let mut tools: Vec<Arc<dyn Tool>> = vec![tool];
            if let Some(ref existing) = config.extra_tools {
                tools.extend(existing.iter().cloned());
            }
            config.extra_tools = Some(Arc::new(tools));
        }

        if let Some(ref allowlist) = task.allowlist {
            let filter = tool_core::BuiltinToolFilter {
                enabled: if allowlist.allow_commands.is_empty() {
                    None
                } else {
                    Some(allowlist.allow_commands.clone())
                },
                disabled: if allowlist.deny.is_empty() {
                    None
                } else {
                    Some(allowlist.deny.clone())
                },
            };
            tracing::debug!(
                target: "workflow::backend",
                agent_id = %task.agent_id,
                allow = ?allowlist.allow_commands,
                deny = ?allowlist.deny,
                "applying builtin tool filter",
            );
            config.builtin_tool_filter = Some(filter);
        }

        tracing::debug!(
            target: "workflow::backend",
            agent_id = %task.agent_id,
            "building agent from config",
        );
        let agent = Agent::from_config(config).await.map_err(|e| {
            tracing::error!(
                target: "workflow::backend",
                agent_id = %task.agent_id,
                error = %e,
                "agent build failed",
            );
            BackendError::Execution(format!("agent build failed: {e}"))
        })?;

        tracing::info!(
            target: "workflow::backend",
            agent_id = %task.agent_id,
            "agent built successfully, starting run",
        );

        let tokens = Arc::new(Mutex::new(TokenUsage::default()));
        let event_sender = ctx.events.clone();
        let agent_id = task.agent_id;
        let run_id = ctx.run_id;
        let slot = output_slot.clone();

        let prompt = task.prompt.clone();
        let callback = {
            let tokens = tokens.clone();
            let event_sender = event_sender.clone();
            move |ev: LoomAgentEvent| match &ev {
                LoomAgentEvent::Usage {
                    input,
                    output,
                    cache_read,
                    ..
                } => {
                    let mut t = tokens.lock().unwrap();
                    t.input += *input as u64;
                    t.output += *output as u64;
                    if let Some(ct) = cache_read {
                        t.cache_read += *ct as u64;
                    }
                }
                _ => {
                    if let Some(delta) = map_loom_event_to_delta(&ev) {
                        let _ = event_sender.send(LuftAgentEvent::AgentProgress {
                            run_id,
                            agent_id,
                            delta,
                        });
                    }
                }
            }
        };

        let mut run_handle = tokio::spawn(async move { agent.run(&prompt, callback).await });

        let has_schema = task.output_schema.is_some();
        #[allow(clippy::never_loop)]
        let run = loop {
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    run_handle.abort();
                    return Ok(AgentResult {
                        agent_id: task.agent_id,
                        status: AgentStatus::Cancelled,
                        output: Value::Null,
                        findings: vec![],
                        tokens_used: *tokens.lock().unwrap(),
                        artifacts: vec![],
                        logs: LogRef::default(),
                        session_id,
                    });
                }
                result = &mut run_handle => {
                    break match result {
                        Ok(r) => r,
                        Err(join_err) => {
                            let msg = if join_err.is_panic() {
                                let panic = join_err.into_panic();
                                if let Some(s) = panic.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "unknown panic payload".to_string()
                                }
                            } else {
                                "task was cancelled".to_string()
                            };
                            tracing::error!(
                                target: "workflow::backend",
                                agent_id = %task.agent_id,
                                msg = %msg,
                                "agent task panicked; converting to AgentError"
                            );
                            Err(agent::agent::AgentError::Run(format!("agent panicked: {msg}")))
                        }
                    };
                }
                _ = submit_notify.notified(), if has_schema => {
                    tracing::debug!(
                        target: "workflow::backend",
                        agent_id = %task.agent_id,
                        grace_secs = POST_SUBMISSION_GRACE.as_secs(),
                        "workflow_validate_schema captured, entering grace period",
                    );
                    tokio::select! {
                        biased;
                        _ = ctx.cancel.cancelled() => {
                            run_handle.abort();
                            return Ok(AgentResult {
                                agent_id: task.agent_id,
                                status: AgentStatus::Cancelled,
                                output: Value::Null,
                                findings: vec![],
                                tokens_used: *tokens.lock().unwrap(),
                                artifacts: vec![],
                                logs: LogRef::default(),
                                session_id,
                            });
                        }
                        result = &mut run_handle => {
                            break match result {
                                Ok(r) => r,
                                Err(join_err) => {
                                    let msg = if join_err.is_panic() {
                                        let panic = join_err.into_panic();
                                        if let Some(s) = panic.downcast_ref::<&str>() {
                                            s.to_string()
                                        } else if let Some(s) = panic.downcast_ref::<String>() {
                                            s.clone()
                                        } else {
                                            "unknown panic payload".to_string()
                                        }
                                    } else {
                                        "task was cancelled".to_string()
                                    };
                                    Err(agent::agent::AgentError::Run(format!("agent panicked: {msg}")))
                                }
                            };
                        }
                        _ = tokio::time::sleep(POST_SUBMISSION_GRACE) => {
                            tracing::warn!(
                                target: "workflow::backend",
                                agent_id = %task.agent_id,
                                "agent did not stop after workflow_validate_schema, aborting",
                            );
                            run_handle.abort();
                            let _ = (&mut run_handle).await;
                            break Err(agent::agent::AgentError::Run(
                                "agent aborted: did not stop after workflow_validate_schema".to_string(),
                            ));
                        }
                    }
                }
            }
        };

        let slot_output = slot.lock().unwrap().take();

        let output = finalize_output(run, slot_output)?;

        let tokens_used = *tokens.lock().unwrap();

        tracing::info!(
            target: "workflow::backend",
            agent_id = %task.agent_id,
            input_tokens = tokens_used.input,
            output_tokens = tokens_used.output,
            cache_read_tokens = tokens_used.cache_read,
            has_structured_output = output.get("_agent_fallback_text").is_none(),
            "agent task completed",
        );

        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output,
            findings: vec![],
            tokens_used,
            artifacts: vec![],
            logs: LogRef::default(),
            session_id,
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Decide the final `output` value after an agent run.
///
/// - Ok + slot filled  → slot value (structured_output takes priority)
/// - Ok + slot empty    → agent reply text wrapped in `_agent_fallback_text` envelope
/// - Err + slot filled  → slot value (salvage: agent crashed *after* capturing structured output)
/// - Err + slot empty   → propagate error
fn finalize_output(
    run_result: Result<agent::agent::AgentResult, agent::agent::AgentError>,
    slot_output: Option<Value>,
) -> Result<Value, BackendError> {
    match (run_result, slot_output) {
        (Ok(_result), Some(slot)) => Ok(slot),
        (Ok(result), None) => Ok(serde_json::json!({
            "_agent_fallback_text": true,
            "text": result.reply,
        })),
        (Err(e), Some(slot)) => {
            tracing::warn!(
                target: "workflow::backend",
                "agent run failed but structured_output was captured, salvaging: {e}"
            );
            Ok(slot)
        }
        (Err(e), None) => Err(BackendError::Execution(format!("agent run failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::{AgentError, AgentResult as LoomAgentResult};
    use serde_json::json;

    #[test]
    fn finalize_ok_with_slot_prefers_slot() {
        let result = LoomAgentResult {
            reply: "agent reply text".into(),
            reasoning: None,
        };
        let slot = Some(json!({ "changed": true, "summary": "done" }));

        let output = finalize_output(Ok(result), slot).unwrap();
        assert_eq!(output, json!({ "changed": true, "summary": "done" }));
    }

    #[test]
    fn finalize_ok_without_slot_uses_fallback_envelope() {
        let result = LoomAgentResult {
            reply: "plain text reply".into(),
            reasoning: None,
        };

        let output = finalize_output(Ok(result), None).unwrap();
        assert_eq!(
            output,
            json!({ "_agent_fallback_text": true, "text": "plain text reply" })
        );
    }

    #[test]
    fn finalize_err_with_slot_salvages() {
        let err = AgentError::Run("LLM timed out on follow-up call".into());
        let slot = Some(json!({ "changed": true, "files": ["a.rs"], "summary": "ok" }));

        let output = finalize_output(Err(err), slot).unwrap();
        assert_eq!(
            output,
            json!({ "changed": true, "files": ["a.rs"], "summary": "ok" })
        );
    }

    #[test]
    fn finalize_err_without_slot_propagates() {
        let err = AgentError::Run("total failure".into());

        let result = finalize_output(Err(err), None);
        assert!(result.is_err());
        match result.unwrap_err() {
            BackendError::Execution(msg) => assert!(msg.contains("total failure")),
            other => panic!("expected BackendError::Execution, got {other:?}"),
        }
    }
}
