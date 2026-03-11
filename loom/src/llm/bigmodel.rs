//! BigModel (智谱) Chat Completions client implementing `LlmClient`.
//!
//! Uses the BigModel API at <https://open.bigmodel.cn/api/paas/v4/> (OpenAI-compatible).
//! Uses the same config as OpenAI: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL` (set `LLM_PROVIDER=bigmodel` to use this client). Optional tools enable function calling.
//!
//! # Streaming
//!
//! Implements `invoke_stream()` and `invoke_stream_with_tool_delta()` via SSE; parses
//! `data:` lines and `data: [DONE]`, accumulates content and tool_calls, and sends
//! `MessageChunk` / `ToolCallDelta` through the provided channel.
//!
//! **Interaction**: Implements `LlmClient`; used by ThinkNode like `ChatOpenAI`.
//! Depends on `reqwest` (no async_openai).

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::{debug, trace};

use crate::error::AgentError;
use crate::llm::{LlmClient, LlmResponse, LlmUsage, ToolCallDelta};
use crate::memory::uuid6;
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;
use crate::tool_source::{ToolSource, ToolSourceError, ToolSpec};

use super::ToolChoiceMode;

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const THINKING_START: &str = "<think>";
const THINKING_END: &str = "</think>";

#[derive(Clone, Copy)]
enum ThinkingParseState {
    Outside,
    Inside,
}

fn strip_thinking_tags(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(THINKING_START) {
        out.push_str(&rest[..start]);
        rest = &rest[start + THINKING_START.len()..];
        if let Some(end) = rest.find(THINKING_END) {
            rest = &rest[end + THINKING_END.len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

// ----- Request DTOs (OpenAI-compatible) -----

#[derive(serde::Serialize)]
struct ChatMessageRequest {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct ToolFunctionRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

#[derive(serde::Serialize)]
struct ToolSpecRequest {
    #[serde(rename = "type")]
    type_: String,
    function: ToolFunctionRequest,
}

#[derive(serde::Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessageRequest>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolSpecRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
}

// ----- Non-stream response DTOs -----

#[derive(serde::Deserialize)]
struct ResponseMessageFunction {
    name: String,
    arguments: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ResponseToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    type_: Option<String>,
    function: Option<ResponseMessageFunction>,
}

#[derive(serde::Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(serde::Deserialize)]
struct ResponseChoice {
    message: ResponseMessage,
}

#[derive(serde::Deserialize)]
struct ResponseUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(serde::Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ResponseChoice>,
    usage: Option<ResponseUsage>,
}

// ----- Stream chunk DTOs -----

#[derive(serde::Deserialize, Default)]
struct StreamDeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct StreamToolCallDelta {
    index: u32,
    id: Option<String>,
    function: Option<StreamDeltaFunction>,
}

#[derive(serde::Deserialize, Default)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(serde::Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(serde::Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<ResponseUsage>,
}

/// BigModel (智谱) Chat Completions client implementing `LlmClient`.
///
/// Uses the same env as OpenAI: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL` when built via `new()`; or provide config via `ChatBigModel::with_config`. Optionally set tools to enable tool_calls.
///
/// **Interaction**: Implements `LlmClient`; used by ThinkNode.
pub struct ChatBigModel {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    tools: Option<Vec<ToolSpec>>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    parse_thinking_tags: bool,
}

impl ChatBigModel {
    /// Build client from environment (same as OpenAI: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL`).
    pub fn new(model: impl Into<String>) -> Result<Self, AgentError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| AgentError::ExecutionFailed("OPENAI_API_KEY is not set".to_string()))?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = model.into();
        Ok(Self::with_config(base_url, api_key, model))
    }

    /// Build client with explicit base URL, API key, and model.
    pub fn with_config(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
            parse_thinking_tags: false,
        }
    }

    /// Build client with tools from the given ToolSource.
    pub async fn new_with_tool_source(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        tool_source: &dyn ToolSource,
    ) -> Result<Self, ToolSourceError> {
        let tools = tool_source.list_tools().await?;
        Ok(Self::with_config(base_url, api_key, model).with_tools(tools))
    }

    /// Set tools for this completion (enables tool_calls in response).
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set temperature (0–1 for BigModel). Clamped to [0.0, 1.0].
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature.clamp(0.0, 1.0));
        self
    }

    /// Set tool choice mode (auto, none, required).
    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
        self
    }

    /// When true, parse streamed content for `<think>...</think>` and emit as MessageChunk::thinking / message.
    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }

    fn messages_to_request(messages: &[Message]) -> Vec<ChatMessageRequest> {
        messages
            .iter()
            .map(|m| {
                let (role, content) = match m {
                    Message::System(s) => ("system", s.as_str()),
                    Message::User(s) => ("user", s.as_str()),
                    Message::Assistant(s) => ("assistant", s.as_str()),
                };
                ChatMessageRequest {
                    role: role.to_string(),
                    content: content.to_string(),
                }
            })
            .collect()
    }

    fn build_request(&self, messages: &[Message], stream: bool) -> ChatCompletionRequest {
        let messages = Self::messages_to_request(messages);
        let mut req = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            stream,
            temperature: self.temperature,
            tools: None,
            tool_choice: None,
        };
        if let Some(ref tools) = self.tools {
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
            req.tool_choice = Some(
                self.tool_choice
                    .map(|m| match m {
                        ToolChoiceMode::Auto => "auto",
                        ToolChoiceMode::None => "none",
                        ToolChoiceMode::Required => "required",
                    })
                    .unwrap_or("required")
                    .to_string(),
            );
        }
        req
    }
}

