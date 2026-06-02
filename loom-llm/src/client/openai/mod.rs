//! OpenAI Chat Completions client via `async_openai`.

mod models;
mod request;
mod stream;

#[cfg(test)]
mod tests;

use async_openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCalls, CompletionUsage, CreateChatCompletionRequest,
        ChatCompletionRequestMessage, ChatCompletionToolChoiceOption, ChatCompletionTools,
    },
    Client,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, trace};

use crate::types::error::LlmError;
use crate::types::message::Message;
use crate::traits::{
    LlmClient, LlmResponse, LlmUsage, ToolChoiceMode, ToolCallDelta, LlmHeaders,
    ModelInfo, PromptTokensDetails, CompletionTokensDetails,
};
use crate::tool_source::ToolSpec;

use super::super::audit::{
    build_audit_entry, LlmAuditLog, LlmAuditRequest, LlmAuditRequestParams, LlmAuditResponse,
    LlmAuditToolCall, LlmAuditUsage,
};

pub(super) fn completion_usage_to_llm(u: &CompletionUsage) -> LlmUsage {
    LlmUsage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens: u.total_tokens,
        prompt_tokens_details: u.prompt_tokens_details.as_ref().map(|d| PromptTokensDetails {
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
pub struct ChatOpenAI {
    client: Client<OpenAIConfig>,
    model: String,
    tools: Option<Vec<ToolSpec>>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    parse_thinking_tags: bool,
    headers: Option<LlmHeaders>,
    audit_log: Option<Arc<dyn LlmAuditLog>>,
}

impl ChatOpenAI {
    /// Builds a client with the default OpenAI configuration.
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
        }
    }

    /// Builds a client with an explicit OpenAI configuration.
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
        }
    }

    /// Sets the tools advertised to the model.
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets the tool choice mode.
    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
        self
    }

    /// Enables parsing of thinking-tag segments in streamed output.
    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    /// Sets HTTP headers for LLM requests.
    pub fn with_headers(mut self, headers: LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets the audit log recorder.
    pub fn with_audit_log(mut self, audit: Arc<dyn LlmAuditLog>) -> Self {
        self.audit_log = Some(audit);
        self
    }

    fn thread_id(&self) -> String {
        self.headers.as_ref().and_then(|h| h.thread_id.as_ref()).cloned().unwrap_or_default()
    }

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

    /// Chat completions URL for logging.
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
            stream,
        )
    }
}

#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, LlmError> {
        let trace_id = uuid::Uuid::new_v4().to_string();
        let url = Self::chat_completions_url();
        let audit_start = std::time::Instant::now();
        
        debug!(trace_id = %trace_id, url = %url, model = %self.model, message_count = messages.len(), "OpenAI chat create");

        let tools_json = self.tools.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default());
        let audit_request = LlmAuditRequest {
            messages: serde_json::json!(messages),
            tools: tools_json,
            parameters: LlmAuditRequestParams {
                temperature: self.temperature,
                stream: false,
                tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
            },
        };

        let request = self.build_request(messages, false)?;
        let response = self.client.chat().create(request).await.map_err(|e| {
            let duration_ms = audit_start.elapsed().as_millis() as u64;
            self.record_audit(&trace_id, "chat", &url, duration_ms, 0, audit_request.clone(), None, Some(e.to_string()));
            LlmError::RequestFailed(e.to_string())
        })?;

        let choice = response.choices.into_iter().next()
            .ok_or_else(|| LlmError::RequestFailed("OpenAI returned no choices".to_string()))?;

        let msg = choice.message;
        let content = msg.content.unwrap_or_default();
        let reasoning_content = super::super::thinking::collect_thinking_tags(&content);
        
        use crate::types::message::AssistantToolCall;
        let tool_calls: Vec<AssistantToolCall> = msg.tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                if let ChatCompletionMessageToolCalls::Function(f) = tc {
                    Some(AssistantToolCall {
                        id: f.id,
                        name: f.function.name,
                        arguments: f.function.arguments,
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
            let audit_response = LlmAuditResponse {
                content: content.clone(),
                reasoning_content: reasoning_content.clone(),
                tool_calls: tool_calls.iter().map(|tc| LlmAuditToolCall {
                    id: Some(tc.id.clone()),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                }).collect(),
                usage: usage.as_ref().map(|u| LlmAuditUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }),
            };
            self.record_audit(&trace_id, "chat", &url, duration_ms, 200, audit_request, Some(audit_response), None);
        }

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
        chunk_tx: Option<mpsc::Sender<super::super::traits::MessageChunk>>,
    ) -> Result<LlmResponse, LlmError> {
        self.invoke_stream_with_tool_delta(messages, chunk_tx, None).await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<super::super::traits::MessageChunk>>,
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, LlmError> {
        if chunk_tx.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid::Uuid::new_v4().to_string();
        let url = Self::chat_completions_url();
        let audit_start = std::time::Instant::now();

        let tools_json = self.tools.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default());
        let audit_request = LlmAuditRequest {
            messages: serde_json::json!(messages),
            tools: tools_json,
            parameters: LlmAuditRequestParams {
                temperature: self.temperature,
                stream: true,
                tool_choice: self.tool_choice.map(|m| format!("{:?}", m)),
            },
        };

        let request = self.build_request(messages, true)?;
        let mut stream = self.client.chat().create_stream(request).await.map_err(|e| {
            let duration_ms = audit_start.elapsed().as_millis() as u64;
            self.record_audit(&trace_id, "chat_stream", &url, duration_ms, 0, audit_request.clone(), None, Some(e.to_string()));
            LlmError::RequestFailed(e.to_string())
        })?;

        use crate::traits::MessageChunk;
        let chunk_tx = chunk_tx.expect("chunk_tx must be Some");
        let mut acc = stream::StreamAccumulator::new(self.parse_thinking_tags);
        
        while let Some(result) = stream.next().await {
            let response = result.map_err(|e| LlmError::RequestFailed(format!("OpenAI stream error: {}", e)))?;
            acc.process_chunk(response, &chunk_tx, tool_delta_tx.as_ref()).await;
        }

        acc.flush(&chunk_tx).await;
        acc.emit_full_if_needed(&chunk_tx).await;

        let result = acc.finish();
        
        trace!(trace_id = %trace_id, url = %url, reasoning_len = result.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0), tool_calls = ?result.tool_calls, usage = ?result.usage, "OpenAI stream response");

        // Record audit entry
        if self.audit_log.is_some() {
            let duration_ms = audit_start.elapsed().as_millis() as u64;
            let audit_response = LlmAuditResponse {
                content: result.content.clone(),
                reasoning_content: result.reasoning_content.clone(),
                tool_calls: result.tool_calls.iter().map(|tc| LlmAuditToolCall {
                    id: Some(tc.id.clone()),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                }).collect(),
                usage: result.usage.as_ref().map(|u| LlmAuditUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }),
            };
            self.record_audit(&trace_id, "chat_stream", &url, duration_ms, 200, audit_request, Some(audit_response), None);
        }

        Ok(LlmResponse {
            content: result.content,
            reasoning_content: result.reasoning_content,
            tool_calls: result.tool_calls,
            usage: result.usage,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        models::list_models(self.client.config()).await
    }
}