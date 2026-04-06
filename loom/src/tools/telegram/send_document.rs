//! Send document tool for Telegram.
//!
//! Sends files (images, documents, etc.) to a Telegram chat with an optional caption.
//! Requires Telegram API to be initialized and `chat_id` in context or parameters.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

use super::{get_current_chat_id, get_telegram_api};

pub const TOOL_TELEGRAM_SEND_DOCUMENT: &str = "telegram_send_document";

#[derive(Debug, Deserialize)]
pub struct SendDocumentParams {
    pub chat_id: Option<i64>,
    pub file_path: String,
    pub caption: Option<String>,
}

pub struct TelegramSendDocumentTool;

#[async_trait]
impl Tool for TelegramSendDocumentTool {
    fn name(&self) -> &str {
        TOOL_TELEGRAM_SEND_DOCUMENT
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TELEGRAM_SEND_DOCUMENT.to_string(),
            description: Some("Send a file (document, image, etc.) to a Telegram chat with an optional caption.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (optional, defaults to current chat)"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Optional caption for the document"
                    }
                },
                "required": ["file_path"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let params: SendDocumentParams = serde_json::from_value(args)
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

        let message_id = api
            .send_document(chat_id, &params.file_path, params.caption.as_deref())
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to send document: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Document sent successfully (message_id: {})",
            message_id
        )))
    }
}
