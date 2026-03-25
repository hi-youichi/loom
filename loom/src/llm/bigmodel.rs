//! BigModel (智谱) chat completions client implementing [`crate::llm::LlmClient`].
//!
//! [`ChatBigModel`] targets BigModel's OpenAI-compatible API at
//! <https://open.bigmodel.cn/api/paas/v4/>. Its builder surface intentionally
//! mirrors [`crate::llm::ChatOpenAI`] so higher-level Loom code can switch
//! providers with minimal branching.
//!
//! # Streaming
//!
//! Implements `invoke_stream()` and `invoke_stream_with_tool_delta()` via SSE; parses
//! `data:` lines and `data: [DONE]`, accumulates content and tool_calls, and sends
//! `MessageChunk` / `ToolCallDelta` through the provided channel.
//!
//! The response body is read with `res.chunk().await` in a loop; each chunk is appended
//! to a line buffer and complete SSE lines (`data: ...` / `data: [DONE]`) are parsed and
//! emitted to `chunk_tx` as they arrive, so the client sees tokens in real time.
//!
//! **Interaction**: Implements `LlmClient`; used by ThinkNode like `ChatOpenAI`.
//! Depends on `reqwest` (no async_openai).

use std::borrow::Cow;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::{debug, trace};

use crate::error::AgentError;
use crate::http_retry::{
    is_retryable_reqwest_error, retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES,
};
use crate::llm::{LlmClient, LlmResponse, LlmUsage, ToolCallDelta};
use crate::memory::uuid6;
use crate::message::{assistant_content_for_chat_api, Message};
use crate::state::ToolCall;
use crate::stream::MessageChunk;
use crate::tool_source::{ToolSource, ToolSourceError, ToolSpec};

use super::ToolChoiceMode;

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const THINKING_START: &str = "<think>";
const THINKING_END: &str = "</think>";

/// Max retries for retryable 5xx (500, 502, 503, 504). Total attempts = 1 + this.
const BIGMODEL_5XX_MAX_RETRIES: u32 = 3;
/// Initial backoff before first retry.
const BIGMODEL_5XX_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
/// Max backoff cap.
const BIGMODEL_5XX_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(16);

/// Returns true for transient 5xx where retry is reasonable: 500, 502, 503, 504.
/// Other 5xx (501 Not Implemented, 505 HTTP Version Not Supported, etc.) are not retried.
fn is_retryable_5xx(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 500 | 502 | 503 | 504)
}

fn backoff_for_attempt(attempt: u32) -> std::time::Duration {
    let secs = BIGMODEL_5XX_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32);
    let d = std::time::Duration::from_secs_f64(secs);
    d.min(BIGMODEL_5XX_MAX_BACKOFF)
}

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

