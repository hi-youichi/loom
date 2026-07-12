use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use luft_core::contract::backend::{
    AgentBackend, AgentCapabilities, AgentResult, AgentStatus, BackendError, AgentTask, LogRef,
    RunContext,
};
use luft_core::contract::event::AgentEvent as LuftAgentEvent;
use luft_core::contract::ids::TokenUsage;
use serde_json::Value;

use agent::agent::{Agent, AgentConfig, AgentEvent as LoomAgentEvent};
use tool_core::Tool;

use crate::event_bridge::map_loom_event_to_delta;
use crate::structured_output::StructuredOutputTool;

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
            structured_output: true,
            models: vec![],
        }
    }

    async fn run(
        &self,
        task: AgentTask,
        ctx: RunContext,
    ) -> Result<AgentResult, BackendError> {
        let mut config = self.config_template.clone();

        if let Some(ref model) = task.model {
            config.model = Some(model.clone());
        }

        config.working_folder = Some(task.workdir.clone());

        let output_slot = Arc::new(Mutex::new(None::<Value>));

        if let Some(ref schema) = task.output_schema {
            let tool: Arc<dyn Tool> = Arc::new(StructuredOutputTool::new(
                schema.clone(),
                output_slot.clone(),
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
            config.builtin_tool_filter = Some(filter);
        }

        let agent = Agent::from_config(config)
            .await
            .map_err(|e| BackendError::Execution(format!("agent build failed: {e}")))?;

        let tokens = Arc::new(Mutex::new(TokenUsage::default()));
        let event_sender = ctx.events.clone();
        let agent_id = task.agent_id;
        let run_id = ctx.run_id;
        let slot = output_slot.clone();

        let run = tokio::select! {
            result = agent.run(&task.prompt, {
                let tokens = tokens.clone();
                let event_sender = event_sender.clone();
                move |ev: LoomAgentEvent| {
                    match &ev {
                        LoomAgentEvent::Usage {
                            prompt_tokens,
                            completion_tokens,
                            cached_tokens,
                            ..
                        } => {
                            let mut t = tokens.lock().unwrap();
                            t.input += *prompt_tokens as u64;
                            t.output += *completion_tokens as u64;
                            if let Some(ct) = cached_tokens {
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
                }
            }) => result,
            _ = ctx.cancel.cancelled() => {
                return Ok(AgentResult {
                    agent_id: task.agent_id,
                    status: AgentStatus::Cancelled,
                    output: Value::Null,
                    findings: vec![],
                    tokens_used: *tokens.lock().unwrap(),
                    artifacts: vec![],
                    logs: LogRef::default(),
                });
            }
        };

        let result =
            run.map_err(|e| BackendError::Execution(format!("agent run failed: {e}")))?;

        let output = slot
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Value::String(result.reply.clone()));

        let tokens_used = *tokens.lock().unwrap();

        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output,
            findings: vec![],
            tokens_used,
            artifacts: vec![],
            logs: LogRef::default(),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
