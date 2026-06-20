//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use tracing::{debug, trace};

use loom_graph::{run_cancellable, Next, Node, RunContext};
use loom_llm::{GraphError, LlmClient, LlmProvider, LlmResponse, LlmUsage, Message, ToolCall};
use loom_stream::{MessageChunk, StreamEvent, StreamEventSink, StreamMetadata, StreamMode};
use loom_tier::load_provider_configs;
use loom_tier::resolve_tier_intelligent;
use loom_types::state::{ModelConfig, ReActState};
use model_spec_core::ModelTier;

pub struct ThinkNode {
    provider: Arc<dyn LlmProvider>,
    client_cache: DashMap<String, Arc<dyn LlmClient>>,
}

impl ThinkNode {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            client_cache: DashMap::new(),
        }
    }

    async fn resolve_client(&self, model_config: &ModelConfig) -> Result<Arc<dyn LlmClient>, GraphError> {
        let model = if !model_config.model_id.is_empty() {
            model_config.model_id.clone()
        } else if model_config.tier != ModelTier::None {
            let providers = load_provider_configs().ok_or_else(|| {
                GraphError::ExecutionFailed("no provider configs for tier resolution".into())
            })?;
            let entry = resolve_tier_intelligent(
                self.provider.provider_name(),
                model_config.tier,
                &providers,
            )
            .await
            .ok_or_else(|| {
                GraphError::ExecutionFailed(format!(
                    "no model found for tier {:?} on provider '{}'",
                    model_config.tier,
                    self.provider.provider_name()
                ))
            })?;
            entry.id
        } else {
            self.provider.default_model().to_string()
        };

        if let Some(client) = self.client_cache.get(&model) {
            return Ok(Arc::clone(client.value()));
        }

        let client = self.provider.create_client(&model)?;
        let client = Arc::from(client);
        self.client_cache.entry(model).or_insert_with(|| Arc::clone(&client));
        Ok(client)
    }

    /// Emits stream events after the LLM returns and before state is committed (messages, tool calls).
    /// `Usage` is sent separately after [`ReActState::apply_think`] to match prior event ordering.
    #[allow(clippy::too_many_arguments)]
    async fn emit_post_response_events(
        &self,
        ctx: &RunContext<ReActState>,
        content: &str,
        should_stream: bool,
        streamed_chunks: u64,
        tool_calls: &[ToolCall],
        should_stream_tools: bool,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<(), GraphError> {
        let Some(stream_tx) = ctx.stream_tx.as_ref() else {
            return Ok(());
        };

        if should_stream && !content.is_empty() && streamed_chunks == 0 {
            let _ = stream_tx
                .try_send(StreamEvent::Messages {
                    chunk: MessageChunk::message(content.to_string()),
                    metadata: StreamMetadata {
                        loom_node: self.id().to_string(),
                        namespace: None,
                    },
                });
        }

        if should_stream_tools && !tool_calls.is_empty() {
            for tc in tool_calls {
                if is_cancelled() {
                    return Err(GraphError::Cancelled);
                }
                let args: Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or_else(|_| Value::String(tc.arguments.clone()));
                let _ = stream_tx
                    .try_send(StreamEvent::ToolCall {
                        call_id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: args,
                    });
            }
        }

        Ok(())
    }

    async fn emit_usage_event(
        &self,
        ctx: &RunContext<ReActState>,
        call_start: Instant,
        first_token_at: Option<Instant>,
        usage: &LlmUsage,
    ) {
        let Some(stream_tx) = ctx.stream_tx.as_ref() else {
            return;
        };
        let (prefill_duration, decode_duration) = match first_token_at {
            Some(ft) => {
                let prefill = ft.duration_since(call_start);
                let decode = call_start.elapsed().saturating_sub(prefill);
                (Some(prefill), Some(decode))
            }
            None => (None, None),
        };
        trace!(
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            total_tokens = usage.total_tokens,
            ?prefill_duration,
            ?decode_duration,
            "think: stream usage"
        );
        let cached_tokens = usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);
        let _ = stream_tx
            .try_send(StreamEvent::Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                cached_tokens,
                prefill_duration,
                decode_duration,
            });
    }
}

