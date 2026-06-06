//! Send poll tool for Telegram.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

use super::{get_current_chat_id, get_telegram_api};

/// Tool name for sending polls.
pub const TOOL_TELEGRAM_SEND_POLL: &str = "telegram_send_poll";

/// Parameters for send_poll tool.
#[derive(Debug, Deserialize)]
pub struct SendPollParams {
    /// Target chat ID (defaults to current chat)
    pub chat_id: Option<i64>,
    /// Poll question
    pub question: String,
    /// List of poll options (2-10 options)
    pub options: Vec<String>,
    /// Whether the poll is anonymous (default: true)
    pub is_anonymous: Option<bool>,
    /// Whether multiple answers are allowed (default: false)
    pub allows_multiple_answers: Option<bool>,
}

/// Tool for sending polls via Telegram.
pub struct TelegramSendPollTool;

#[async_trait]
impl Tool for TelegramSendPollTool {
    fn name(&self) -> &str {
        TOOL_TELEGRAM_SEND_POLL
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TELEGRAM_SEND_POLL.to_string(),
            description: Some("Send a poll to a Telegram chat. Use this to gather opinions or let users vote on options.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (optional, defaults to current chat)"
                    },
                    "question": {
                        "type": "string",
                        "description": "Poll question"
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 2,
                        "maxItems": 10,
                        "description": "List of poll options (2-10 options)"
                    },
                    "is_anonymous": {
                        "type": "boolean",
                        "description": "Whether the poll is anonymous (default: true)"
                    },
                    "allows_multiple_answers": {
                        "type": "boolean",
                        "description": "Whether multiple answers are allowed (default: false)"
                    }
                },
                "required": ["question", "options"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let params: SendPollParams = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        if params.options.len() < 2 {
            return Err(ToolSourceError::InvalidInput(
                "Poll must have at least 2 options".to_string(),
            ));
        }
        if params.options.len() > 10 {
            return Err(ToolSourceError::InvalidInput(
                "Poll cannot have more than 10 options".to_string(),
            ));
        }

        let api = get_telegram_api().ok_or_else(|| {
            ToolSourceError::Transport("Telegram API not initialized".to_string())
        })?;

        let chat_id = params
            .chat_id
            .unwrap_or_else(|| get_current_chat_id().unwrap_or(0));

        if chat_id == 0 {
            return Err(ToolSourceError::InvalidInput(
                "No chat_id provided and no current chat context".to_string(),
            ));
        }

        let poll_id = api
            .send_poll(
                chat_id,
                &params.question,
                params.options,
                params.is_anonymous.unwrap_or(true),
                params.allows_multiple_answers.unwrap_or(false),
            )
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to send poll: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Poll sent successfully (poll_id: {})",
            poll_id
        )))
    }
}
