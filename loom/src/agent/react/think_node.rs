//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::sync::Arc;

use async_trait::async_trait;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::{LlmClient, ToolCallDelta};
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
        let should_stream_tools = (ctx.stream_mode.contains(&StreamMode::Tools)
            || ctx.stream_mode.contains(&StreamMode::Debug))
            && ctx.stream_tx.is_some();

        let (response, streamed_chunks) = if should_stream || should_stream_tools {
            let stream_tx = ctx.stream_tx.clone().unwrap();

            let (chunk_tx, chunk_rx) = if should_stream {
                let adapter = ChunkToStreamSender::new(stream_tx.clone(), self.id());
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

            let tool_forward = async {
                if let Some(mut rx) = tool_delta_rx {
                    while let Some(delta) = rx.recv().await {
                        let _ = stream_tx
                            .send(StreamEvent::ToolCallChunk {
                                call_id: delta.call_id,
                                name: delta.name,
                                arguments_delta: delta.arguments_delta,
                            })
                            .await;
                    }
                }
            };

            let msg_forward = async {
                if let Some((adapter, rx)) = chunk_rx {
                    adapter.forward(rx).await
                } else {
                    0
                }
            };

            let (result, forwarded_chunks, _) = tokio::join!(
                self.llm
                    .invoke_stream_with_tool_delta(&state.messages, chunk_tx, tool_delta_tx,),
                msg_forward,
                tool_forward,
            );
            (result?, forwarded_chunks)
        } else {
            (self.llm.invoke(&state.messages).await?, 0)
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
                        loom_node: self.id().to_string(),
                    },
                })
                .await;
        }

        if should_stream && !used_fallback && !content.is_empty() && streamed_chunks == 0 {
            let _ = ctx
                .stream_tx
                .as_ref()
                .unwrap()
                .send(StreamEvent::Messages {
                    chunk: MessageChunk {
                        content: content.clone(),
                    },
                    metadata: StreamMetadata {
                        loom_node: self.id().to_string(),
                    },
                })
                .await;
        }

        // Emit complete tool_call events before applying state
        if should_stream_tools && !response.tool_calls.is_empty() {
            let tx = ctx.stream_tx.as_ref().unwrap();
            for tc in &response.tool_calls {
                let args: Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or_else(|_| Value::String(tc.arguments.clone()));
                let _ = tx
                    .send(StreamEvent::ToolCall {
                        call_id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: args,
                    })
                    .await;
            }
        }

        let new_state =
            apply_think_response(state, content, response.tool_calls, response.usage.clone());

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
