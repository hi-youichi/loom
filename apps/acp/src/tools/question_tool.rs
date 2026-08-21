//! Structured user-question tool exposed to Loom agents over ACP.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use uuid::Uuid;

use crate::extensions::question::{QuestionChoice, QuestionRequest};

use super::{create_tool_spec, ClientBridgeTrait};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskQuestionArgs {
    #[serde(default)]
    question_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    prompt: String,
    #[serde(default)]
    choices: Vec<QuestionChoice>,
    #[serde(default)]
    allow_free_text: bool,
    #[serde(default)]
    free_text_placeholder: Option<String>,
    #[serde(default)]
    default_choice: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

pub struct AskQuestionTool {
    bridge: Arc<dyn ClientBridgeTrait>,
}

impl AskQuestionTool {
    pub fn with_bridge(bridge: Arc<dyn ClientBridgeTrait>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for AskQuestionTool {
    fn name(&self) -> &str {
        "ask_user_question"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "ask_user_question",
            "Ask the user a structured question and wait for their answer. Use this when a decision or missing input is required before continuing.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "questionId": { "type": "string", "description": "Optional unique id; generated when omitted" },
                    "title": { "type": "string" },
                    "prompt": { "type": "string" },
                    "choices": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "value": { "type": "string" },
                                "label": { "type": "string" },
                                "description": { "type": "string" },
                                "disabled": { "type": "boolean" }
                            },
                            "required": ["value", "label"]
                        }
                    },
                    "allowFreeText": { "type": "boolean" },
                    "freeTextPlaceholder": { "type": "string" },
                    "defaultChoice": { "type": "string" },
                    "timeoutMs": { "type": "integer", "minimum": 1 }
                },
                "required": ["prompt"]
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let args: AskQuestionArgs = serde_json::from_value(args).map_err(|error| {
            ToolSourceError::InvalidInput(format!("Invalid arguments: {error}"))
        })?;
        let request = QuestionRequest {
            question_id: args
                .question_id
                .unwrap_or_else(|| format!("agent-question-{}", Uuid::new_v4())),
            title: args.title,
            prompt: args.prompt,
            choices: args.choices,
            allow_free_text: args.allow_free_text,
            free_text_placeholder: args.free_text_placeholder,
            default_choice: args.default_choice,
            timeout_ms: args.timeout_ms,
            session_id: None,
        };
        let reply = self
            .bridge
            .ask_question(request)
            .await
            .map_err(ToolSourceError::Transport)?;
        let output = serde_json::to_string(&reply).map_err(|error| {
            ToolSourceError::ToolError(format!("Failed to serialize question reply: {error}"))
        })?;
        Ok(ToolCallContent::text(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_spec_requires_prompt() {
        let tool = AskQuestionTool::with_bridge(Arc::new(super::super::NoOpClientBridge));
        let spec = tool.spec();
        assert_eq!(spec.name, "ask_user_question");
        assert_eq!(spec.input_schema["required"], serde_json::json!(["prompt"]));
    }
}
