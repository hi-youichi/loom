//! OpenAI Chat Completions client implementing [`crate::llm::LlmClient`].
//!
//! [`ChatOpenAI`] is the default production client for OpenAI-compatible chat
//! completions. It can be constructed directly from environment-backed defaults
//! with [`ChatOpenAI::new`] or with an explicit [`OpenAIConfig`] via
//! [`ChatOpenAI::with_config`].
//!
//! # Streaming
//!
//! Implements `invoke_stream()` for token-by-token streaming. Uses the OpenAI
//! streaming API (`create_stream`) and sends `MessageChunk` through the provided
//! channel as tokens arrive. Tool calls are accumulated from stream chunks.
//!
//! Stream response format follows the [OpenAI Chat Completions Streaming] spec:
//! each SSE chunk is a chat completion chunk object with `choices[]`, and we read
//! `choices[0].delta.content` for incremental text and `choices[0].delta.tool_calls`
//! for tool calls. When `stream_options.include_usage` is true, the last chunk may
//! have empty `choices`; we omit `stream_options` so the request matches typical clients.
//!
//! [OpenAI Chat Completions Streaming]: https://platform.openai.com/docs/api-reference/chat-streaming
//!
//! **Interaction**: Implements `LlmClient`; used by ThinkNode like `MockLlm`.
//! Depends on `async_openai` (feature `openai`).

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, trace};

use crate::error::AgentError;
use crate::http_retry::{
    looks_like_transient_http_error_message, retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES,
};
use crate::llm::{LlmClient, LlmResponse, LlmUsage, ToolCallDelta};
use crate::memory::uuid6;
use crate::message::{assistant_content_for_chat_api, Message};
use crate::state::ToolCall;
use crate::stream::MessageChunk;
use crate::tool_source::{ToolSource, ToolSourceError, ToolSpec};

use async_openai::{
    config::{Config, OpenAIConfig},
    types::chat::{
        ChatCompletionMessageToolCalls, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage, ChatCompletionTool,
        ChatCompletionToolChoiceOption, ChatCompletionTools, CreateChatCompletionRequestArgs,
        FunctionObject, ToolChoiceOptions,
    },
    Client,
};

use super::ToolChoiceMode;

/// OpenAI Chat Completions client.
///
/// This type owns provider configuration plus optional tool metadata that will
/// be advertised to the model on each request. Use the builder-style `with_*`
/// methods to enable tools, configure temperature, or force a particular tool
/// choice policy.
pub struct ChatOpenAI {
    client: Client<OpenAIConfig>,
    model: String,
    tools: Option<Vec<ToolSpec>>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    /// When true, parse content for thinking tags and emit as MessageChunk::thinking / message.
    parse_thinking_tags: bool,
}

/// Tags used when `parse_thinking_tags` is enabled.
const THINKING_START: &str = "<think>";
const THINKING_END: &str = "</think>";

/// State for incremental parsing of thinking-tag segments.
#[derive(Clone, Copy)]
enum ThinkingParseState {
    Outside,
    Inside,
}

/// Removes thinking-tag blocks (and the tags) from content for stored assistant message.
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

