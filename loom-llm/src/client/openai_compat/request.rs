//! Request / response DTOs and message-conversion helpers for the
//! OpenAI-compatible `/chat/completions` endpoint.

use std::borrow::Cow;

use crate::message::{assistant_content_for_chat_api, ContentPart, Message, UserContent};
use crate::tool::ToolSpec;
use crate::traits::ToolChoiceMode;

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone)]
pub(super) struct BigModelToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct BigModelToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: &'static str,
    pub function: BigModelToolFunction,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct ChatMessageRequest {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<BigModelToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct ToolFunctionRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct ToolSpecRequest {
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolFunctionRequest,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessageRequest>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSpecRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

// ---------------------------------------------------------------------------
// Non-stream response DTOs
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub(super) struct ResponseMessageFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub(super) struct ResponseToolCall {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub function: Option<ResponseMessageFunction>,
}

#[derive(serde::Deserialize)]
pub(super) struct ResponseMessage {
    pub content: Option<String>,
    #[serde(default, alias = "reasoning", alias = "reason_content")]
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(serde::Deserialize)]
pub(super) struct ResponseChoice {
    pub message: ResponseMessage,
}

#[derive(serde::Deserialize, Clone)]
pub(super) struct ResponseUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<crate::traits::PromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<crate::traits::CompletionTokensDetails>,
}

#[derive(serde::Deserialize)]
pub(super) struct ChatCompletionResponse {
    pub choices: Vec<ResponseChoice>,
    pub usage: Option<ResponseUsage>,
}

// ---------------------------------------------------------------------------
// /models endpoint DTOs
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub(super) struct ModelsResponse {
    pub data: Vec<ModelData>,
}

#[derive(serde::Deserialize)]
pub(super) struct ModelData {
    pub id: String,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

// ---------------------------------------------------------------------------
// Message → request conversion
// ---------------------------------------------------------------------------

/// Convert [`Message`] list into OpenAI-compatible request DTOs.
pub(super) fn messages_to_request(
    messages: &[Message],
    model: &str,
) -> Vec<ChatMessageRequest> {
    let use_space_for_empty_assistant = model.to_lowercase().starts_with("kimi");
    messages
        .iter()
        .map(|m| match m {
            Message::System(s) => ChatMessageRequest {
                role: "system".to_string(),
                content: Some(s.clone()),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            Message::User(content) => {
                let content_value = match content {
                    UserContent::Text(s) => serde_json::json!(s),
                    UserContent::Multimodal(parts) => {
                        serde_json::json!(parts
                            .iter()
                            .map(|p| match p {
                                ContentPart::Text { text } => {
                                    serde_json::json!({ "type": "text", "text": text })
                                }
                                ContentPart::ImageUrl { url, detail } => {
                                    let mut obj = serde_json::json!({
                                        "type": "image_url",
                                        "image_url": { "url": url }
                                    });
                                    if let Some(detail) = detail {
                                        if let Some(obj_url) = obj.get_mut("image_url") {
                                            if let Some(obj_url) = obj_url.as_object_mut() {
                                                obj_url.insert(
                                                    "detail".into(),
                                                    serde_json::json!(detail),
                                                );
                                            }
                                        }
                                    }
                                    obj
                                }
                                ContentPart::ImageBase64 { media_type, data } => {
                                    serde_json::json!({
                                        "type": "image_url",
                                        "image_url": {
                                            "url": format!("data:{};base64,{}", media_type, data)
                                        }
                                    })
                                }
                                _ => {
                                    let modality = crate::message::content_part_modality(p);
                                    tracing::warn!(
                                        modality = ?modality,
                                        "Modality not supported by OpenAI-compatible API, converting to placeholder. \
                                        The original content will NOT be sent to the model."
                                    );
                                    serde_json::json!({
                                        "type": "text",
                                        "text": format!("[[[{:?} 未被当前模型支持，内容已省略]]]", modality)
                                    })
                                }
                            })
                            .collect::<Vec<_>>())
                    }
                };
                ChatMessageRequest {
                    role: "user".to_string(),
                    content: Some(content_value.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                }
            }
            Message::Assistant(payload) => {
                let valid_tool_calls: Vec<_> = payload
                    .tool_calls
                    .iter()
                    .filter(|tc| !tc.name.is_empty())
                    .collect();
                let tool_calls = if valid_tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        valid_tool_calls
                            .iter()
                            .map(|tc| BigModelToolCall {
                                id: tc.id.clone(),
                                call_type: "function",
                                function: BigModelToolFunction {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                },
                            })
                            .collect(),
                    )
                };
                let content = if valid_tool_calls.is_empty() {
                    let c = assistant_content_for_chat_api(payload.content.as_str());
                    let c = if use_space_for_empty_assistant && c.trim().is_empty() {
                        Cow::Borrowed(" ")
                    } else {
                        c
                    };
                    Some(c.into_owned())
                } else if payload.content.trim().is_empty() {
                    None
                } else {
                    Some(payload.content.clone())
                };
                ChatMessageRequest {
                    role: "assistant".to_string(),
                    content,
                    tool_calls,
                    tool_call_id: None,
                    reasoning_content: payload.reasoning_content.clone(),
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => ChatMessageRequest {
                role: "tool".to_string(),
                content: Some(content.to_display_string()),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
                reasoning_content: None,
            },
        })
        .collect()
}

/// Build a complete [`ChatCompletionRequest`] from the client's configuration.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_request(
    model: &str,
    messages: &[Message],
    tools: Option<&[ToolSpec]>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    stream: bool,
) -> ChatCompletionRequest {
    use crate::message::{check_orphan_tool_calls, message_summary};
    tracing::debug!(
        model = %model,
        stream,
        message_count = messages.len(),
        "compat:build_request"
    );
    for (i, msg) in messages.iter().enumerate() {
        tracing::debug!("  {}", message_summary(i, msg));
    }
    for w in check_orphan_tool_calls(messages) {
        tracing::warn!("compat:build_request {}", w);
    }

    let msgs = messages_to_request(messages, model);
    let mut req = ChatCompletionRequest {
        model: model.to_string(),
        messages: msgs,
        stream,
        temperature,
        tools: None,
        tool_choice: None,
    };
    if let Some(tools) = tools {
        if !tools.is_empty() {
            req.tools = Some(
                tools
                    .iter()
                    .map(|t| ToolSpecRequest {
                        type_: "function".to_string(),
                        function: ToolFunctionRequest {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.input_schema.clone(),
                        },
                    })
                    .collect(),
            );
            if let Some(mode) = tool_choice {
                req.tool_choice = Some(
                    match mode {
                        ToolChoiceMode::Auto => "auto",
                        ToolChoiceMode::None => "none",
                        ToolChoiceMode::Required => "required",
                    }
                    .to_string(),
                );
            }
        }
    }
    req
}
