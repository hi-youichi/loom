//! OpenAI Chat Completions client ([`crate::llm::LlmClient`]) via `async_openai`.
//! Streaming uses the Chat Completions SSE API; see OpenAI docs for chunk shape.

mod models;
mod request;
mod stream;

#[cfg(test)]
mod tests;

use async_openai::{
    config::OpenAIConfig,
    types::chat::{ChatCompletionMessageToolCalls, CompletionUsage, CreateChatCompletionRequest},
    Client,
};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, trace};

use crate::error::AgentError;
use crate::http_retry::{
    classify_openai_error_message, retry_backoff_for_attempt, RetryDecision,
    TRANSIENT_HTTP_MAX_RETRIES,
};
use crate::llm::thinking::collect_thinking_tags;
use crate::llm::{LlmClient, LlmResponse, LlmUsage, ToolCallDelta};
use crate::memory::uuid6;
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;
use crate::tool_source::{ToolSource, ToolSourceError, ToolSpec};

use super::ToolChoiceMode;

pub(super) fn completion_usage_to_llm(u: &CompletionUsage) -> LlmUsage {
    use crate::llm::{CompletionTokensDetails, PromptTokensDetails};

    LlmUsage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens: u.total_tokens,
        prompt_tokens_details: u.prompt_tokens_details.as_ref().map(|d| PromptTokensDetails {
            cached_tokens: d.cached_tokens,
            audio_tokens: d.audio_tokens,
        }),
        completion_tokens_details: u
            .completion_tokens_details
            .as_ref()
            .map(|d| CompletionTokensDetails {
                reasoning_tokens: d.reasoning_tokens,
                audio_tokens: d.audio_tokens,
                accepted_prediction_tokens: d.accepted_prediction_tokens,
                rejected_prediction_tokens: d.rejected_prediction_tokens,
            }),
    }
}

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
    headers: Option<crate::llm::LlmHeaders>,
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
            headers: None,
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
            headers: None,
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

    /// Enables parsing of thinking-tag segments in streamed output.
    ///
    /// Content inside thinking tags is emitted as
    /// [`MessageChunk::thinking`](crate::stream::MessageChunk::thinking), while
    /// the remaining content is emitted as normal message text.
    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    /// Sets HTTP headers for LLM requests.
    ///
    /// This allows adding custom headers like X-App-Id, X-Thread-Id, X-Trace-Id
    /// for request tracking and observability.
    pub fn with_headers(mut self, headers: crate::llm::LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    #[allow(dead_code)]
    fn get_headers_map(&self) -> std::collections::HashMap<String, String> {
        let mut headers = std::collections::HashMap::new();

        if let Some(config) = &self.headers {
            // Fixed X-App-Id header as "loom"
            headers.insert("X-App-Id".to_string(), "loom".to_string());
            
            if let Some(thread_id) = &config.thread_id {
                headers.insert("X-Thread-Id".to_string(), thread_id.clone());
            }
            if let Some(trace_id) = &config.trace_id {
                headers.insert("X-Trace-Id".to_string(), trace_id.clone());
            }

            for (key, value) in &config.custom_headers {
                headers.insert(key.clone(), value.clone());
            }
        }

        headers
    }

    /// Chat completions URL for logging (`OPENAI_BASE_URL` / `OPENAI_API_BASE` or default).
    pub(crate) fn chat_completions_url() -> String {
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

    fn build_request(
        &self,
        messages: &[Message],
        stream: bool,
    ) -> Result<CreateChatCompletionRequest, AgentError> {
        request::build_chat_request(
            &self.model,
            messages,
            self.tools.as_deref(),
            self.temperature,
            self.tool_choice,
            stream,
        )
    }
}

#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let trace_id = uuid6().to_string();
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
        let response = loop {
            let request = self.build_request(messages, false)?;
            match self.client.chat().create(request).await {
                Ok(response) => break response,
                Err(e) => {
                    let error_message = e.to_string();
                    let retry_decision = classify_openai_error_message(&error_message);
                    if matches!(retry_decision, RetryDecision::Retryable)
                        && attempt < TRANSIENT_HTTP_MAX_RETRIES
                    {
                        let delay = retry_backoff_for_attempt(attempt);
                        tracing::warn!(
                            url = %url,
                            attempt = attempt + 1,
                            max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                            delay_secs = delay.as_secs_f64(),
                            retry_decision = ?retry_decision,
                            error = %error_message,
                            "OpenAI API request failed, retrying"
                        );
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        retry_decision = ?retry_decision,
                        error = %error_message,
                        "OpenAI API request failed without retry"
                    );
                    return Err(AgentError::ExecutionFailed(format!(
                        "OpenAI API error: {}",
                        error_message
                    )));
                }
            }
        };

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

        let usage = response.usage.as_ref().map(completion_usage_to_llm);
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
        let mut stream = loop {
            let request = self.build_request(messages, true)?;
            match self.client.chat().create_stream(request).await {
                Ok(stream) => break stream,
                Err(e) => {
                    let error_message = e.to_string();
                    let retry_decision = classify_openai_error_message(&error_message);
                    if matches!(retry_decision, RetryDecision::Retryable)
                        && attempt < TRANSIENT_HTTP_MAX_RETRIES
                    {
                        let delay = retry_backoff_for_attempt(attempt);
                        tracing::warn!(
                            url = %url,
                            attempt = attempt + 1,
                            max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                            delay_secs = delay.as_secs_f64(),
                            retry_decision = ?retry_decision,
                            error = %error_message,
                            "OpenAI stream request failed, retrying"
                        );
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        retry_decision = ?retry_decision,
                        error = %error_message,
                        "OpenAI stream request failed without retry"
                    );
                    return Err(AgentError::ExecutionFailed(format!(
                        "OpenAI stream error: {}",
                        error_message
                    )));
                }
            }
        };

        let mut acc = stream::StreamAccumulator::new(self.parse_thinking_tags);
        while let Some(result) = stream.next().await {
            let response = result
                .map_err(|e| AgentError::ExecutionFailed(format!("OpenAI stream error: {}", e)))?;
            acc.process_chunk(response, &chunk_tx, tool_delta_tx.as_ref())
                .await;
        }

        acc.flush(&chunk_tx).await;

        acc.emit_full_if_needed(&chunk_tx).await;

        let result = acc.finish();
        trace!(
            trace_id = %trace_id,
            url = %url,
            reasoning_len = result.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0),
            tool_calls = ?result.tool_calls,
            usage = ?result.usage,
            "OpenAI stream response"
        );

        Ok(LlmResponse {
            content: result.content,
            reasoning_content: result.reasoning_content,
            tool_calls: result.tool_calls,
            usage: result.usage,
        })
    }

    async fn list_models(&self) -> Result<Vec<crate::llm::ModelInfo>, AgentError> {
        models::list_models(self.client.config()).await
    }
}
