//! Request building for OpenAI Chat Completions.

use async_openai::types::chat::{
    CreateChatCompletionRequest,
    ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage,
    ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestToolMessage,
    ChatCompletionToolChoiceOption,
};
use serde_json::Value;

use crate::types::message::{Message, UserContent, ToolCallContent};
use crate::traits::ToolChoiceMode;
use crate::tool_source::ToolSpec;

/// Build a chat completion request from messages and configuration.
pub fn build_chat_request(
    model: &str,
    messages: &[Message],
    tools: Option<&[ToolSpec]>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    stream: bool,
) -> Result<CreateChatCompletionRequest, crate::types::error::LlmError> {
    let mut request_messages: Vec<ChatCompletionRequestMessage> = Vec::with_capacity(messages.len());
    
    for msg in messages {
        let chat_msg = convert_message(msg)?;
        request_messages.push(chat_msg);
    }

    let mut request = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: request_messages,
        stream: Some(stream),
        ..Default::default()
    };

    if let Some(temp) = temperature {
        request.temperature = Some(temp);
    }

    if let Some(tools) = tools {
        if !tools.is_empty() {
            let tool_definitions: Vec<_> = tools.iter().map(|t| {
                serde_json::to_value(t).unwrap_or_default()
            }).collect();
            let tools_value: Value = serde_json::json!({
                "tools": tool_definitions
            });
            request.tools = Some(serde_json::from_value(tools_value).unwrap_or_default());
        }
    }

    if let Some(mode) = tool_choice {
        let choice = match mode {
            ToolChoiceMode::Auto => ChatCompletionToolChoiceOption::Auto,
            ToolChoiceMode::None => ChatCompletionToolChoiceOption::None,
            ToolChoiceMode::Required => ChatCompletionToolChoiceOption::Named(serde_json::json!({"type": "function", "function": {"name": ""}})),
        };
        request.tool_choice = Some(choice);
    }

    Ok(request)
}

fn convert_message(msg: &Message) -> Result<ChatCompletionRequestMessage, crate::types::error::LlmError> {
    match msg {
        Message::System(content) => {
            Ok(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    content: content.clone().into(),
                }
            ))
        }
        Message::User(content) => {
            let user_content = match content {
                UserContent::Text(s) => s.clone().into(),
                UserContent::Multimodal(parts) => {
                    let mut content_parts = Vec::new();
                    for part in parts {
                        match part {
                            crate::types::message::ContentPart::Text { text } => {
                                content_parts.push(serde_json::json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                            crate::types::message::ContentPart::ImageUrl { url, detail } => {
                                content_parts.push(serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": url,
                                        "detail": detail.as_deref().unwrap_or("auto")
                                    }
                                }));
                            }
                            _ => {
                                // Skip unsupported content types for now
                            }
                        }
                    }
                    serde_json::json!(content_parts).to_string().into()
                }
            };
            Ok(ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: user_content,
                }
            ))
        }
        Message::Assistant(payload) => {
            let mut content = Some(payload.content.clone().into());
            let mut tool_calls: Option<Vec<ChatCompletionToolChoiceOption>> = None;
            
            if !payload.tool_calls.is_empty() {
                let calls: Vec<_> = payload.tool_calls.iter().map(|tc| {
                    ChatCompletionToolChoiceOption::Named(serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments
                        }
                    }))
                }).collect();
                tool_calls = Some(calls);
                // Clear content if there are tool calls
                if !payload.content.is_empty() {
                    content = Some(payload.content.clone().into());
                }
            }
            
            Ok(ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content,
                    tool_calls,
                    ..Default::default()
                }
            ))
        }
        Message::Tool { tool_call_id, content } => {
            let tool_content = match content {
                ToolCallContent::Text(s) => s.clone(),
                ToolCallContent::Diff { path, old_text, new_text } => {
                    serde_json::json!({
                        "type": "diff",
                        "path": path,
                        "old_text": old_text,
                        "new_text": new_text
                    }).to_string()
                }
                ToolCallContent::Terminal { terminal_id } => {
                    serde_json::json!({
                        "type": "terminal",
                        "terminal_id": terminal_id
                    }).to_string()
                }
            };
            Ok(ChatCompletionRequestMessage::Tool(
                ChatCompletionRequestToolMessage {
                    content: tool_content.into(),
                    tool_call_id: tool_call_id.clone(),
                }
            ))
        }
    }
}