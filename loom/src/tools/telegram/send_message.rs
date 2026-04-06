//! Send message tool for Telegram.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

use super::{get_current_chat_id, get_telegram_api};

/// Tool name for sending messages.
pub const TOOL_TELEGRAM_SEND_MESSAGE: &str = "telegram_send_message";

/// Parameters for send_message tool.
#[derive(Debug, Deserialize)]
pub struct SendMessageParams {
    /// Target chat ID (defaults to current chat)
    pub chat_id: Option<i64>,
    /// Message text (supports Markdown/HTML based on parse_mode)
    pub text: String,
    /// Parse mode: "MarkdownV2", "HTML", or None for plain text
    pub parse_mode: Option<String>,
}

/// Tool for sending text messages via Telegram.
pub struct TelegramSendMessageTool;

#[async_trait]
impl Tool for TelegramSendMessageTool {
    fn name(&self) -> &str {
        TOOL_TELEGRAM_SEND_MESSAGE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TELEGRAM_SEND_MESSAGE.to_string(),
            description: Some("Send a text message to a Telegram chat. Use this to proactively send messages, notifications, or updates.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (optional, defaults to current chat)"
                    },
                    "text": {
                        "type": "string",
                        "description": "Message text to send"
                    },
                    "parse_mode": {
                        "type": "string",
                        "enum": ["MarkdownV2", "HTML"],
                        "description": "Parse mode for formatting (optional)"
                    }
                },
                "required": ["text"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let params: SendMessageParams = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let api = get_telegram_api()
            .ok_or_else(|| ToolSourceError::Transport("Telegram API not initialized".to_string()))?;

        let chat_id = params.chat_id.unwrap_or_else(|| {
            get_current_chat_id().unwrap_or(0)
        });

        if chat_id == 0 {
            return Err(ToolSourceError::InvalidInput(
                "No chat_id provided and no current chat context".to_string(),
            ));
        }

        let parse_mode = params.parse_mode.as_deref();
        let message_id = api
            .send_message(chat_id, &params.text, parse_mode)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to send message: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Message sent successfully (message_id: {})",
            message_id
        )))
    }
}
