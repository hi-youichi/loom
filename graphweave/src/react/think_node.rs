//! Think node: read messages, call LLM, write assistant message and optional tool_calls.
//!
//! ThinkNode holds an LLM client (e.g. MockLlm or `Box<dyn LlmClient>`), implements
//! `Node<ReActState>`; run reads state.messages, calls LLM, appends one assistant message
//! and sets state.tool_calls from the response (empty when no tools).
//!
//! # Streaming Support
//!
//! ThinkNode implements `run_with_context` to support Messages streaming. When
//! `stream_mode` contains `StreamMode::Messages`, it uses `LlmClient::invoke_stream()`
//! and forwards `MessageChunk` tokens to the stream channel as `StreamEvent::Messages`.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::LlmClient;
use crate::message::Message;
use crate::state::ReActState;
use crate::stream::{MessageChunk, StreamEvent, StreamMetadata, StreamMode};
use crate::Node;

/// Think node: one ReAct step that produces assistant message and optional tool_calls.
///
/// Reads `state.messages`, calls the LLM, appends one assistant message and sets
/// `state.tool_calls` from the response. When the LLM returns no tool_calls, the
/// graph can end after observe. Does not call ToolSource::list_tools in this minimal
/// version (prompt can be fixed).
///
/// **Interaction**: Implements `Node<ReActState>`; used by StateGraph. Holds
/// `Arc<dyn LlmClient>` so the same LLM can be shared with the compression subgraph.
pub struct ThinkNode {
    /// LLM client used to produce assistant message and optional tool_calls.
    llm: Arc<dyn LlmClient>,
}

impl ThinkNode {
    /// Creates a Think node with the given LLM client.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    /// Reads state.messages, calls LLM, appends assistant message and sets tool_calls.
    /// Returns Next::Continue to follow linear edge order (e.g. think → act).
    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let response = self.llm.invoke(&state.messages).await?;
        let mut messages = state.messages;
        messages.push(Message::Assistant(response.content));
        let (usage, total_usage) = match (&state.total_usage, &response.usage) {
            (Some(t), Some(u)) => (
                response.usage.clone(),
                Some(crate::llm::LlmUsage {
                    prompt_tokens: t.prompt_tokens + u.prompt_tokens,
                    completion_tokens: t.completion_tokens + u.completion_tokens,
                    total_tokens: t.total_tokens + u.total_tokens,
                }),
            ),
            (None, Some(u)) => (response.usage.clone(), Some(u.clone())),
            (Some(t), None) => (None, Some(t.clone())),
            (None, None) => (None, None),
        };
        let message_count_after_last_think = Some(messages.len());
        let new_state = ReActState {
            messages,
            tool_calls: response.tool_calls,
            tool_results: state.tool_results,
            turn_count: state.turn_count,
            approval_result: state.approval_result,
            usage,
            total_usage,
            message_count_after_last_think,
        };
        Ok((new_state, Next::Continue))
    }

    /// Streaming-aware variant: when `stream_mode` contains `Messages`, uses
    /// `invoke_stream()` and forwards chunks to the stream channel.
    ///
    /// Token chunks are sent as `StreamEvent::Messages` with metadata containing
    /// the node id ("think"). This enables real-time LLM output display (typewriter effect).
    async fn run_with_context(
        &self,
        state: ReActState,
        ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        let should_stream =
            ctx.stream_mode.contains(&StreamMode::Messages) && ctx.stream_tx.is_some();

        let response = if should_stream {
            // Create internal channel for message chunks
            let (chunk_tx, mut chunk_rx) = mpsc::channel::<MessageChunk>(128);

            // Get a clone of the stream sender for the forwarding task
            let stream_tx = ctx.stream_tx.clone().unwrap();
            let node_id = self.id().to_string();

            // Spawn task to forward chunks as StreamEvent::Messages
            let forward_task = tokio::spawn(async move {
                while let Some(chunk) = chunk_rx.recv().await {
                    let event = StreamEvent::Messages {
                        chunk,
                        metadata: StreamMetadata {
                            graphweave_node: node_id.clone(),
                        },
                    };
                    // Ignore send errors (consumer may have dropped)
                    let _ = stream_tx.send(event).await;
                }
            });

            // Call LLM with streaming
            let result = self
                .llm
                .invoke_stream(&state.messages, Some(chunk_tx))
                .await;

            // Wait for forwarding task to complete (chunk_tx is dropped after invoke_stream)
            let _ = forward_task.await;

            result?
        } else {
            // Non-streaming path: use regular invoke
            self.llm.invoke(&state.messages).await?
        };

        // When the model returns no content and no tool calls, still push a fallback reply
        // so the user sees a response (e.g. some APIs return empty content in stream).
        let used_fallback = response.content.is_empty() && response.tool_calls.is_empty();
        let content = if used_fallback {
            "No text response from the model. Please try again or check the API.".to_string()
        } else {
            response.content
        };

        // So that streaming clients see the fallback, emit it as a Messages event when streaming.
        if used_fallback && ctx.stream_tx.is_some() {
            let fallback_chunk = MessageChunk {
                content: content.clone(),
            };
            let _ = ctx
                .stream_tx
                .as_ref()
                .unwrap()
                .send(StreamEvent::Messages {
                    chunk: fallback_chunk,
                    metadata: StreamMetadata {
                        graphweave_node: self.id().to_string(),
                    },
                })
                .await;
        }

        let mut messages = state.messages;
        messages.push(Message::Assistant(content));
        let (usage, total_usage) = match (&state.total_usage, &response.usage) {
            (Some(t), Some(u)) => (
                response.usage.clone(),
                Some(crate::llm::LlmUsage {
                    prompt_tokens: t.prompt_tokens + u.prompt_tokens,
                    completion_tokens: t.completion_tokens + u.completion_tokens,
                    total_tokens: t.total_tokens + u.total_tokens,
                }),
            ),
            (None, Some(u)) => (response.usage.clone(), Some(u.clone())),
            (Some(t), None) => (None, Some(t.clone())),
            (None, None) => (None, None),
        };
        let message_count_after_last_think = Some(messages.len());
        let new_state = ReActState {
            messages,
            tool_calls: response.tool_calls,
            tool_results: state.tool_results,
            turn_count: state.turn_count,
            approval_result: state.approval_result,
            usage,
            total_usage,
            message_count_after_last_think,
        };

        // Emit token usage when available so CLI can print when --verbose
        if let (Some(ref tx), Some(ref u)) = (ctx.stream_tx.as_ref(), response.usage.as_ref()) {
            let _ = tx
                .send(StreamEvent::Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                })
                .await;
        }

        Ok((new_state, Next::Continue))
    }
}
