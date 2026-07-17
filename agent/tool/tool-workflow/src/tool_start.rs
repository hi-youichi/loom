use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::{TOOL_WORKFLOW_START, TOOL_WORKFLOW_STATUS};
use tool_core::{
    BuiltinSkill, Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy,
    ToolSourceError, ToolSpec,
};

use crate::runtime::WorkflowRuntime;

const WORKFLOW_SKILL: &str = include_str!("workflow_skill.md");
const REF_TOOL_USAGE: &str = include_str!("references/tool-usage.md");
const REF_DSL_REFERENCE: &str = include_str!("references/dsl-reference.md");
const REF_ARCH_HEADER: &str = include_str!("references/architecture-header.md");
const REF_AGENT_PROMPTS: &str = include_str!("references/agent-prompts.md");
const REF_DECOMPOSITION: &str = include_str!("references/task-decomposition.md");
const REF_ADVERSARIAL: &str = include_str!("references/adversarial-verification.md");
const REF_EXAMPLES: &str = include_str!("references/examples.md");

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
        crate::service::start_workflow(&self.runtime, args, ctx).await
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
