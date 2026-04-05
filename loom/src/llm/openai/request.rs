//! OpenAI chat completion request building.
//!
//! Centralizes `Message → ChatCompletionRequestMessage` conversion and
//! the `CreateChatCompletionRequest` assembly (tools, temperature,
//! tool_choice, stream flag) so `invoke()` and `invoke_stream()` share
//! one code path.

use async_openai::types::chat::{
    ChatCompletionRequestMessageContentPartImage, ChatCompletionRequestMessageContentPartText,
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent,
    ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
    ChatCompletionRequestUserMessageContentPart,
    ChatCompletionTool,
    ChatCompletionToolChoiceOption,
    ChatCompletionTools,
    CreateChatCompletionRequestArgs,
    FunctionCall,
    FunctionObject,
    ImageUrl,
    ImageDetail,
    ToolChoiceOptions,
};

use crate::error::AgentError;
use crate::llm::ToolChoiceMode;
use crate::message::{assistant_content_for_chat_api, Message};
use crate::tool_source::ToolSpec;
use tracing::debug;

/// Convert internal `Message` list to OpenAI request messages.
pub(super) fn messages_to_openai(messages: &[Message]) -> Vec<ChatCompletionRequestMessage> {
    use crate::message::{ContentPart, UserContent};

    messages
        .iter()
        .map(|m| match m {
            Message::System(s) => ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage::from(s.as_str()),
            ),
            Message::User(content) => {
                let msg = match content {
                    UserContent::Text(s) => ChatCompletionRequestUserMessage::from(s.as_str()),
                    UserContent::Multimodal(parts) => {
                        let content_parts: Vec<
                            ChatCompletionRequestUserMessageContentPart,
                        > = parts
                            .iter()
                            .map(|p| match p {
                                ContentPart::Text { text } => {
                                    ChatCompletionRequestUserMessageContentPart::Text(
                                        ChatCompletionRequestMessageContentPartText {
                                            text: text.clone(),
                                        },
                                    )
                                }
                                ContentPart::ImageUrl { url, detail } => {
                                    let image_url = ImageUrl {
                                        url: url.clone(),
                                        detail: detail.as_ref().map(|d| match d.as_str() {
                                            "low" => ImageDetail::Low,
                                            "auto" => ImageDetail::Auto,
                                            _ => ImageDetail::High,
                                        }),
                                    };
                                    ChatCompletionRequestUserMessageContentPart::ImageUrl(
                                        ChatCompletionRequestMessageContentPartImage {
                                            image_url,
                                        },
                                    )
                                }
                                ContentPart::ImageBase64 { media_type, data } => {
                                    let data_uri = format!("data:{};base64,{}", media_type, data);
                                    ChatCompletionRequestUserMessageContentPart::ImageUrl(
                                        ChatCompletionRequestMessageContentPartImage {
                                            image_url: ImageUrl {
                                                url: data_uri,
                                                detail: None,
                                            },
                                        },
                                    )
                                }
                                // 其他模态降级处理：转为文本说明并记录警告
                                ContentPart::AudioBase64 { .. }
                                | ContentPart::VideoUrl { .. }
                                | ContentPart::VideoBase64 { .. }
                                | ContentPart::PdfUrl { .. }
                                | ContentPart::PdfBase64 { .. }
                                | ContentPart::File { .. } => {
                                    let modality = p.modality();
                                    tracing::warn!(
                                        modality = ?modality,
                                        "Modality not supported by OpenAI Chat API, converting to placeholder. \
                                        The original content will NOT be sent to the model."
                                    );
                                    ChatCompletionRequestUserMessageContentPart::Text(
                                        ChatCompletionRequestMessageContentPartText {
                                            text: format!(
                                                "[[[{:?} 未被当前模型支持，内容已省略]]]",
                                                modality
                                            ),
                                        },
                                    )
                                }
                            })
                            .collect();

                        ChatCompletionRequestUserMessage::from(ChatCompletionRequestUserMessageContent::Array(content_parts))
                    }
                };
                ChatCompletionRequestMessage::User(msg)
            }
            Message::Assistant(payload) => {
                let tool_calls: Option<Vec<ChatCompletionMessageToolCalls>> =
                    if payload.tool_calls.is_empty() {
                        None
                    } else {
                        Some(
                            payload
                                .tool_calls
                                .iter()
                                .map(|tc| {
                                    ChatCompletionMessageToolCalls::Function(
                                        ChatCompletionMessageToolCall {
                                            id: tc.id.clone(),
                                            function: FunctionCall {
                                                name: tc.name.clone(),
                                                arguments: tc.arguments.clone(),
                                            },
                                        },
                                    )
                                })
                                .collect(),
                        )
                    };
                let content = if payload.tool_calls.is_empty() {
                    let c = assistant_content_for_chat_api(payload.content.as_str());
                    Some(ChatCompletionRequestAssistantMessageContent::Text(
                        c.into_owned(),
                    ))
                } else if payload.content.trim().is_empty() {
                    None
                } else {
                    Some(ChatCompletionRequestAssistantMessageContent::Text(
                        payload.content.clone(),
                    ))
                };
                ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                    content,
                    tool_calls,
                    ..Default::default()
                })
            }
            Message::Tool {
                tool_call_id,
                content,
            } => ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                tool_call_id: tool_call_id.clone(),
                content: ChatCompletionRequestToolMessageContent::Text(content.to_display_string()),
            }),
        })
        .collect()
}

