//! OpenAI Chat Completions client ([`crate::llm::LlmClient`]) via `async_openai`.
//! Streaming uses the Chat Completions SSE API; see OpenAI docs for chunk shape.

mod models;
mod request;
mod stream;

#[cfg(test)]
mod tests;

use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    types::chat::{ChatCompletionMessageToolCalls, CompletionUsage, CreateChatCompletionRequest},
    Client,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::{debug, trace};

use crate::error::LlmError;
use crate::message::Message;
use crate::support::audit::{
    build_audit_entry, LlmAuditLog, LlmAuditRequest, LlmAuditRequestParams, LlmAuditResponse,
    LlmAuditToolCall, LlmAuditUsage,
};
use crate::support::error_classifier::LlmErrorClassifierConfig;
use crate::support::http_retry::{retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES};
use crate::support::thinking::collect_thinking_tags;
use crate::support::uuid6::uuid6;
use crate::tool::ToolCall;
use crate::tool::ToolSpec;
use crate::traits::{LlmClient, LlmResponse, LlmUsage, StreamSink};

use crate::traits::ToolChoiceMode;
use model_spec_core::error::{ProviderError, RetryPolicy};

/// async_openai 错误的分类结果。
enum ApiErrorClass {
    /// 不可重试的结构化错误（立即返回）。
    Final(ProviderError),
    /// 可重试的结构化错误（进入重试循环）。
    Retryable(ProviderError),
    /// 非 ApiError（网络/传输错误，走网络重试判定）。
    Network,
}

/// 将 async_openai 的 API 错误接入统一解析器。
///
/// `OpenAIError::ApiError` 不含 HTTP 状态码（async_openai 0.32），
/// 因此以 status=0 + 结构化字段构造 JSON body 交给统一解析器分类。
fn openai_api_error_to_class(e: &OpenAIError) -> ApiErrorClass {
    let OpenAIError::ApiError(api_err) = e else {
        return ApiErrorClass::Network;
    };
    let body = serde_json::json!({
        "error": {
            "message": api_err.message,
            "type": api_err.r#type,
            "code": api_err.code,
        }
    });
    let parser = crate::error::provider::parser_for("openai");
    let err = parser.parse(0, &[], body.to_string().as_bytes());
    if err.is_retryable() {
        ApiErrorClass::Retryable(err)
    } else {
        ApiErrorClass::Final(err)
    }
}

/// 包装为 `LlmError::Provider`（Box 化，避免 `LlmError` 变大）。
fn into_provider_err(err: ProviderError) -> LlmError {
    LlmError::Provider(Box::new(err))
}

