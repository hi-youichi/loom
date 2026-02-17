//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::LlmClient;
use crate::message::Message;
use crate::state::{ReActState, ToolCall};
use crate::stream::{ChunkToStreamSender, MessageChunk, StreamEvent, StreamMetadata, StreamMode};
use crate::Node;

pub struct ThinkNode {
    llm: Arc<dyn LlmClient>,
}

impl ThinkNode {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

fn compute_usage(
    state: &ReActState,
    response_usage: &Option<crate::llm::LlmUsage>,
) -> (Option<crate::llm::LlmUsage>, Option<crate::llm::LlmUsage>) {
    match (&state.total_usage, response_usage) {
        (Some(t), Some(u)) => (
            response_usage.clone(),
            Some(crate::llm::LlmUsage {
                prompt_tokens: t.prompt_tokens + u.prompt_tokens,
                completion_tokens: t.completion_tokens + u.completion_tokens,
                total_tokens: t.total_tokens + u.total_tokens,
            }),
        ),
        (None, Some(u)) => (response_usage.clone(), Some(u.clone())),
        (Some(t), None) => (None, Some(t.clone())),
        (None, None) => (None, None),
    }
}

fn apply_think_response(
    state: ReActState,
    content: String,
    tool_calls: Vec<ToolCall>,
    response_usage: Option<crate::llm::LlmUsage>,
) -> ReActState {
    let (usage, total_usage) = compute_usage(&state, &response_usage);
    let mut messages = state.messages;
    messages.push(Message::Assistant(content));
    let message_count_after_last_think = Some(messages.len());
    ReActState {
        messages,
        tool_calls,
        tool_results: state.tool_results,
        turn_count: state.turn_count,
        approval_result: state.approval_result,
        usage,
        total_usage,
        message_count_after_last_think,
    }
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let response = self.llm.invoke(&state.messages).await?;
        let new_state =
            apply_think_response(state, response.content, response.tool_calls, response.usage);
        Ok((new_state, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: ReActState,
        ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        let should_stream =
            ctx.stream_mode.contains(&StreamMode::Messages) && ctx.stream_tx.is_some();

        let response = if should_stream {
            let stream_tx = ctx.stream_tx.clone().unwrap();
            let adapter = ChunkToStreamSender::new(stream_tx, self.id());
            let (chunk_tx, chunk_rx) = adapter.channel();
            let (result, ()) = tokio::join!(
                self.llm.invoke_stream(&state.messages, Some(chunk_tx)),
                adapter.forward(chunk_rx),
            );
            result?
        } else {
            self.llm.invoke(&state.messages).await?
        };

        let used_fallback = response.content.is_empty() && response.tool_calls.is_empty();
        let content = if used_fallback {
            "No text response from the model. Please try again or check the API.".to_string()
        } else {
            response.content
        };

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

        let new_state = apply_think_response(
            state,
            content,
            response.tool_calls,
            response.usage.clone(),
        );

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