/// Build a complete `CreateChatCompletionRequest`.
///
/// When `stream` is true, sets `args.stream(true)` so the same function
/// serves both invoke and invoke_stream.
pub(super) fn build_chat_request(
    model: &str,
    messages: &[Message],
    tools: Option<&[ToolSpec]>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    stream: bool,
) -> Result<async_openai::types::chat::CreateChatCompletionRequest, AgentError> {
    debug!(
        model,
        stream,
        tools_count = tools.map_or(0, |t| t.len()),
        input_message_count = messages.len(),
        input_message_summary = ?messages
            .iter()
            .enumerate()
            .map(|(idx, msg)| match msg {
                Message::System(content) => {
                    format!("idx={idx} role=system content_len={}", content.len())
                }
                Message::User(content) => {
                    format!("idx={idx} role=user content_len={}", content.as_text().len())
                }
                Message::Assistant(payload) => format!(
                    "idx={idx} role=assistant tool_calls={} content_len={} reasoning_len={}",
                    payload.tool_calls.len(),
                    payload.content.len(),
                    payload.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0)
                ),
                Message::Tool { tool_call_id, content } => format!(
                    "idx={idx} role=tool tool_call_id={} content_len={}",
                    tool_call_id,
                    content.len()
                ),
            })
            .collect::<Vec<_>>(),
        "building OpenAI chat request from internal messages"
    );
    let openai_messages = messages_to_openai(messages);
    let mut args = CreateChatCompletionRequestArgs::default();

    args.model(model);
    args.messages(openai_messages);
    if stream {
        args.stream(true);
    }

    if let Some(tools) = tools {
        let chat_tools: Vec<ChatCompletionTools> = tools
            .iter()
            .map(|t| {
                ChatCompletionTools::Function(ChatCompletionTool {
                    function: FunctionObject {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: Some(t.input_schema.clone()),
                        ..Default::default()
                    },
                })
            })
            .collect();
        args.tools(chat_tools);
    }

    if let Some(t) = temperature {
        args.temperature(t);
    }

    let tools_nonempty = tools.map_or(false, |t| !t.is_empty());
    if let Some(mode) = tool_choice {
        if tools_nonempty {
            let opt = match mode {
                ToolChoiceMode::Auto => ToolChoiceOptions::Auto,
                ToolChoiceMode::None => ToolChoiceOptions::None,
                ToolChoiceMode::Required => ToolChoiceOptions::Required,
            };
            args.tool_choice(ChatCompletionToolChoiceOption::Mode(opt));
        } else {
            tracing::trace!(
                mode = ?mode,
                "omitting tool_choice: no tools advertised (API requires tools when tool_choice is set)"
            );
        }
    }

    let req = args
        .build()
        .map_err(|e| AgentError::ExecutionFailed(format!("OpenAI request build failed: {}", e)))?;

    // tracing::trace!(
    //     request = %serde_json::to_string(&req).unwrap_or_else(|e| format!("<serde error: {e}>")),
    //     "build_chat_request: full request JSON"
    // );

    tracing::debug!(
        model = req.model.as_str(),
        message_count = req.messages.len(),
        tools_count = req.tools.as_ref().map_or(0, |t| t.len()),
        temperature = ?req.temperature,
        tool_choice = ?req.tool_choice,
        stream = ?req.stream,
        "build_chat_request complete"
    );

    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolChoiceMode;
    use crate::message::Message;
    use crate::tool_source::ToolSpec;

    #[test]
    fn messages_to_openai_maps_all_roles() {
        let req = messages_to_openai(&[
            Message::System("s".to_string()),
            Message::user("u"),
            Message::assistant("a"),
        ]);
        assert_eq!(req.len(), 3);
    }

    #[test]
    fn messages_to_openai_serializes_assistant_tool_calls_and_tool_role() {
        use crate::message::{AssistantToolCall, Message};
        let req = messages_to_openai(&[
            Message::user("now?"),
            Message::assistant_with_tool_calls(
                String::new(),
                vec![AssistantToolCall {
                    id: "call_1".into(),
                    name: "get_time".into(),
                    arguments: "{}".into(),
                }],
            ),
            Message::Tool {
                tool_call_id: "call_1".into(),
                content: r#"{"iso":"2025"}"#.into(),
            },
        ]);
        let v = serde_json::to_value(&req).expect("json");
        assert_eq!(v[1]["role"], "assistant");
        assert!(v[1]["tool_calls"].is_array());
        assert_eq!(v[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(v[2]["role"], "tool");
        assert_eq!(v[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn build_chat_request_with_stream_flag() {
        let r = build_chat_request(
            "gpt-4o-mini",
            &[Message::user("hi")],
            None,
            None,
            None,
            true,
        )
        .unwrap();
        assert_eq!(r.stream, Some(true));
        let r2 = build_chat_request(
            "gpt-4o-mini",
            &[Message::user("hi")],
            None,
            None,
            None,
            false,
        )
        .unwrap();
        // Non-streaming requests may omit `stream` (None) rather than `Some(false)`.
        assert!(r2.stream.is_none() || r2.stream == Some(false));
    }

    #[test]
    fn build_chat_request_with_tools_and_tool_choice() {
        let tools = vec![ToolSpec {
            name: "get_time".into(),
            description: None,
            input_schema: serde_json::json!({}),
            output_hint: None,
        }];
        let r = build_chat_request(
            "gpt-4o-mini",
            &[Message::user("hi")],
            Some(&tools),
            None,
            Some(ToolChoiceMode::Required),
            false,
        )
        .unwrap();
        assert!(r.tools.is_some());
        assert!(r.tool_choice.is_some());
    }

    #[test]
    fn build_chat_request_omits_tool_choice_when_no_tools() {
        let r = build_chat_request(
            "gpt-4o-mini",
            &[Message::user("hi")],
            None,
            None,
            Some(ToolChoiceMode::Required),
            false,
        )
        .unwrap();
        assert!(r.tool_choice.is_none());
    }
}