fn collect_thinking_tags(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(THINKING_START) {
        rest = &rest[start + THINKING_START.len()..];
        if let Some(end) = rest.find(THINKING_END) {
            out.push_str(&rest[..end]);
            rest = &rest[end + THINKING_END.len()..];
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
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
    #[serde(default, alias = "reasoning", alias = "reason_content")]
    reasoning_content: Option<String>,
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
    #[serde(default, alias = "reasoning", alias = "reason_content")]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(serde::Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    /// OpenAI-compatible; optional so we don't fail if the API omits it.
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<ResponseUsage>,
}

/// BigModel chat completions client.
///
/// This client uses OpenAI-compatible request and response shapes, including
/// tool calling and SSE streaming. Use the builder-style `with_*` methods to
/// align request behavior with the tool source and prompting strategy used by
/// the surrounding ReAct runtime.
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
    /// Builds a client from environment-backed defaults.
    ///
    /// This reads `OPENAI_API_KEY` and optionally `OPENAI_BASE_URL`. The model
    /// name is still provided explicitly so callers can choose it at runtime.
    pub fn new(model: impl Into<String>) -> Result<Self, AgentError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| AgentError::ExecutionFailed("OPENAI_API_KEY is not set".to_string()))?;
        let base_url =
            std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = model.into();
        Ok(Self::with_config(base_url, api_key, model))
    }

    /// Builds a client with an explicit base URL, API key, and model.
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

    /// Builds a client with tools loaded from a [`ToolSource`].
    ///
    /// This eagerly calls `tool_source.list_tools().await` and then stores the
    /// resulting tool definitions in the client.
    pub async fn new_with_tool_source(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        tool_source: &dyn ToolSource,
    ) -> Result<Self, ToolSourceError> {
        let tools = tool_source.list_tools().await?;
        Ok(Self::with_config(base_url, api_key, model).with_tools(tools))
    }

    /// Sets the tools advertised to the model for each completion.
    ///
    /// Passing tools allows the provider to return function calls in the
    /// response payload.
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sets the sampling temperature for requests made by this client.
    ///
    /// BigModel expects values in `[0.0, 1.0]`, so inputs are clamped into that range.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature.clamp(0.0, 1.0));
        self
    }

    /// Sets the tool choice mode used when tools are present.
    ///
    /// If unset, `tool_choice` is omitted from the request (provider default,
    /// usually `auto`). `required` conflicts with thinking/reasoning on some APIs.
    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
        self
    }

    /// Enables parsing of `<think>...</think>` segments in streamed output.
    ///
    /// Content inside thinking tags is emitted separately from normal assistant
    /// message text so callers can render reasoning and final output differently.
    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }

    fn messages_to_request(messages: &[Message], model: &str) -> Vec<ChatMessageRequest> {
        let use_space_for_empty_assistant = model.to_lowercase().starts_with("kimi");
        messages
            .iter()
            .map(|m| {
                let (role, content) = match m {
                    Message::System(s) => ("system", Cow::Borrowed(s.as_str())),
                    Message::User(s) => ("user", Cow::Borrowed(s.as_str())),
                    Message::Assistant(s) => {
                        let c = assistant_content_for_chat_api(s.as_str());
                        let content = if use_space_for_empty_assistant && c.trim().is_empty() {
                            Cow::Borrowed(" ")
                        } else {
                            c
                        };
                        ("assistant", content)
                    }
                };
                ChatMessageRequest {
                    role: role.to_string(),
                    content: content.into_owned(),
                }
            })
            .collect()
    }

    fn build_request(&self, messages: &[Message], stream: bool) -> ChatCompletionRequest {
        let messages = Self::messages_to_request(messages, &self.model);
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
            if let Some(mode) = self.tool_choice {
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

        let mut transport_attempt = 0;
        let (_status, body_bytes) = 'request: loop {
            let res = {
                let mut attempt = 0;
                loop {
                    match self
                        .client
                        .post(&url)
                        .bearer_auth(&self.api_key)
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(res) => break res,
                        Err(e)
                            if is_retryable_reqwest_error(&e)
                                && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                        {
                            let delay = retry_backoff_for_attempt(attempt);
                            tracing::warn!(
                                url = %url,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %e,
                                "BigModel request transport failed, retrying"
                            );
                            attempt += 1;
                            tokio::time::sleep(delay).await;
                        }
                        Err(e) => {
                            return Err(AgentError::ExecutionFailed(format!(
                                "BigModel request failed: {}",
                                e
                            )));
                        }
                    }
                }
            };

            let status = res.status();
            let body_bytes = match res.bytes().await {
                Ok(body_bytes) => body_bytes,
                Err(e)
                    if is_retryable_reqwest_error(&e)
                        && transport_attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(transport_attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = transport_attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "BigModel response body read failed, retrying"
                    );
                    transport_attempt += 1;
                    tokio::time::sleep(delay).await;
                    continue 'request;
                }
                Err(e) => {
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel response read: {}",
                        e
                    )));
                }
            };

            if status.is_success() {
                break 'request (status, body_bytes);
            }
            if !is_retryable_5xx(status) {
                let msg = String::from_utf8_lossy(&body_bytes);
                return Err(AgentError::ExecutionFailed(format!(
                    "BigModel API error {}: {}",
                    status, msg
                )));
            }
            for attempt in 0..BIGMODEL_5XX_MAX_RETRIES {
                let delay = backoff_for_attempt(attempt);
                tracing::warn!(
                    status = %status,
                    attempt = attempt + 1,
                    max_retries = BIGMODEL_5XX_MAX_RETRIES,
                    delay_secs = delay.as_secs_f64(),
                    "BigModel 5xx, retrying"
                );
                tokio::time::sleep(delay).await;
                let retry_res = self
                    .client
                    .post(&url)
                    .bearer_auth(&self.api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        AgentError::ExecutionFailed(format!("BigModel request failed: {}", e))
                    })?;
                let retry_status = retry_res.status();
                let retry_bytes = retry_res.bytes().await.map_err(|e| {
                    AgentError::ExecutionFailed(format!("BigModel response read: {}", e))
                })?;
                if retry_status.is_success() {
                    break 'request (retry_status, retry_bytes);
                }
                if !is_retryable_5xx(retry_status) {
                    let msg = String::from_utf8_lossy(&retry_bytes);
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel API error {}: {}",
                        retry_status, msg
                    )));
                }
                if attempt == BIGMODEL_5XX_MAX_RETRIES - 1 {
                    let msg = String::from_utf8_lossy(&retry_bytes);
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel API error {}: {} (after {} retries)",
                        retry_status, msg, BIGMODEL_5XX_MAX_RETRIES
                    )));
                }
            }
            let msg = String::from_utf8_lossy(&body_bytes);
            return Err(AgentError::ExecutionFailed(format!(
                "BigModel API error {}: {}",
                status, msg
            )));
        };

        let response: ChatCompletionResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| AgentError::ExecutionFailed(format!("BigModel response parse: {}", e)))?;

        let choice = response.choices.into_iter().next().ok_or_else(|| {
            AgentError::ExecutionFailed("BigModel returned no choices".to_string())
        })?;

        let msg = choice.message;
        let content = msg.content.unwrap_or_default();
        let reasoning_content = msg
            .reasoning_content
            .or_else(|| collect_thinking_tags(&content));
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
            reasoning_content,
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
        let chunk_tx = chunk_tx.expect("chunk_tx must be Some when streaming");
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

        let mut res = 'stream_request: loop {
            let response = {
                let mut attempt = 0;
                loop {
                    match self
                        .client
                        .post(&url)
                        .bearer_auth(&self.api_key)
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(response) => break response,
                        Err(e)
                            if is_retryable_reqwest_error(&e)
                                && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                        {
                            let delay = retry_backoff_for_attempt(attempt);
                            tracing::warn!(
                                url = %url,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %e,
                                "BigModel stream request failed, retrying"
                            );
                            attempt += 1;
                            tokio::time::sleep(delay).await;
                        }
                        Err(e) => {
                            return Err(AgentError::ExecutionFailed(format!(
                                "BigModel stream request: {}",
                                e
                            )));
                        }
                    }
                }
            };

            let status = response.status();
            if status.is_success() {
                break 'stream_request response;
            }
            if !is_retryable_5xx(status) {
                let body_bytes = response.bytes().await.unwrap_or_default();
                let msg = String::from_utf8_lossy(&body_bytes);
                return Err(AgentError::ExecutionFailed(format!(
                    "BigModel stream error {}: {}",
                    status, msg
                )));
            }
            for attempt in 0..BIGMODEL_5XX_MAX_RETRIES {
                let delay = backoff_for_attempt(attempt);
                tracing::warn!(
                    status = %status,
                    attempt = attempt + 1,
                    max_retries = BIGMODEL_5XX_MAX_RETRIES,
                    delay_secs = delay.as_secs_f64(),
                    "BigModel stream 5xx, retrying"
                );
                tokio::time::sleep(delay).await;
                let retry_res = self
                    .client
                    .post(&url)
                    .bearer_auth(&self.api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        AgentError::ExecutionFailed(format!("BigModel stream request: {}", e))
                    })?;
                let retry_status = retry_res.status();
                if retry_status.is_success() {
                    break 'stream_request retry_res;
                }
                if !is_retryable_5xx(retry_status) {
                    let body_bytes = retry_res.bytes().await.unwrap_or_default();
                    let msg = String::from_utf8_lossy(&body_bytes);
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel stream error {}: {}",
                        retry_status, msg
                    )));
                }
                if attempt == BIGMODEL_5XX_MAX_RETRIES - 1 {
                    let body_bytes = retry_res.bytes().await.unwrap_or_default();
                    let msg = String::from_utf8_lossy(&body_bytes);
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel stream error {}: {} (after {} retries)",
                        retry_status, msg, BIGMODEL_5XX_MAX_RETRIES
                    )));
                }
            }
            let body_bytes = response.bytes().await.unwrap_or_default();
            let msg = String::from_utf8_lossy(&body_bytes);
            return Err(AgentError::ExecutionFailed(format!(
                "BigModel stream error {}: {}",
                status, msg
            )));
        };

        let mut buf = Vec::<u8>::new();
        let mut full_content = String::new();
        let mut full_reasoning_content = String::new();
        let mut sent_any_content = false;
        let mut tool_call_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new();
        let mut stream_usage: Option<LlmUsage> = None;
        let mut segment_buf = String::new();
        let mut think_state = ThinkingParseState::Outside;
        let mut done = false;
        let mut stream_read_attempt = 0;

        while !done {
            let chunk = match res.chunk().await {
                Ok(Some(bytes)) => Some(bytes),
                Ok(None) => None,
                Err(e)
                    if is_retryable_reqwest_error(&e)
                        && stream_read_attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(stream_read_attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = stream_read_attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "BigModel stream body read failed, retrying"
                    );
                    stream_read_attempt += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => {
                    return Err(AgentError::ExecutionFailed(format!(
                        "BigModel stream body: {}",
                        e
                    )));
                }
            };
            let Some(bytes) = chunk else { break };

            trace!(bytes_len = bytes.len(), "BigModel stream bytes");
            buf.extend_from_slice(&bytes);

            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                let line = match std::str::from_utf8(&line_bytes) {
                    Ok(s) => s.trim(),
                    Err(_) => continue,
                };
                if line.is_empty() {
                    continue;
                }
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = line.trim_start_matches("data: ").trim();
                if data == "[DONE]" {
                    done = true;
                    break;
                }
                let stream_chunk: StreamChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                if let Some(ref u) = stream_chunk.usage {
                    stream_usage = Some(LlmUsage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                    });
                }

                let choices = match stream_chunk.choices {
                    Some(c) => c,
                    None => continue,
                };

                trace!(data = %data, "BigModel stream data");

                for choice in choices {
                    let content_len = choice.delta.content.as_ref().map(|s| s.len()).unwrap_or(0);
                    let reasoning_len = choice
                        .delta
                        .reasoning_content
                        .as_ref()
                        .map(|s| s.len())
                        .unwrap_or(0);
                    let tool_call_count = choice
                        .delta
                        .tool_calls
                        .as_ref()
                        .map(|calls| calls.len())
                        .unwrap_or(0);
                    trace!(
                        content_len,
                        reasoning_len,
                        tool_call_count,
                        finish_reason = ?choice.finish_reason,
                        content = ?choice.delta.content,
                        reasoning_content = ?choice.delta.reasoning_content,
                        "BigModel stream chunk"
                    );
                    let delta = choice.delta;

                    if let Some(ref reasoning_content) = delta.reasoning_content {
                        if !reasoning_content.is_empty() {
                            full_reasoning_content.push_str(reasoning_content);
                            let _ = chunk_tx
                                .send(MessageChunk::thinking(reasoning_content.clone()))
                                .await;
                        }
                    }

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
                                                        .send(MessageChunk::message(
                                                            before.to_string(),
                                                        ))
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
                            let tool_name = tc.function.as_ref().and_then(|f| f.name.clone());
                            let args_delta = tc
                                .function
                                .as_ref()
                                .and_then(|f| f.arguments.clone())
                                .unwrap_or_default();
                            trace!(
                                tool_index = tc.index,
                                call_id = ?tc.id,
                                name = ?tool_name,
                                arguments_delta_len = args_delta.len(),
                                arguments_delta = %args_delta,
                                "BigModel stream tool chunk"
                            );
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
                                if !args_delta.is_empty() || tc.id.is_some() {
                                    let _ = tool_tx
                                        .send(ToolCallDelta {
                                            call_id: tc.id.clone(),
                                            name: tool_name,
                                            arguments_delta: args_delta,
                                        })
                                        .await;
                                }
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

        let completion_tokens = stream_usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        if full_content.is_empty()
            && full_reasoning_content.is_empty()
            && tool_call_map.is_empty()
            && completion_tokens > 0
        {
            if let Ok(fallback_resp) = self.invoke(messages).await {
                if !fallback_resp.content.is_empty()
                    || fallback_resp.reasoning_content.is_some()
                    || !fallback_resp.tool_calls.is_empty()
                {
                    full_content = fallback_resp.content.clone();
                    if let Some(reasoning_content) = fallback_resp.reasoning_content.clone() {
                        full_reasoning_content = reasoning_content.clone();
                        let _ = chunk_tx
                            .send(MessageChunk::thinking(reasoning_content))
                            .await;
                    }
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
            reasoning_len = full_reasoning_content.len(),
            tool_calls = ?tool_calls.len(),
            usage = ?stream_usage,
            "BigModel stream response"
        );

        let reasoning_content = if full_reasoning_content.is_empty() {
            collect_thinking_tags(&full_content)
        } else {
            Some(full_reasoning_content)
        };

        Ok(LlmResponse {
            content: if self.parse_thinking_tags {
                strip_thinking_tags(&full_content)
            } else {
                full_content
            },
            reasoning_content,
            tool_calls,
            usage: stream_usage,
        })
    }

    async fn list_models(&self) -> Result<Vec<crate::llm::ModelInfo>, AgentError> {
        // BigModel base URL already includes version path (e.g., /api/paas/v4)
        // so we only append /models, not /v1/models
        let url = format!("{}/models", self.base_url);
        let res = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("list_models request failed: {}", e)))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(AgentError::ExecutionFailed(format!(
                "list_models failed: {} - {}",
                status, body
            )));
        }

        let body = res
            .text()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("list_models read body failed: {}", e)))?;

        let models_resp: ModelsResponse = serde_json::from_str(&body)
            .map_err(|e| AgentError::ExecutionFailed(format!("list_models parse failed: {}", e)))?;

        Ok(models_resp
            .data
            .into_iter()
            .map(|m| crate::llm::ModelInfo {
                id: m.id,
                created: m.created,
                owned_by: m.owned_by,
            })
            .collect())
    }
}

/// Response from /v1/models endpoint
#[derive(serde::Deserialize)]
struct ModelsResponse {
    data: Vec<ModelData>,
}

#[derive(serde::Deserialize)]
struct ModelData {
    id: String,
    created: Option<i64>,
    owned_by: Option<String>,
}