pub(super) fn completion_usage_to_llm(u: &CompletionUsage) -> LlmUsage {
    use crate::traits::{CompletionTokensDetails, PromptTokensDetails};

    LlmUsage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens: u.total_tokens,
        prompt_tokens_details: u
            .prompt_tokens_details
            .as_ref()
            .map(|d| PromptTokensDetails {
                cached_tokens: d.cached_tokens,
                audio_tokens: d.audio_tokens,
            }),
        completion_tokens_details: u.completion_tokens_details.as_ref().map(|d| {
            CompletionTokensDetails {
                reasoning_tokens: d.reasoning_tokens,
                audio_tokens: d.audio_tokens,
                accepted_prediction_tokens: d.accepted_prediction_tokens,
                rejected_prediction_tokens: d.rejected_prediction_tokens,
            }
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
    headers: Option<crate::traits::LlmHeaders>,
    audit_log: Option<Arc<dyn LlmAuditLog>>,
    /// Reasoning effort level passed to the API as `reasoning_effort`.
    reasoning_effort: Option<String>,
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
            parse_thinking_tags: true,
            headers: None,
            audit_log: None,
            reasoning_effort: None,
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
            parse_thinking_tags: true,
            headers: None,
            audit_log: None,
            reasoning_effort: None,
        }
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

    /// Sets the reasoning effort level for reasoning models.
    ///
    /// Valid values: "none", "minimal", "low", "medium", "high", "xhigh".
    /// The value "auto" or `None` means no override (use model default).
    pub fn with_reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }

    /// Sets HTTP headers for LLM requests.
    ///
    /// This allows adding custom headers like X-App-Id, X-Thread-Id, X-Trace-Id
    /// for request tracking and observability.
    pub fn with_headers(mut self, headers: crate::traits::LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets the audit log recorder.
    pub fn with_audit_log(mut self, audit: Arc<dyn LlmAuditLog>) -> Self {
        self.audit_log = Some(audit);
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

    fn thread_id(&self) -> String {
        self.headers
            .as_ref()
            .and_then(|h| h.thread_id.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    fn record_audit(
        &self,
        trace_id: &str,
        entry_type: &str,
        url: &str,
        duration_ms: u64,
        status: u16,
        request: LlmAuditRequest,
        response: Option<LlmAuditResponse>,
        error: Option<String>,
    ) {
        if let Some(ref log) = self.audit_log {
            let entry = build_audit_entry(
                self.thread_id(),
                trace_id.to_string(),
                entry_type,
                self.model.clone(),
                url,
                duration_ms,
                status,
                request,
                response,
                error,
            );
            log.log(entry);
        }
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
    ) -> Result<CreateChatCompletionRequest, LlmError> {
        request::build_chat_request(
            &self.model,
            messages,
            self.tools.as_deref(),
            self.temperature,
            self.tool_choice,
            self.reasoning_effort.as_deref(),
            stream,
        )
    }
}

#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, LlmError> {
        let trace_id = uuid6().to_string();
        let request_id = uuid6().to_string();
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let url = Self::chat_completions_url();
        let audit_start = std::time::Instant::now();
        debug!(
            trace_id = %trace_id,
            request_id = %request_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            tools_count = tools_count,
            temperature = ?self.temperature,
            tool_choice = ?self.tool_choice,
            "OpenAI chat create"
        );

        let tools_json = self
            .tools
            .as_ref()
            .map(|t| serde_json::to_value(t).unwrap_or_default());
        let audit_request = LlmAuditRequest {
            messages: serde_json::json!(messages),
            tools: tools_json,
            parameters: LlmAuditRequestParams {
                temperature: self.temperature,
                stream: false,
                tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
            },
        };

        let mut attempt = 0;
        let response = loop {
            let request = self.build_request(messages, false)?;
            match self.client.chat().create(request).await {
                Ok(response) => break response,
                Err(e) => match openai_api_error_to_class(&e) {
                    ApiErrorClass::Final(provider_err) => {
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(provider_err.to_string()),
                        );
                        return Err(into_provider_err(provider_err));
                    }
                    ApiErrorClass::Retryable(provider_err) => {
                        if attempt < TRANSIENT_HTTP_MAX_RETRIES {
                            let delay = match provider_err.retry_policy {
                                RetryPolicy::RetryAfter(ms) => std::time::Duration::from_millis(ms),
                                _ => retry_backoff_for_attempt(attempt),
                            };
                            tracing::warn!(
                                url = %url,
                                kind = ?provider_err.kind,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %provider_err.message,
                                "OpenAI API request failed, retrying"
                            );

                            tokio::time::sleep(delay).await;
                            attempt += 1;
                            continue;
                        }
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(provider_err.to_string()),
                        );
                        return Err(into_provider_err(provider_err));
                    }
                    ApiErrorClass::Network => {
                        let error_message = e.to_string();
                        let classifier = LlmErrorClassifierConfig::openai();
                        let retryable = classifier
                            .classify_network_error(&error_message)
                            .is_retryable();
                        if retryable && attempt < TRANSIENT_HTTP_MAX_RETRIES {
                            let delay = retry_backoff_for_attempt(attempt);
                            tracing::warn!(
                                url = %url,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %error_message,
                                "OpenAI API request failed, retrying"
                            );

                            tokio::time::sleep(delay).await;
                            attempt += 1;
                            continue;
                        }

                        tracing::warn!(
                            url = %url,
                            attempt = attempt + 1,
                            retryable = retryable,
                            error = %error_message,
                            "OpenAI API request failed without retry"
                        );
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(error_message.clone()),
                        );
                        return Err(LlmError::InvokeFailed(format!(
                            "OpenAI API error: {} (trace_id: {})",
                            error_message, trace_id
                        )));
                    }
                },
            }
        };

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvokeFailed("OpenAI returned no choices".to_string()))?;

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

        // Record audit entry
        if self.audit_log.is_some() {
            let duration_ms = audit_start.elapsed().as_millis() as u64;
            let tools_json = self
                .tools
                .as_ref()
                .map(|t| serde_json::to_value(t).unwrap_or_default());
            let audit_request = LlmAuditRequest {
                messages: serde_json::json!(messages),
                tools: tools_json,
                parameters: LlmAuditRequestParams {
                    temperature: self.temperature,
                    stream: false,
                    tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
                },
            };
            let audit_response = LlmAuditResponse {
                content: content.clone(),
                reasoning_content: reasoning_content.clone(),
                tool_calls: tool_calls
                    .iter()
                    .map(|tc| LlmAuditToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .collect(),
                usage: usage.as_ref().map(|u| LlmAuditUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }),
            };
            self.record_audit(
                &trace_id,
                "chat",
                &url,
                duration_ms,
                200,
                audit_request,
                Some(audit_response),
                None,
            );
        }

        Ok(LlmResponse {
            content,
            reasoning_content,
            tool_calls,
            usage,
            ..Default::default()
        })
    }

    async fn invoke_stream(
        &self,
        messages: &[Message],
        sink: Option<&dyn StreamSink>,
        node_id: &str,
    ) -> Result<LlmResponse, LlmError> {
        if sink.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid6().to_string();
        let request_id = uuid6().to_string();
        let sink = sink.expect("sink must be Some when streaming");
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let url = Self::chat_completions_url();
        let audit_start = std::time::Instant::now();
        debug!(
            trace_id = %trace_id,
            request_id = %request_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            stream = true,
            tools_count = tools_count,
            temperature = ?self.temperature,
            tool_choice = ?self.tool_choice,
            "OpenAI chat create_stream"
        );

        let tools_json = self
            .tools
            .as_ref()
            .map(|t| serde_json::to_value(t).unwrap_or_default());
        let audit_request = LlmAuditRequest {
            messages: serde_json::json!(messages),
            tools: tools_json,
            parameters: LlmAuditRequestParams {
                temperature: self.temperature,
                stream: true,
                tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
            },
        };

        let mut attempt = 0;
        let mut stream = loop {
            let request = self.build_request(messages, true)?;
            match self.client.chat().create_stream(request).await {
                Ok(stream) => break stream,
                Err(e) => match openai_api_error_to_class(&e) {
                    ApiErrorClass::Final(provider_err) => {
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat_stream",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(provider_err.to_string()),
                        );
                        return Err(into_provider_err(provider_err));
                    }
                    ApiErrorClass::Retryable(provider_err) => {
                        if attempt < TRANSIENT_HTTP_MAX_RETRIES {
                            let delay = match provider_err.retry_policy {
                                RetryPolicy::RetryAfter(ms) => std::time::Duration::from_millis(ms),
                                _ => retry_backoff_for_attempt(attempt),
                            };
                            tracing::warn!(
                                url = %url,
                                kind = ?provider_err.kind,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %provider_err.message,
                                "OpenAI stream request failed, retrying"
                            );

                            tokio::time::sleep(delay).await;
                            attempt += 1;
                            continue;
                        }
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat_stream",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(provider_err.to_string()),
                        );
                        return Err(into_provider_err(provider_err));
                    }
                    ApiErrorClass::Network => {
                        let error_message = e.to_string();
                        let classifier = LlmErrorClassifierConfig::openai();
                        let retryable = classifier
                            .classify_network_error(&error_message)
                            .is_retryable();
                        if retryable && attempt < TRANSIENT_HTTP_MAX_RETRIES {
                            let delay = retry_backoff_for_attempt(attempt);
                            tracing::warn!(
                                url = %url,
                                attempt = attempt + 1,
                                max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                                delay_secs = delay.as_secs_f64(),
                                error = %error_message,
                                "OpenAI stream request failed, retrying"
                            );

                            tokio::time::sleep(delay).await;
                            attempt += 1;
                            continue;
                        }

                        tracing::warn!(
                            url = %url,
                            attempt = attempt + 1,
                            retryable = retryable,
                            error = %error_message,
                            "OpenAI stream request failed without retry"
                        );
                        let duration_ms = audit_start.elapsed().as_millis() as u64;
                        self.record_audit(
                            &trace_id,
                            "chat_stream",
                            &url,
                            duration_ms,
                            0,
                            audit_request.clone(),
                            None,
                            Some(error_message.clone()),
                        );
                        return Err(LlmError::InvokeFailed(format!(
                            "OpenAI stream error: {} (trace_id: {})",
                            error_message, trace_id
                        )));
                    }
                },
            }
        };

        let mut acc = stream::StreamAccumulator::new(self.parse_thinking_tags);
        let mut first_chunk_at: Option<std::time::Instant> = None;
        while let Some(result) = stream.next().await {
            let response = result.map_err(|e| {
                LlmError::InvokeFailed(format!(
                    "OpenAI stream error: {} (trace_id: {})",
                    e, trace_id
                ))
            })?;
            if let Some(t) = acc.process_chunk(response, sink, node_id) {
                if first_chunk_at.is_none() {
                    first_chunk_at = Some(t);
                }
            }
        }

        if let Some(t) = acc.flush(sink, node_id) {
            if first_chunk_at.is_none() {
                first_chunk_at = Some(t);
            }
        }
        if let Some(t) = acc.emit_full_if_needed(sink, node_id) {
            if first_chunk_at.is_none() {
                first_chunk_at = Some(t);
            }
        }

        acc.finish_tool_inputs(sink, node_id);

        let result = acc.finish();
        trace!(
            trace_id = %trace_id,
            url = %url,
            reasoning_len = result.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0),
            tool_calls = ?result.tool_calls,
            usage = ?result.usage,
            "OpenAI stream response"
        );

        // Record audit entry
        if self.audit_log.is_some() {
            let duration_ms = audit_start.elapsed().as_millis() as u64;
            let tools_json = self
                .tools
                .as_ref()
                .map(|t| serde_json::to_value(t).unwrap_or_default());
            let audit_request = LlmAuditRequest {
                messages: serde_json::json!(messages),
                tools: tools_json,
                parameters: LlmAuditRequestParams {
                    temperature: self.temperature,
                    stream: true,
                    tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
                },
            };
            let audit_response = LlmAuditResponse {
                content: result.content.clone(),
                reasoning_content: result.reasoning_content.clone(),
                tool_calls: result
                    .tool_calls
                    .iter()
                    .map(|tc| LlmAuditToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .collect(),
                usage: result.usage.as_ref().map(|u| LlmAuditUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }),
            };
            self.record_audit(
                &trace_id,
                "chat_stream",
                &url,
                duration_ms,
                200,
                audit_request,
                Some(audit_response),
                None,
            );
        }

        Ok(LlmResponse {
            content: result.content,
            reasoning_content: result.reasoning_content,
            tool_calls: result.tool_calls,
            usage: result.usage,
            first_chunk_at,
            finish_reason: None,
        })
    }

    async fn list_models(&self) -> Result<Vec<crate::traits::ModelInfo>, LlmError> {
        models::list_models(self.client.config()).await
    }
}
