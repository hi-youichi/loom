//! Request / response DTOs and message-conversion helpers for the
//! OpenAI-compatible `/chat/completions` endpoint.

use std::borrow::Cow;

use crate::message::{
    assistant_content_for_chat_api, AssistantToolCall, ContentPart, Message, UserContent,
};
use crate::tool::ToolSpec;
use crate::traits::ToolChoiceMode;
use serde_json::json;
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
    pub content: Option<serde_json::Value>,
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
    pub stream_options: Option<StreamOptionsRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSpecRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(serde::Serialize, Clone)]
pub(super) struct StreamOptionsRequest {
    pub include_usage: bool,
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
    #[serde(default)]
    pub finish_reason: Option<String>,
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
pub(super) fn messages_to_request(messages: &[Message], model: &str) -> Vec<ChatMessageRequest> {
    let use_space_for_empty_assistant = model.to_lowercase().starts_with("kimi");
    messages
        .iter()
        .map(|m| match m {
            Message::System(s) => ChatMessageRequest {
                role: "system".to_string(),
                content: Some(serde_json::json!(s)),
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
                                ContentPart::AudioBase64 { media_type, data } => {
                                    json!({
                                        "type": "input_audio",
                                        "input_audio": {
                                            "data": data,
                                            "format": audio_format_from_media_type(media_type)
                                        }
                                    })
                                }
                                ContentPart::VideoUrl { url } => {
                                    json!({
                                        "type": "video_url",
                                        "video_url": { "url": url }
                                    })
                                }
                                ContentPart::VideoBase64 { media_type, data } => {
                                    json!({
                                        "type": "input_video",
                                        "input_video": {
                                            "data": data,
                                            "format": media_type
                                        }
                                    })
                                }
                                ContentPart::PdfUrl { url } => {
                                    json!({
                                        "type": "file",
                                        "file": { "url": url }
                                    })
                                }
                                ContentPart::PdfBase64 { data } => {
                                    json!({
                                        "type": "file",
                                        "file": {
                                            "data": data,
                                            "format": "pdf"
                                        }
                                    })
                                }
                                ContentPart::File {
                                    file_id,
                                    file_data,
                                    filename,
                                } => {
                                    let mut file_obj = serde_json::Map::new();
                                    if let Some(id) = file_id {
                                        file_obj.insert("file_id".into(), json!(id));
                                    }
                                    if let Some(d) = file_data {
                                        file_obj.insert("data".into(), json!(d));
                                    }
                                    if let Some(n) = filename {
                                        file_obj.insert("filename".into(), json!(n));
                                    }
                                    json!({ "type": "file", "file": file_obj })
                                }
                            })
                            .collect::<Vec<_>>())
                    }
                };
                ChatMessageRequest {
                    role: "user".to_string(),
                    content: Some(content_value),
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
                    .map(|tc| {
                        if tc.id.is_empty() {
                            AssistantToolCall {
                                id: format!("call_{}", uuid::Uuid::new_v4()),
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            }
                        } else {
                            tc.clone()
                        }
                    })
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
                    Some(serde_json::json!(c))
                } else if payload.content.trim().is_empty() {
                    None
                } else {
                    Some(serde_json::json!(payload.content))
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
                content: Some(serde_json::json!(content.to_display_string())),
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
    reasoning_effort: Option<&str>,
    stream: bool,
) -> ChatCompletionRequest {
    use crate::message::{check_orphan_tool_calls, message_summary, sanitize_tool_call_ids};

    let sanitized = sanitize_tool_call_ids(messages.to_vec());
    if sanitized.len() != messages.len() {
        tracing::warn!(
            before = messages.len(),
            after = sanitized.len(),
            "compat:build_request messages were sanitized"
        );
    }

    tracing::debug!(
        model = %model,
        stream,
        message_count = sanitized.len(),
        "compat:build_request"
    );
    for (i, msg) in sanitized.iter().enumerate() {
        tracing::debug!("  {}", message_summary(i, msg));
    }
    for w in check_orphan_tool_calls(&sanitized) {
        tracing::warn!("compat:build_request {}", w);
    }

    let msgs = messages_to_request(&sanitized, model);
    let stream_options = if stream {
        Some(StreamOptionsRequest {
            include_usage: true,
        })
    } else {
        None
    };
    let mut req = ChatCompletionRequest {
        model: model.to_string(),
        messages: msgs,
        stream,
        stream_options,
        temperature,
        max_tokens: None,
        top_p: None,
        response_format: None,
        seed: None,
        tools: None,
        tool_choice: None,
        reasoning_effort: reasoning_effort
            .filter(|e| *e != "auto")
            .map(|e| e.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ContentPart, Message};

    #[test]
    fn text_user_message_serializes_as_json_string() {
        let msgs = vec![Message::user("hello")];
        let req = messages_to_request(&msgs, "qwen-plus");
        let json = serde_json::to_value(&req[0]).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello");
    }

    #[test]
    fn multimodal_user_message_serializes_as_json_array() {
        let parts = vec![
            ContentPart::Text {
                text: "describe this".into(),
            },
            ContentPart::ImageBase64 {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ];
        let msg = Message::user_multimodal(parts).unwrap();
        let req = messages_to_request(&[msg], "qwen-plus");
        let json = serde_json::to_value(&req[0]).unwrap();

        assert!(
            json["content"].is_array(),
            "multimodal content must be array"
        );
        let arr = json["content"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[1]["type"], "image_url");
        assert!(
            arr[1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,"),
            "base64 image must be data URI"
        );
    }

    #[test]
    fn image_url_user_message_serializes_correctly() {
        let parts = vec![ContentPart::ImageUrl {
            url: "https://example.com/test.png".into(),
            detail: Some("high".into()),
        }];
        let msg = Message::user_multimodal(parts).unwrap();
        let req = messages_to_request(&[msg], "qwen-plus");
        let json = serde_json::to_value(&req[0]).unwrap();

        assert!(json["content"].is_array());
        let arr = json["content"].as_array().unwrap();
        assert_eq!(arr[0]["type"], "image_url");
        assert_eq!(arr[0]["image_url"]["url"], "https://example.com/test.png");
        assert_eq!(arr[0]["image_url"]["detail"], "high");
    }

    #[test]
    fn system_message_serializes_as_plain_string() {
        let msgs = vec![Message::System("you are helpful".into())];
        let req = messages_to_request(&msgs, "qwen-plus");
        let json = serde_json::to_value(&req[0]).unwrap();
        assert_eq!(json["role"], "system");
        assert_eq!(json["content"], "you are helpful");
    }

    #[test]
    fn assistant_message_serializes_as_plain_string() {
        let msgs = vec![Message::assistant("I can help")];
        let req = messages_to_request(&msgs, "qwen-plus");
        let json = serde_json::to_value(&req[0]).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"], "I can help");
    }
}

fn audio_format_from_media_type(media_type: &str) -> &str {
    match media_type {
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/mp4" => "m4a",
        _ => media_type
            .rsplit_once('/')
            .map(|(_, f)| f)
            .unwrap_or(media_type),
    }
}
