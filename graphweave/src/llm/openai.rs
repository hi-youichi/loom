//! OpenAI Chat Completions client implementing `LlmClient` (ChatOpenAI).
//!
//! Uses the real OpenAI Chat Completions API. Requires `OPENAI_API_KEY` (or
//! explicit config). Optional tools can be set for function/tool calling;
//! when present, API may return `tool_calls` in the response.
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
use crate::llm::{LlmClient, LlmResponse, LlmUsage};
use crate::memory::uuid6;
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;
use crate::tool_source::{ToolSource, ToolSourceError, ToolSpec};

use async_openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCalls, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage, ChatCompletionTool,
        ChatCompletionToolChoiceOption, ChatCompletionTools, CreateChatCompletionRequestArgs,
        FunctionObject, ToolChoiceOptions,
    },
    Client,
};

use super::ToolChoiceMode;

/// OpenAI Chat Completions client implementing `LlmClient` (aligns with LangChain ChatOpenAI).
///
/// Uses `OPENAI_API_KEY` from the environment by default; or provide
/// config via `ChatOpenAI::with_config`. Optionally set tools (e.g. from
/// `ToolSource::list_tools()`) to enable tool_calls in the response.
///
/// **Interaction**: Implements `LlmClient`; used by ThinkNode.
pub struct ChatOpenAI {
    client: Client<OpenAIConfig>,
    model: String,
    tools: Option<Vec<ToolSpec>>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
}

impl ChatOpenAI {
    /// Build client with default config (API key from `OPENAI_API_KEY` env).
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
        }
    }

    /// Build client with custom config (e.g. custom API key or base URL).
    pub fn with_config(config: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            client: Client::with_config(config),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
        }
    }

    /// Build client with tools from the given ToolSource.
    ///
    /// Calls `tool_source.list_tools().await` and sets them via `with_tools`.
    /// Use the same ToolSource for ActNode so the LLM and execution see the same tools
    /// (e.g. memory + MCP when exa_api_key is set).
    ///
    /// **Interaction**: Caller builds a ToolSource (e.g. AggregateToolSource with memory
    /// and optional MCP); this constructor fetches the full list and enables tool_calls.
    pub async fn new_with_tool_source(
        config: OpenAIConfig,
        model: impl Into<String>,
        tool_source: &dyn ToolSource,
    ) -> Result<Self, ToolSourceError> {
        let tools = tool_source.list_tools().await?;
        Ok(Self::with_config(config, model).with_tools(tools))
    }

    /// Set tools for this completion (enables tool_calls in response).
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set temperature (0–2). Lower values are more deterministic.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set tool choice mode (auto, none, required). Overrides API default when tools are present.
    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
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
                    ChatCompletionRequestMessage::Assistant((s.as_str()).into())
                }
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let trace_id = uuid6().to_string();
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
            args.tool_choice(ChatCompletionToolChoiceOption::Mode(ToolChoiceOptions::Required));
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

        let request = args.build().map_err(|e| {
            AgentError::ExecutionFailed(format!("OpenAI request build failed: {}", e))
        })?;

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
        if let Ok(js) = serde_json::to_string_pretty(&request) {
            trace!(trace_id = %trace_id, url = %url, request = %js, "OpenAI request body");
        } else {
            trace!(trace_id = %trace_id, url = %url, request = ?request, "OpenAI request body (debug)");
        }

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("OpenAI API error: {}", e)))?;

        if let Ok(js) = serde_json::to_string_pretty(&response) {
            trace!(trace_id = %trace_id, url = %url, response = %js, "OpenAI response body");
        } else {
            trace!(trace_id = %trace_id, url = %url, response = ?response, "OpenAI response body (debug)");
        }

        let choice =
            response.choices.into_iter().next().ok_or_else(|| {
                AgentError::ExecutionFailed("OpenAI returned no choices".to_string())
            })?;

        let msg = choice.message;
        let content = msg.content.unwrap_or_default();
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
            tool_calls,
            usage,
        })
    }

    /// Streaming variant: sends message chunks as they arrive from OpenAI.
    ///
    /// Uses OpenAI's streaming API to receive tokens incrementally. Each content
    /// delta is sent through `chunk_tx` as a `MessageChunk`. Tool calls are
    /// accumulated from stream chunks and returned in the final `LlmResponse`.
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        // If no streaming requested, use non-streaming path
        if chunk_tx.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid6().to_string();
        let chunk_tx = chunk_tx.unwrap();
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
            args.tool_choice(ChatCompletionToolChoiceOption::Mode(ToolChoiceOptions::Required));
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

        let request = args.build().map_err(|e| {
            AgentError::ExecutionFailed(format!("OpenAI request build failed: {}", e))
        })?;

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
        if let Ok(js) = serde_json::to_string_pretty(&request) {
            trace!(trace_id = %trace_id, url = %url, request = %js, "OpenAI stream request body");
        } else {
            trace!(trace_id = %trace_id, url = %url, request = ?request, "OpenAI stream request body (debug)");
        }

        let mut stream = self
            .client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("OpenAI stream error: {}", e)))?;

        // Accumulate content, tool calls, and usage from stream
        let mut full_content = String::new();
        // Track if we sent any content chunk (avoid duplicating at end for non-incremental APIs).
        let mut sent_any_content = false;
        // Tool calls accumulator: index -> (id, name, arguments)
        let mut tool_call_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new();
        let mut stream_usage: Option<LlmUsage> = None;

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
                        // Send chunk to channel (ignore errors if receiver dropped)
                        let _ = chunk_tx
                            .send(MessageChunk {
                                content: content.clone(),
                            })
                            .await;
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
                            .send(MessageChunk {
                                content: full_content.clone(),
                            })
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
                .send(MessageChunk {
                    content: full_content.clone(),
                })
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
        trace!(
            trace_id = %trace_id,
            url = %url,
            content = %full_content,
            tool_calls = ?tool_calls,
            usage = ?stream_usage,
            "OpenAI stream response"
        );

        Ok(LlmResponse {
            content: full_content,
            tool_calls,
            usage: stream_usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmClient;
    use crate::message::Message;

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

    /// **Scenario**: Builder chain with_tools and with_temperature builds without panic.
    #[test]
    fn chat_openai_with_tools_and_temperature_builder() {
        let tools = vec![ToolSpec {
            name: "get_time".into(),
            description: None,
            input_schema: serde_json::json!({}),
        }];
        let _ = ChatOpenAI::new("gpt-4")
            .with_tools(tools)
            .with_temperature(0.5f32);
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

    /// **Scenario**: invoke() against real OpenAI API returns Ok when OPENAI_API_KEY is set.
    /// Given a client with default config and valid API key in env, when we call invoke() with one user message,
    /// then the result is Ok and the response has content or tool_calls (model-dependent).
    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY; run with: cargo test -p graphweave invoke_with_real_api -- --ignored"]
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
    #[ignore = "Requires OPENAI_API_KEY; run with: cargo test -p graphweave invoke_stream_with_real_api -- --ignored"]
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