#[async_trait]
impl LlmClient for ChatBigModel {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let trace_id = uuid6().to_string();
        let url = self.chat_completions_url();
        let body = self.build_request(messages, false);
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        debug!(
            trace_id = %trace_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            tools_count = tools_count,
            "BigModel chat create"
        );

        let res = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("BigModel request failed: {}", e)))?;

        let status = res.status();
        let body_bytes = res
            .bytes()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("BigModel response read: {}", e)))?;

        if !status.is_success() {
            let msg = String::from_utf8_lossy(&body_bytes);
            return Err(AgentError::ExecutionFailed(format!(
                "BigModel API error {}: {}",
                status,
                msg
            )));
        }

        let response: ChatCompletionResponse = serde_json::from_slice(&body_bytes).map_err(|e| {
            AgentError::ExecutionFailed(format!("BigModel response parse: {}", e))
        })?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AgentError::ExecutionFailed("BigModel returned no choices".to_string()))?;

        let msg = choice.message;
        let content = msg.content.unwrap_or_default();
        let tool_calls: Vec<ToolCall> = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                tc.function.as_ref().map(|f| ToolCall {
                    name: f.name.clone(),
                    arguments: f.arguments.clone(),
                    id: tc.id.clone(),
                })
            })
            .collect();

        let usage = response.usage.map(|u| LlmUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(LlmResponse {
            content,
            tool_calls,
            usage,
        })
    }

    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        self.invoke_stream_with_tool_delta(messages, chunk_tx, None)
            .await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError> {
        if chunk_tx.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid6().to_string();
        let chunk_tx = chunk_tx.unwrap();
        let url = self.chat_completions_url();
        let body = self.build_request(messages, true);
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        debug!(
            trace_id = %trace_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            stream = true,
            tools_count = tools_count,
            "BigModel chat create_stream"
        );

        let res = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("BigModel stream request: {}", e)))?;

        let status = res.status();
        if !status.is_success() {
            let body_bytes = res.bytes().await.unwrap_or_default();
            let msg = String::from_utf8_lossy(&body_bytes);
            return Err(AgentError::ExecutionFailed(format!(
                "BigModel stream error {}: {}",
                status,
                msg
            )));
        }

        let body_bytes = res
            .bytes()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("BigModel stream body: {}", e)))?;
        let text = String::from_utf8_lossy(&body_bytes);

        let mut full_content = String::new();
        let mut sent_any_content = false;
        let mut tool_call_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new();
        let mut stream_usage: Option<LlmUsage> = None;
        let mut segment_buf = String::new();
        let mut think_state = ThinkingParseState::Outside;

        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with("data: ") {
                continue;
            }
            let data = line.trim_start_matches("data: ").trim();
            if data == "[DONE]" {
                break;
            }
            let chunk: StreamChunk = match serde_json::from_str(data) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if let Some(ref u) = chunk.usage {
                stream_usage = Some(LlmUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                });
            }

            let choices = match chunk.choices {
                Some(c) => c,
                None => continue,
            };

            for choice in choices {
                let delta = choice.delta;

                if let Some(ref content) = delta.content {
                    if !content.is_empty() {
                        full_content.push_str(content);
                        sent_any_content = true;

                        if self.parse_thinking_tags {
                            segment_buf.push_str(content);
                            loop {
                                match think_state {
                                    ThinkingParseState::Outside => {
                                        if let Some(i) = segment_buf.find(THINKING_START) {
                                            let (before, after) = segment_buf.split_at(i);
                                            if !before.is_empty() {
                                                let _ = chunk_tx
                                                    .send(MessageChunk::message(before.to_string()))
                                                    .await;
                                            }
                                            segment_buf =
                                                after[THINKING_START.len()..].to_string();
                                            think_state = ThinkingParseState::Inside;
                                        } else {
                                            let keep = segment_buf.len().saturating_sub(
                                                THINKING_START.len().saturating_sub(1),
                                            );
                                            let to_send = segment_buf[..keep].to_string();
                                            segment_buf = segment_buf[keep..].to_string();
                                            if !to_send.is_empty() {
                                                let _ = chunk_tx
                                                    .send(MessageChunk::message(to_send))
                                                    .await;
                                            }
                                            break;
                                        }
                                    }
                                    ThinkingParseState::Inside => {
                                        if let Some(i) = segment_buf.find(THINKING_END) {
                                            let (inside, after) = segment_buf.split_at(i);
                                            if !inside.is_empty() {
                                                let _ = chunk_tx
                                                    .send(MessageChunk::thinking(
                                                        inside.to_string(),
                                                    ))
                                                    .await;
                                            }
                                            segment_buf =
                                                after[THINKING_END.len()..].to_string();
                                            think_state = ThinkingParseState::Outside;
                                        } else {
                                            let keep = segment_buf.len().saturating_sub(
                                                THINKING_END.len().saturating_sub(1),
                                            );
                                            let to_send = segment_buf[..keep].to_string();
                                            segment_buf = segment_buf[keep..].to_string();
                                            if !to_send.is_empty() {
                                                let _ = chunk_tx
                                                    .send(MessageChunk::thinking(to_send))
                                                    .await;
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        } else {
                            let _ = chunk_tx.send(MessageChunk::message(content.clone())).await;
                        }
                    }
                }

                if let Some(ref tool_calls) = delta.tool_calls {
                    for tc in tool_calls {
                        let entry = tool_call_map.entry(tc.index).or_insert_with(|| {
                            (
                                tc.id.clone().unwrap_or_default(),
                                String::new(),
                                String::new(),
                            )
                        });
                        if let Some(ref id) = tc.id {
                            if !id.is_empty() {
                                entry.0 = id.clone();
                            }
                        }
                        if let Some(ref func) = tc.function {
                            if let Some(ref name) = func.name {
                                entry.1.push_str(name);
                            }
                            if let Some(ref args) = func.arguments {
                                entry.2.push_str(args);
                            }
                        }
                        if let Some(ref tool_tx) = tool_delta_tx {
                            let args_delta = tc
                                .function
                                .as_ref()
                                .and_then(|f| f.arguments.clone())
                                .unwrap_or_default();
                            if !args_delta.is_empty() || tc.id.is_some() {
                                let _ = tool_tx
                                    .send(ToolCallDelta {
                                        call_id: tc.id.clone(),
                                        name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                        arguments_delta: args_delta,
                                    })
                                    .await;
                            }
                        }
                    }
                }
            }
        }

        if self.parse_thinking_tags && !segment_buf.is_empty() {
            match think_state {
                ThinkingParseState::Outside => {
                    let _ = chunk_tx
                        .send(MessageChunk::message(segment_buf.clone()))
                        .await;
                }
                ThinkingParseState::Inside => {
                    let _ = chunk_tx
                        .send(MessageChunk::thinking(segment_buf.clone()))
                        .await;
                }
            }
        }

        let completion_tokens = stream_usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
        if full_content.is_empty() && tool_call_map.is_empty() && completion_tokens > 0 {
            if let Ok(fallback_resp) = self.invoke(messages).await {
                if !fallback_resp.content.is_empty() || !fallback_resp.tool_calls.is_empty() {
                    full_content = fallback_resp.content.clone();
                    if !full_content.is_empty() {
                        sent_any_content = true;
                        let _ = chunk_tx
                            .send(MessageChunk::message(full_content.clone()))
                            .await;
                    }
                    if stream_usage.is_none() {
                        stream_usage = fallback_resp.usage;
                    }
                    tool_call_map = fallback_resp
                        .tool_calls
                        .into_iter()
                        .enumerate()
                        .map(|(i, tc)| {
                            (i as u32, (tc.id.unwrap_or_default(), tc.name, tc.arguments))
                        })
                        .collect();
                }
            }
        }

        if !sent_any_content && !full_content.is_empty() {
            let _ = chunk_tx
                .send(MessageChunk::message(full_content.clone()))
                .await;
        }

        let mut tool_calls: Vec<ToolCall> = tool_call_map
            .into_iter()
            .map(|(_, (id, name, arguments))| ToolCall {
                name,
                arguments,
                id: if id.is_empty() { None } else { Some(id) },
            })
            .collect();
        tool_calls.sort_by(|a, b| a.name.cmp(&b.name));

        trace!(
            trace_id = %trace_id,
            url = %url,
            tool_calls = ?tool_calls.len(),
            usage = ?stream_usage,
            "BigModel stream response"
        );

        Ok(LlmResponse {
            content: if self.parse_thinking_tags {
                strip_thinking_tags(&full_content)
            } else {
                full_content
            },
            tool_calls,
            usage: stream_usage,
        })
    }
}
