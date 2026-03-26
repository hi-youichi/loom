//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use crate::cli_run::ActiveOperationKind;
use crate::error::AgentError;
use crate::graph::{run_cancellable, Next, RunContext};
use crate::llm::{LlmClient, LlmResponse, ToolCallDelta};
use crate::message::Message;
use crate::state::{ReActState, ToolCall};
use crate::stream::{
    ChunkToStreamSender, MessageChunk, StreamEvent, StreamMetadata, StreamMode,
};
use crate::Node;

pub struct ThinkNode {
    llm: Arc<dyn LlmClient>,
}

impl ThinkNode {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Emits stream events after the LLM returns and before state is committed (messages, tool calls).
    /// `Usage` is sent separately after [`ReActState::apply_think`] to match prior event ordering.
    async fn emit_post_response_events(
        &self,
        ctx: &RunContext<ReActState>,
        content: &str,
        used_fallback: bool,
        should_stream: bool,
        streamed_chunks: u64,
        tool_calls: &[ToolCall],
        should_stream_tools: bool,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<(), AgentError> {
        let Some(stream_tx) = ctx.stream_tx.as_ref() else {
            return Ok(());
        };

        if used_fallback {
            let fallback_chunk = MessageChunk::message(content.to_string());
            let _ = stream_tx
                .send(StreamEvent::Messages {
                    chunk: fallback_chunk,
                    metadata: StreamMetadata {
                        loom_node: self.id().to_string(),
                        namespace: None,
                    },
                })
                .await;
        }

        if should_stream && !used_fallback && !content.is_empty() && streamed_chunks == 0 {
            let _ = stream_tx
                .send(StreamEvent::Messages {
                    chunk: MessageChunk::message(content.to_string()),
                    metadata: StreamMetadata {
                        loom_node: self.id().to_string(),
                        namespace: None,
                    },
                })
                .await;
        }

        if should_stream_tools && !tool_calls.is_empty() {
            for tc in tool_calls {
                if is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
                let args: Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or_else(|_| Value::String(tc.arguments.clone()));
                let _ = stream_tx
                    .send(StreamEvent::ToolCall {
                        call_id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: args,
                    })
                    .await;
            }
        }

        Ok(())
    }

    async fn emit_usage_event(
        &self,
        ctx: &RunContext<ReActState>,
        call_start: Instant,
        first_token_at: Option<Instant>,
        usage: &crate::llm::LlmUsage,
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
        let _ = stream_tx
            .send(StreamEvent::Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                prefill_duration,
                decode_duration,
            })
            .await;
    }
}

async fn invoke_think_llm(
    llm: &Arc<dyn LlmClient>,
    messages: &[Message],
    should_stream: bool,
    should_stream_tools: bool,
    stream_tx: mpsc::Sender<StreamEvent<ReActState>>,
    node_id: &str,
) -> Result<(LlmResponse, u64, Option<Instant>), AgentError> {
    let (chunk_tx, chunk_rx) = if should_stream {
        let adapter = ChunkToStreamSender::new(stream_tx.clone(), node_id);
        let (tx, rx) = adapter.channel();
        (Some(tx), Some((adapter, rx)))
    } else {
        (None, None)
    };

    let (tool_delta_tx, tool_delta_rx) = if should_stream_tools {
        let (tx, rx) = mpsc::channel::<ToolCallDelta>(64);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let stream_tx_tool = stream_tx.clone();
    let tool_forward = async move {
        if let Some(mut rx) = tool_delta_rx {
            while let Some(delta) = rx.recv().await {
                let _ = stream_tx_tool
                    .send(StreamEvent::ToolCallChunk {
                        call_id: delta.call_id,
                        name: delta.name,
                        arguments_delta: delta.arguments_delta,
                    })
                    .await;
            }
        }
    };

    let msg_forward = async move {
        if let Some((adapter, rx)) = chunk_rx {
            adapter.forward(rx).await
        } else {
            (0, None)
        }
    };

    let (result, (forwarded_chunks, first_token_at), _) = tokio::join!(
        llm.invoke_stream_with_tool_delta(messages, chunk_tx, tool_delta_tx),
        msg_forward,
        tool_forward,
    );
    Ok((result?, forwarded_chunks as u64, first_token_at))
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let response = self.llm.invoke(&state.messages).await?;
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
    ) -> Result<(ReActState, Next), AgentError> {
        let is_cancelled = || {
            ctx.cancellation
                .as_ref()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
        };
        if is_cancelled() {
            return Err(AgentError::Cancelled);
        }
        let should_stream =
            ctx.stream_mode.contains(&StreamMode::Messages) && ctx.stream_tx.is_some();
        let should_stream_tools = (ctx.stream_mode.contains(&StreamMode::Tools)
            || ctx.stream_mode.contains(&StreamMode::Debug))
            && ctx.stream_tx.is_some();

        debug!(
            messages = state.messages.len(),
            should_stream,
            should_stream_tools,
            "think: invoking LLM"
        );

        let call_start = Instant::now();
        let llm_call = async {
            if should_stream || should_stream_tools {
                invoke_think_llm(
                    &self.llm,
                    &state.messages,
                    should_stream,
                    should_stream_tools,
                    ctx.stream_tx.as_ref().unwrap().clone(),
                    self.id(),
                )
                .await
            } else {
                Ok((
                    self.llm.invoke(&state.messages).await?,
                    0u64,
                    None::<Instant>,
                ))
            }
        };

        let (response, streamed_chunks, first_token_at) = match run_cancellable(
            llm_call,
            ctx.cancellation.as_ref(),
            ctx.run_cancellation.as_ref(),
            ActiveOperationKind::Llm,
        )
        .await
        {
            Ok(Ok(triple)) => triple,
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(e),
        };

        if is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        let crate::llm::LlmResponse {
            content: resp_content,
            reasoning_content,
            tool_calls,
            usage,
        } = response;

        let used_fallback = resp_content.is_empty() && tool_calls.is_empty();
        if used_fallback {
            warn!("think: empty LLM response (no text, no tool calls); using fallback message");
        }

        let content = if used_fallback {
            "No text response from the model. Please try again or check the API.".to_string()
        } else {
            resp_content
        };

        trace!(
            content_len = content.len(),
            tool_calls = tool_calls.len(),
            used_fallback,
            "think: LLM response ready"
        );

        self.emit_post_response_events(
            ctx,
            &content,
            used_fallback,
            should_stream,
            streamed_chunks,
            &tool_calls,
            should_stream_tools,
            is_cancelled,
        )
        .await?;

        let new_state = state.apply_think(
            content,
            reasoning_content,
            tool_calls,
            usage,
        );

        if let Some(ref u) = new_state.usage {
            self.emit_usage_event(ctx, call_start, first_token_at, u).await;
        }

        Ok((new_state, Next::Continue))
    }
}
