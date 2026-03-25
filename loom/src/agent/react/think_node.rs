//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures_util::future::{abortable, Aborted};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::cli_run::ActiveOperationKind;
use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::context_persistence;
use crate::llm::{LlmClient, ToolCallDelta};
use crate::memory::uuid6;
use crate::message::{AssistantToolCall, Message};
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
    // `total_usage` sums only the three headline counts across turns. Per-turn breakdown
    // (`*_details`) is not summed (OpenAI usage is per request); keep details on `usage`.
    match (&state.total_usage, response_usage) {
        (Some(t), Some(u)) => (
            response_usage.clone(),
            Some(crate::llm::LlmUsage {
                prompt_tokens: t.prompt_tokens + u.prompt_tokens,
                completion_tokens: t.completion_tokens + u.completion_tokens,
                total_tokens: t.total_tokens + u.total_tokens,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        ),
        (None, Some(u)) => (response_usage.clone(), Some(u.clone())),
        (Some(t), None) => (None, Some(t.clone())),
        (None, None) => (None, None),
    }
}

fn normalize_tool_call_ids(mut calls: Vec<ToolCall>) -> Vec<ToolCall> {
    for tc in &mut calls {
        if tc.id.as_deref().map_or(true, |s| s.is_empty()) {
            tc.id = Some(format!("call_{}", uuid6()));
        }
    }
    calls
}

fn apply_think_response(
    state: ReActState,
    content: String,
    reasoning_content: Option<String>,
    tool_calls: Vec<ToolCall>,
    response_usage: Option<crate::llm::LlmUsage>,
) -> ReActState {
    let (usage, total_usage) = compute_usage(&state, &response_usage);
    let tool_calls = normalize_tool_call_ids(tool_calls);
    let assistant_tool_calls: Vec<AssistantToolCall> = tool_calls
        .iter()
        .map(|tc| AssistantToolCall {
            id: tc.id.clone().unwrap_or_default(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        })
        .collect();
    let mut messages = state.messages;
    let think_message = if assistant_tool_calls.is_empty() {
        Message::assistant(content)
    } else {
        Message::assistant_with_tool_calls(content, assistant_tool_calls)
    };
    messages.push(think_message);
    let message_count_after_last_think = Some(messages.len());
    ReActState {
        messages,
        last_reasoning_content: reasoning_content,
        tool_calls,
        tool_results: state.tool_results,
        turn_count: state.turn_count,
        approval_result: state.approval_result,
        usage,
        total_usage,
        message_count_after_last_think,
        think_count: state.think_count + 1,
        summary: state.summary,
        should_continue: state.should_continue,
    }
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        context_persistence::save_llm_request(
            "think",
            None,
            state.turn_count,
            &state.messages,
            None,
        );
        let response = self.llm.invoke(&state.messages).await?;

        // Save LLM response (no session_id available in run())
        context_persistence::save_llm_response(
            "think",
            None,
            state.turn_count,
            &response.content,
            response.reasoning_content.as_deref(),
            &response.tool_calls,
            response.usage.as_ref(),
            response.raw_request.as_deref(),
            response.raw_response.as_deref(),
        );

        let new_state = apply_think_response(
            state,
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

        let session_id = ctx.config.thread_id.as_deref();
        crate::llm::context_persistence::save_llm_request(
            "think",
            session_id,
            state.turn_count,
            &state.messages,
            None,
        );

        let call_start = Instant::now();
        let llm_call = async {
            if should_stream || should_stream_tools {
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
                        (0, None)
                    }
                };

                let (result, (forwarded_chunks, first_token_at), _) = tokio::join!(
                    self.llm
                        .invoke_stream_with_tool_delta(&state.messages, chunk_tx, tool_delta_tx,),
                    msg_forward,
                    tool_forward,
                );
                Ok::<_, AgentError>((result?, forwarded_chunks, first_token_at))
            } else {
                Ok::<_, AgentError>((self.llm.invoke(&state.messages).await?, 0, None))
            }
        };
        let (llm_call, abort_handle) = abortable(llm_call);
        if let Some(run_cancellation) = ctx.run_cancellation.as_ref() {
            run_cancellation.set_abortable_operation(ActiveOperationKind::Llm, abort_handle);
        }
        let (response, streamed_chunks, first_token_at) = if let Some(token) = ctx.cancellation.as_ref() {
            tokio::select! {
                _ = token.cancelled() => return Err(AgentError::Cancelled),
                result = llm_call => match result {
                    Ok(result) => result?,
                    Err(Aborted) => return Err(AgentError::Cancelled),
                },
            }
        } else {
            match llm_call.await {
                Ok(result) => result?,
                Err(Aborted) => return Err(AgentError::Cancelled),
            }
        };
        if let Some(run_cancellation) = ctx.run_cancellation.as_ref() {
            run_cancellation.clear_active_operation();
        }

        if is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        let used_fallback = response.content.is_empty() && response.tool_calls.is_empty();

        // Save LLM response with session_id from context
        crate::llm::context_persistence::save_llm_response(
            "think",
            session_id,
            state.turn_count,
            &response.content,
            response.reasoning_content.as_deref(),
            &response.tool_calls,
            response.usage.as_ref(),
            response.raw_request.as_deref(),
            response.raw_response.as_deref(),
        );

        let content = if used_fallback {
            "No text response from the model. Please try again or check the API.".to_string()
        } else {
            response.content
        };

        if used_fallback && ctx.stream_tx.is_some() {
            let fallback_chunk = MessageChunk::message(content.clone());
            let _ = ctx
                .stream_tx
                .as_ref()
                .unwrap()
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
            let _ = ctx
                .stream_tx
                .as_ref()
                .unwrap()
                .send(StreamEvent::Messages {
                    chunk: MessageChunk::message(content.clone()),
                    metadata: StreamMetadata {
                        loom_node: self.id().to_string(),
                        namespace: None,
                    },
                })
                .await;
        }

        // Emit complete tool_call events before applying state
        if should_stream_tools && !response.tool_calls.is_empty() {
            let tx = ctx.stream_tx.as_ref().unwrap();
            for tc in &response.tool_calls {
                if is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
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

        let new_state = apply_think_response(
            state,
            content,
            response.reasoning_content.clone(),
            response.tool_calls,
            response.usage.clone(),
        );

        if let (Some(ref tx), Some(ref u)) = (ctx.stream_tx.as_ref(), response.usage.as_ref()) {
            let (prefill_duration, decode_duration) = match first_token_at {
                Some(ft) => {
                    let prefill = ft.duration_since(call_start);
                    let decode = call_start.elapsed().saturating_sub(prefill);
                    (Some(prefill), Some(decode))
                }
                None => (None, None),
            };
            let _ = tx
                .send(StreamEvent::Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                    prefill_duration,
                    decode_duration,
                })
                .await;
        }

        Ok((new_state, Next::Continue))
    }
}