impl ChatOpenAI {
    /// Builds a client with the default OpenAI configuration.
    ///
    /// Authentication and base URL are resolved by `async_openai`, which
    /// typically reads `OPENAI_API_KEY` and related environment variables.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
            parse_thinking_tags: false,
        }
    }

    /// Builds a client with an explicit OpenAI configuration.
    ///
    /// Use this when targeting a custom base URL, organization, project, or API
    /// key instead of the process environment.
    pub fn with_config(config: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            client: Client::with_config(config),
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
    /// resulting tool definitions in the client. In most setups you should use
    /// the same `ToolSource` for both the LLM-facing tool list and the
    /// [`crate::agent::react::ActNode`] execution layer so advertised tools and
    /// executable tools stay in sync.
    pub async fn new_with_tool_source(
        config: OpenAIConfig,
        model: impl Into<String>,
        tool_source: &dyn ToolSource,
    ) -> Result<Self, ToolSourceError> {
        let tools = tool_source.list_tools().await?;
        Ok(Self::with_config(config, model).with_tools(tools))
    }

    /// Sets the tools advertised to the model for each completion.
    ///
    /// Passing a non-empty tool list allows the provider to return tool calls.
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sets the sampling temperature for requests made by this client.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets the tool choice mode used when tools are present.
    ///
    /// If unset, the request omits `tool_choice` and the API default applies
    /// (typically `auto`). Note: OpenAI rejects `tool_choice: required` when
    /// the model has thinking/reasoning enabled; use [`ToolChoiceMode::Auto`]
    /// in that case.
    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
        self
    }

    /// Enables parsing of `<think>...</think>` segments in streamed output.
    ///
    /// Content inside thinking tags is emitted as
    /// [`MessageChunk::thinking`](crate::stream::MessageChunk::thinking), while
    /// the remaining content is emitted as normal message text.
    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    /// Returns the chat completions URL used for logging (base from OPENAI_BASE_URL or
    /// OPENAI_API_BASE env, else default). Does not append /v1 when base already ends with /v1.
    fn chat_completions_url() -> String {
        let base = std::env::var("OPENAI_BASE_URL")
            .or_else(|_| std::env::var("OPENAI_API_BASE"))
            .unwrap_or_else(|_| "https://api.openai.com".to_string());
        let base = base.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }

    /// Convert our `Message` list to OpenAI request messages (system/user/assistant text only).
    fn messages_to_request(messages: &[Message]) -> Vec<ChatCompletionRequestMessage> {
        messages
            .iter()
            .map(|m| match m {
                Message::System(s) => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessage::from(s.as_str()),
                ),
                Message::User(s) => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage::from(s.as_str()),
                ),
                Message::Assistant(s) => {
                    let c = assistant_content_for_chat_api(s.as_str());
                    ChatCompletionRequestMessage::Assistant((c.as_ref()).into())
                }
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let trace_id = uuid6().to_string();
        let build_request = || {
            let openai_messages = Self::messages_to_request(messages);
            let mut args = CreateChatCompletionRequestArgs::default();
            args.model(self.model.clone());
            args.messages(openai_messages);

            if let Some(ref tools) = self.tools {
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

            if let Some(t) = self.temperature {
                args.temperature(t);
            }

            if let Some(mode) = self.tool_choice {
                let opt = match mode {
                    ToolChoiceMode::Auto => ToolChoiceOptions::Auto,
                    ToolChoiceMode::None => ToolChoiceOptions::None,
                    ToolChoiceMode::Required => ToolChoiceOptions::Required,
                };
                args.tool_choice(ChatCompletionToolChoiceOption::Mode(opt));
            }

            args.build().map_err(|e| {
                AgentError::ExecutionFailed(format!("OpenAI request build failed: {}", e))
            })
        };

        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let url = Self::chat_completions_url();
        debug!(
            trace_id = %trace_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            tools_count = tools_count,
            temperature = ?self.temperature,
            tool_choice = ?self.tool_choice,
            "OpenAI chat create"
        );

        let mut attempt = 0;
        let (response, raw_request) = loop {
            let request = build_request()?;
            let raw_request = serde_json::to_string(&request).ok();
            match self.client.chat().create(request).await {
                Ok(response) => break (response, raw_request),
                Err(e)
                    if looks_like_transient_http_error_message(&e.to_string())
                        && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "OpenAI API request failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    return Err(AgentError::ExecutionFailed(format!(
                        "OpenAI API error: {}",
                        e
                    )));
                }
            }
        };

        let raw_response = serde_json::to_string(&response).ok();

        let choice =
            response.choices.into_iter().next().ok_or_else(|| {
                AgentError::ExecutionFailed("OpenAI returned no choices".to_string())
            })?;

        let msg = choice.message;
        let content = msg.content.unwrap_or_default();
        let reasoning_content = collect_thinking_tags(&content);
        let tool_calls: Vec<ToolCall> = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                if let ChatCompletionMessageToolCalls::Function(f) = tc {
                    Some(ToolCall {
                        name: f.function.name,
                        arguments: f.function.arguments,
                        id: Some(f.id),
                    })
                } else {
                    None
                }
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
            raw_request,
            raw_response,
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
        // If no streaming requested, use non-streaming path
        if chunk_tx.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid6().to_string();
        let chunk_tx = chunk_tx.expect("chunk_tx must be Some when streaming");
        let build_request = || {
            let openai_messages = Self::messages_to_request(messages);
            let mut args = CreateChatCompletionRequestArgs::default();
            args.model(self.model.clone());
            args.messages(openai_messages);
            args.stream(true);
            // Do not set stream_options so the request matches typical OpenAI clients. When
            // stream_options.include_usage is true, the last chunk may have empty choices and
            // usage; we already handle empty choices. Some proxies (e.g. GPTProto) return
            // broken streams when stream_options is sent, so omit it for compatibility.

            if let Some(ref tools) = self.tools {
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

            if let Some(t) = self.temperature {
                args.temperature(t);
            }

            if let Some(mode) = self.tool_choice {
                let opt = match mode {
                    ToolChoiceMode::Auto => ToolChoiceOptions::Auto,
                    ToolChoiceMode::None => ToolChoiceOptions::None,
                    ToolChoiceMode::Required => ToolChoiceOptions::Required,
                };
                args.tool_choice(ChatCompletionToolChoiceOption::Mode(opt));
            }

            args.build().map_err(|e| {
                AgentError::ExecutionFailed(format!("OpenAI request build failed: {}", e))
            })
        };

        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let url = Self::chat_completions_url();
        debug!(
            trace_id = %trace_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            stream = true,
            tools_count = tools_count,
            temperature = ?self.temperature,
            tool_choice = ?self.tool_choice,
            "OpenAI chat create_stream"
        );

        let mut attempt = 0;
        let (mut stream, raw_request) = loop {
            let request = build_request()?;
            let raw_request = serde_json::to_string(&request).ok();
            match self.client.chat().create_stream(request).await {
                Ok(stream) => break (stream, raw_request),
                Err(e)
                    if looks_like_transient_http_error_message(&e.to_string())
                        && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "OpenAI stream request failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    return Err(AgentError::ExecutionFailed(format!(
                        "OpenAI stream error: {}",
                        e
                    )));
                }
            }
        };

        // Accumulate content, tool calls, and usage from stream
        let mut full_content = String::new();
        // Track if we sent any content chunk (avoid duplicating at end for non-incremental APIs).
        let mut sent_any_content = false;
        // Tool calls accumulator: index -> (id, name, arguments)
        let mut tool_call_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new();
        let mut stream_usage: Option<LlmUsage> = None;

        // When parse_thinking_tags: buffer and parse thinking-tag segments.
        let mut segment_buf = String::new();
        let mut think_state = ThinkingParseState::Outside;

        while let Some(result) = stream.next().await {
            let response = result
                .map_err(|e| AgentError::ExecutionFailed(format!("OpenAI stream error: {}", e)))?;

            if let Some(ref u) = response.usage {
                stream_usage = Some(LlmUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                });
            }

            for choice in response.choices {
                let delta = &choice.delta;

                // Handle content delta
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
                                            segment_buf = after[THINKING_START.len()..].to_string();
                                            think_state = ThinkingParseState::Inside;
                                        } else {
                                            // No full start tag yet; keep potential prefix for next delta
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
                                            segment_buf = after[THINKING_END.len()..].to_string();
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

                // Handle tool calls delta (accumulated by index)
                if let Some(ref tool_calls) = delta.tool_calls {
                    for tc in tool_calls {
                        let entry = tool_call_map.entry(tc.index).or_insert_with(|| {
                            (
                                tc.id.clone().unwrap_or_default(),
                                String::new(),
                                String::new(),
                            )
                        });

                        // Update id if provided
                        if let Some(ref id) = tc.id {
                            if !id.is_empty() {
                                entry.0 = id.clone();
                            }
                        }

                        // Accumulate function name and arguments
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

        // Flush remaining segment buffer (thinking tags mode)
        if self.parse_thinking_tags {
            if !segment_buf.is_empty() {
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
        }

        // Some proxies (e.g. GPTProto) return stream chunks with empty choices[] but valid usage;
        // non-streaming with the same request returns content. Fall back to one non-streaming call
        // so the user gets the real reply instead of a generic fallback message.
        let completion_tokens = stream_usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        if full_content.is_empty() && tool_call_map.is_empty() && completion_tokens > 0 {
            match self.invoke(messages).await {
                Ok(fallback_resp)
                    if !fallback_resp.content.is_empty()
                        || !fallback_resp.tool_calls.is_empty() =>
                {
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
                    // Use fallback tool_calls; we'll overwrite tool_call_map so the final collect below yields these.
                    tool_call_map = fallback_resp
                        .tool_calls
                        .into_iter()
                        .enumerate()
                        .map(|(i, tc)| {
                            (i as u32, (tc.id.unwrap_or_default(), tc.name, tc.arguments))
                        })
                        .collect();
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        // Some APIs (e.g. proxies) send content only in the final payload, not in deltas.
        // Send the full content as one chunk so the SSE stream still has assistant text.
        if !sent_any_content && !full_content.is_empty() {
            let _ = chunk_tx
                .send(MessageChunk::message(full_content.clone()))
                .await;
        }

        // Convert accumulated tool calls to our format
        let mut tool_calls: Vec<ToolCall> = tool_call_map
            .into_iter()
            .map(|(_, (id, name, arguments))| ToolCall {
                name,
                arguments,
                id: if id.is_empty() { None } else { Some(id) },
            })
            .collect();

        // Sort by name for deterministic order
        tool_calls.sort_by(|a, b| a.name.cmp(&b.name));

        let url = Self::chat_completions_url();
        let reasoning_content = collect_thinking_tags(&full_content);
        trace!(
            trace_id = %trace_id,
            url = %url,
            reasoning_len = reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0),
            tool_calls = ?tool_calls,
            usage = ?stream_usage,
            "OpenAI stream response"
        );

        Ok(LlmResponse {
            content: if self.parse_thinking_tags {
                strip_thinking_tags(&full_content)
            } else {
                full_content
            },
            reasoning_content,
            tool_calls,
            usage: stream_usage,
            raw_request,
            raw_response: None, // SSE: full response body not captured
        })
    }

    async fn list_models(&self) -> Result<Vec<crate::llm::ModelInfo>, AgentError> {
        // OpenAI-compatible gateways often omit `created` on each model; `async-openai`'s
        // `Model` type requires it and fails to deserialize. Parse permissively instead.
        let cfg = self.client.config();
        let url = cfg.url("/models");
        let res = reqwest::Client::new()
            .get(&url)
            .headers(cfg.headers())
            .send()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("Failed to list models: {}", e)))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(AgentError::ExecutionFailed(format!(
                "Failed to list models: {} - {}",
                status, body
            )));
        }

        let body = res
            .text()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("Failed to list models: {}", e)))?;

        let parsed: OpenAiListModelsBody = serde_json::from_str(&body).map_err(|e| {
            AgentError::ExecutionFailed(format!(
                "Failed to list models: failed to deserialize api response: {} content:{}",
                e, body
            ))
        })?;

        Ok(parsed
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

/// `/v1/models` list payload: tolerate missing `created` and other gateway quirks.
#[derive(serde::Deserialize)]
struct OpenAiListModelsBody {
    data: Vec<OpenAiModelListRow>,
}

#[derive(serde::Deserialize)]
struct OpenAiModelListRow {
    id: String,
    #[serde(default, deserialize_with = "deserialize_optional_model_created")]
    created: Option<i64>,
    #[serde(default)]
    owned_by: Option<String>,
}

fn deserialize_optional_model_created<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Deserialize;
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(v.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmClient;
    use crate::message::Message;
    use std::sync::{Mutex, OnceLock};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = pos + 4;
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = buf[header_end..].to_vec();
                while body.len() < content_length {
                    let m = stream.read(&mut tmp).await.unwrap();
                    if m == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..m]);
                }
                return String::from_utf8_lossy(&body[..content_length]).to_string();
            }
        }
        String::new()
    }

    async fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            status,
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).await.unwrap();
    }

    /// **Scenario**: strip_thinking_tags removes thinking-tag blocks for stored message.
    #[test]
    fn strip_thinking_tags_removes_blocks() {
        use super::{strip_thinking_tags, THINKING_END, THINKING_START};
        assert_eq!(strip_thinking_tags("hello"), "hello");
        let with_block = format!("a {}think{} b", THINKING_START, THINKING_END);
        assert_eq!(strip_thinking_tags(&with_block), "a  b");
        let only_block = format!("{}only{}", THINKING_START, THINKING_END);
        assert_eq!(strip_thinking_tags(&only_block), "");
    }

    /// **Scenario**: collect_thinking_tags extracts inner thinking text without tags.
    #[test]
    fn collect_thinking_tags_extracts_inner_text() {
        use super::{collect_thinking_tags, THINKING_END, THINKING_START};
        let tagged = format!(
            "before {}alpha{} middle {}beta{}",
            THINKING_START, THINKING_END, THINKING_START, THINKING_END
        );
        assert_eq!(collect_thinking_tags(&tagged).as_deref(), Some("alphabeta"));
        assert_eq!(collect_thinking_tags("plain text"), None);
    }

    /// **Scenario**: ChatOpenAI::new sets model; tools and temperature are None.
    #[test]
    fn chat_openai_new_creates_client() {
        let _ = ChatOpenAI::new("gpt-4");
        let _ = ChatOpenAI::new("gpt-4o-mini");
    }

    /// **Scenario**: ChatOpenAI::with_config uses custom config and model.
    #[test]
    fn chat_openai_with_config_creates_client() {
        let config = OpenAIConfig::new().with_api_key("test-key");
        let _ = ChatOpenAI::with_config(config, "gpt-4");
    }

    /// **Scenario**: `/v1/models` payloads without per-model `created` deserialize (OpenAI-compatible gateways).
    #[test]
    fn openai_list_models_body_allows_missing_created() {
        let json = r#"{"data":[{"id":"chatgpt-4o-latest","object":"model","owned_by":"openai","permission":[],"root":"chatgpt-4o-latest","parent":null}]}"#;
        let parsed: super::OpenAiListModelsBody = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, "chatgpt-4o-latest");
        assert_eq!(parsed.data[0].created, None);
        assert_eq!(parsed.data[0].owned_by.as_deref(), Some("openai"));
    }

    /// **Scenario**: `created` may be a JSON number or string depending on the gateway.
    #[test]
    fn openai_list_models_body_parses_created_number_or_string() {
        let with_num = r#"{"data":[{"id":"a","created":1700000000}]}"#;
        let p: super::OpenAiListModelsBody = serde_json::from_str(with_num).unwrap();
        assert_eq!(p.data[0].created, Some(1_700_000_000));

        let with_str = r#"{"data":[{"id":"b","created":"1700000001"}]}"#;
        let p2: super::OpenAiListModelsBody = serde_json::from_str(with_str).unwrap();
        assert_eq!(p2.data[0].created, Some(1_700_000_001));
    }

    /// **Scenario**: Builder chain with_tools and with_temperature builds without panic.
    #[test]
    fn chat_openai_with_tools_and_temperature_builder() {
        let tools = vec![ToolSpec {
            name: "get_time".into(),
            description: None,
            input_schema: serde_json::json!({}),
            output_hint: None,
        }];
        let _ = ChatOpenAI::new("gpt-4")
            .with_tools(tools)
            .with_temperature(0.5f32);
    }

    #[test]
    fn chat_completions_url_uses_env_variants() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("OPENAI_BASE_URL", "https://example.com");
        assert_eq!(
            ChatOpenAI::chat_completions_url(),
            "https://example.com/v1/chat/completions"
        );
        std::env::remove_var("OPENAI_BASE_URL");

        std::env::set_var("OPENAI_API_BASE", "https://example.com/v1");
        assert_eq!(
            ChatOpenAI::chat_completions_url(),
            "https://example.com/v1/chat/completions"
        );
        std::env::remove_var("OPENAI_API_BASE");
    }

    #[test]
    fn messages_to_request_maps_all_roles() {
        let req = ChatOpenAI::messages_to_request(&[
            Message::System("s".to_string()),
            Message::User("u".to_string()),
            Message::Assistant("a".to_string()),
        ]);
        assert_eq!(req.len(), 3);
    }

    /// **Scenario**: invoke() against an unreachable API base returns an error (no real API key needed).
    /// Given a client configured with an invalid base URL, when we call invoke() with one user message,
    /// then the result is Err (e.g. connection refused or timeout).
    #[tokio::test]
    async fn invoke_with_unreachable_base_returns_error() {
        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base("https://127.0.0.1:1");
        let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
        let messages = [Message::user("Hello")];

        let result = client.invoke(&messages).await;

        assert!(
            result.is_err(),
            "invoke against unreachable base should return Err"
        );
    }

    /// **Scenario**: invoke_stream() against an unreachable API base returns an error (no real API key needed).
    /// Given a client configured with an invalid base URL and a channel, when we call invoke_stream()
    /// with one user message, then the result is Err.
    #[tokio::test]
    async fn invoke_stream_with_unreachable_base_returns_error() {
        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base("https://127.0.0.1:1");
        let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
        let messages = [Message::user("Hello")];
        let (tx, _rx) = mpsc::channel(16);

        let result = client.invoke_stream(&messages, Some(tx)).await;

        assert!(
            result.is_err(),
            "invoke_stream against unreachable base should return Err"
        );
    }

    /// **Scenario**: invoke_stream() with no channel delegates to invoke() and returns the same outcome.
    /// Given a client with unreachable base, when we call invoke_stream(msgs, None), then we get the same
    /// Err as invoke(msgs).
    #[tokio::test]
    async fn invoke_stream_with_none_channel_delegates_to_invoke() {
        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base("https://127.0.0.1:1");
        let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
        let messages = [Message::user("Hi")];

        let res_invoke = client.invoke(&messages).await;
        let res_stream = client.invoke_stream(&messages, None).await;

        assert!(res_invoke.is_err());
        assert!(res_stream.is_err());
    }

    #[tokio::test]
    async fn invoke_and_invoke_stream_none_channel_succeed_with_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let body = read_http_request(&mut stream).await;
                let req: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                assert!(req.get("messages").is_some());
                let response = serde_json::json!({
                    "id":"chatcmpl-1",
                    "object":"chat.completion",
                    "created": 1,
                    "model":"gpt-4o-mini",
                    "choices":[
                        {
                            "index":0,
                            "message":{
                                "role":"assistant",
                                "content":"hello",
                                "tool_calls":[
                                    {
                                        "id":"call_1",
                                        "type":"function",
                                        "function":{"name":"get_time","arguments":"{}"}
                                    }
                                ]
                            },
                            "finish_reason":"stop"
                        }
                    ],
                    "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
                })
                .to_string();
                write_http_response(&mut stream, "200 OK", &response).await;
            }
        });

        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(format!("http://{}", addr));
        let tools = vec![ToolSpec {
            name: "get_time".into(),
            description: Some("time".into()),
            input_schema: serde_json::json!({"type":"object"}),
            output_hint: None,
        }];
        let client = ChatOpenAI::with_config(config, "gpt-4o-mini")
            .with_tools(tools)
            .with_temperature(0.2)
            .with_tool_choice(ToolChoiceMode::Required);
        let messages = [Message::user("hello")];
        let res = client.invoke(&messages).await.unwrap();
        assert_eq!(res.content, "hello");
        assert_eq!(res.tool_calls.len(), 1);
        assert_eq!(res.usage.unwrap().total_tokens, 2);

        let res_stream = client.invoke_stream(&messages, None).await.unwrap();
        assert_eq!(res_stream.content, "hello");
        assert_eq!(res_stream.tool_calls.len(), 1);
    }

    #[tokio::test]
    async fn invoke_returns_error_when_choices_missing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            let response = serde_json::json!({
                "id":"chatcmpl-2",
                "object":"chat.completion",
                "created": 1,
                "model":"gpt-4o-mini",
                "choices":[],
                "usage":{"prompt_tokens":1,"completion_tokens":0,"total_tokens":1}
            })
            .to_string();
            write_http_response(&mut stream, "200 OK", &response).await;
        });

        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(format!("http://{}", addr));
        let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
        let err = match client.invoke(&[Message::user("x")]).await {
            Ok(_) => panic!("expected no-choices error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("no choices"));
    }

    /// **Scenario**: invoke() against real OpenAI API returns Ok when OPENAI_API_KEY is set.
    /// Given a client with default config and valid API key in env, when we call invoke() with one user message,
    /// then the result is Ok and the response has content or tool_calls (model-dependent).
    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY; run with: cargo test -p loom invoke_with_real_api -- --ignored"]
    async fn invoke_with_real_api_returns_ok() {
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for this test");

        let model = std::env::var("MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let client = ChatOpenAI::new(model);
        let messages = [Message::user("Say exactly: ok")];

        let result = client.invoke(&messages).await;

        let response = result.expect("invoke with real API should succeed");
        assert!(
            !response.content.is_empty() || !response.tool_calls.is_empty(),
            "response should have content or tool_calls"
        );
    }

    /// **Scenario**: invoke_stream() against real OpenAI API returns Ok and sends chunks when OPENAI_API_KEY is set.
    /// Given a client with default config and a channel, when we call invoke_stream() with one user message,
    /// then the result is Ok, the response content is non-empty or tool_calls present, and chunks were received.
    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY; run with: cargo test -p loom invoke_stream_with_real_api -- --ignored"]
    async fn invoke_stream_with_real_api_returns_ok() {
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for this test");

        let model = std::env::var("MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let client = ChatOpenAI::new(model);
        let messages = [Message::user("Say exactly: ok")];
        let (tx, mut rx) = mpsc::channel(16);

        let result = client.invoke_stream(&messages, Some(tx)).await;

        let response = result.expect("invoke_stream with real API should succeed");
        assert!(
            !response.content.is_empty() || !response.tool_calls.is_empty(),
            "response should have content or tool_calls"
        );

        let mut chunks = 0u32;
        while rx.try_recv().is_ok() {
            chunks += 1;
        }
        assert!(chunks > 0, "should receive at least one stream chunk");
    }
}