async fn invoke_think_llm(
    llm: &Arc<dyn LlmClient>,
    messages: &[Message],
    should_stream: bool,
    _should_stream_tools: bool,
    stream_tx: tokio::sync::mpsc::Sender<StreamEvent<ReActState>>,
    node_id: &str,
) -> Result<(LlmResponse, u64, Option<Instant>), GraphError> {
    if !should_stream {
        // No streaming: skip sink entirely. Tool deltas were never consumed anywhere
        // (only drained by the tool_forward task) and are removed in this refactor.
        let response = llm.invoke(messages).await?;
        return Ok((response, 0, None));
    }

    // New streaming path: LLM calls sink.try_send_message() directly. No intermediate
    // channel, no separate forwarder task, no .await on send inside LLM.
    let sink = StreamEventSink::new(stream_tx, None);

    let result = llm.invoke_stream(messages, Some(&sink), node_id).await;
    let response = result?;
    // We don't have a per-chunk count anymore (no forwarder). Use first_chunk_at as
    // a proxy: at least one chunk was forwarded iff first_chunk_at is Some.
    let streamed_chunks: u64 = u64::from(response.first_chunk_at.is_some());
    let first_token_at = response.first_chunk_at;
    Ok((response, streamed_chunks, first_token_at))
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), GraphError> {
        let llm = self.resolve_client(&state.model_config).await?;
        let response = llm.invoke(&state.messages).await?;
        let new_state = state.apply_think(
            response.content,
            response.reasoning_content,
            response.tool_calls,
            response.usage,
        );
        Ok((new_state, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: ReActState,
        ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), GraphError> {
        let is_cancelled = || {
            ctx.cancellation
                .as_ref()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
        };
        if is_cancelled() {
            return Err(GraphError::Cancelled);
        }
        let should_stream =
            ctx.stream_mode.contains(&StreamMode::Messages) && ctx.stream_tx.is_some();
        let should_stream_tools = (ctx.stream_mode.contains(&StreamMode::Tools)
            || ctx.stream_mode.contains(&StreamMode::Debug))
            && ctx.stream_tx.is_some();

        debug!(
            messages = state.messages.len(),
            should_stream, should_stream_tools, "think: invoking LLM"
        );

        let call_start = Instant::now();
        let llm = self.resolve_client(&state.model_config).await?;
        let llm_call = async {
            if should_stream || should_stream_tools {
                invoke_think_llm(
                    &llm,
                    &state.messages,
                    should_stream,
                    should_stream_tools,
                    ctx.stream_tx.as_ref().unwrap().clone(),
                    self.id(),
                )
                .await
            } else {
                Ok((
                    llm.invoke(&state.messages).await?,
                    0u64,
                    None::<Instant>,
                ))
            }
        };

        let (response, streamed_chunks, first_token_at) = match run_cancellable(
            llm_call,
            ctx.cancellation.as_ref(),
        )
        .await
        {
            Ok(Ok(triple)) => triple,
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(e),
        };

        if is_cancelled() {
            return Err(GraphError::Cancelled);
        }

        let LlmResponse {
            content: resp_content,
            reasoning_content,
            tool_calls,
            usage,
            first_chunk_at: _,
        } = response;

        let content = if !resp_content.is_empty() {
            resp_content
        } else if let Some(reasoning) = &reasoning_content {
            reasoning.clone()
        } else {
            String::new()
        };

        trace!(
            content_len = content.len(),
            tool_calls = tool_calls.len(),
            "think: LLM response ready"
        );

        self.emit_post_response_events(
            ctx,
            &content,
            should_stream,
            streamed_chunks,
            &tool_calls,
            should_stream_tools,
            is_cancelled,
        )
        .await?;

        let new_state = state.apply_think(content, reasoning_content, tool_calls, usage);

        if let Some(ref u) = new_state.usage {
            self.emit_usage_event(ctx, call_start, first_token_at, u)
                .await;
        }

        Ok((new_state, Next::Continue))
    }
}
