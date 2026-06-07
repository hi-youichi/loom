//! Think node: read messages, call LLM, write assistant message and optional tool_calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, trace};

use loom_llm::error::AgentError;
use loom_graph::{run_cancellable, Next, RunContext};
use loom_llm::{LlmClient, LlmProvider, LlmResponse, ToolCallDelta};
use loom_llm::message::Message;
use loom_model_spec::ModelTier;
use loom_types::state::{ModelConfig, ReActState, ToolCall};
use loom_stream::{ChunkToStreamSender, MessageChunk, StreamEvent, StreamMetadata, StreamMode};
use loom_graph::Node;

pub struct ThinkNode {
    provider: Arc<dyn LlmProvider>,
    client_cache: RwLock<HashMap<String, Arc<dyn LlmClient>>>,
}

impl ThinkNode {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            client_cache: RwLock::new(HashMap::new()),
        }
    }

    async fn resolve_client(&self, model_config: &ModelConfig) -> Result<Arc<dyn LlmClient>, AgentError> {
        let model = if !model_config.model_id.is_empty() {
            model_config.model_id.clone()
        } else if model_config.tier != ModelTier::None {
            let providers = loom_tier::provider::load_provider_configs().ok_or_else(|| {
                AgentError::ExecutionFailed("no provider configs for tier resolution".into())
            })?;
            let entry = loom_tier::resolve::resolve_tier_intelligent(
                self.provider.provider_name(),
                model_config.tier,
                &providers,
            )
            .await
            .ok_or_else(|| {
                AgentError::ExecutionFailed(format!(
                    "no model found for tier {:?} on provider '{}'",
                    model_config.tier,
                    self.provider.provider_name()
                ))
            })?;
            entry.id
        } else {
            self.provider.default_model().to_string()
        };

        {
            let cache = self.client_cache.read().await;
            if let Some(client) = cache.get(&model) {
                return Ok(Arc::clone(client));
            }
        }

        let client = self.provider.create_client(&model)?;
        let client = Arc::from(client);
        {
            let mut cache = self.client_cache.write().await;
            cache.entry(model).or_insert_with(|| Arc::clone(&client));
        }
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
    ) -> Result<(), AgentError> {
        let Some(stream_tx) = ctx.stream_tx.as_ref() else {
            return Ok(());
        };

        if should_stream && !content.is_empty() && streamed_chunks == 0 {
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
        usage: &loom_llm::LlmUsage,
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

/// Max retries when MiniMax returns 2013 (tool session lost).
const MAX_SESSION_LOST_RETRIES: u32 = 3;

fn is_minimax_session_lost(err: &AgentError) -> bool {
    match err {
        AgentError::ExecutionFailed(msg) => msg.to_lowercase().contains("(code: 2013)"),
        _ => false,
    }
}

fn find_last_assistant_with_tool_calls(messages: &[Message]) -> Option<usize> {
    messages.iter().rposition(|m| {
        matches!(m, Message::Assistant(p) if !p.tool_calls.is_empty())
    })
}

fn rollback_last_tool_round(state: &mut ReActState) -> bool {
    if let Some(pos) = find_last_assistant_with_tool_calls(&state.messages) {
        state.messages.truncate(pos);
        state.tool_calls.clear();
        state.tool_results.clear();
        state.message_count_after_last_think = Some(state.messages.len());
        true
    } else {
        false
    }
}

#[async_trait]
impl Node<ReActState> for ThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(&self, mut state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let llm = self.resolve_client(&state.model_config).await?;
        let mut session_retries = 0;
        let response = loop {
            match llm.invoke(&state.messages).await {
                Ok(resp) => break resp,
                Err(ref e)
                    if is_minimax_session_lost(e)
                        && session_retries < MAX_SESSION_LOST_RETRIES =>
                {
                    session_retries += 1;
                    if !rollback_last_tool_round(&mut state) {
                        return Err(e.clone());
                    }
                    tracing::warn!(
                        attempt = session_retries,
                        max_retries = MAX_SESSION_LOST_RETRIES,
                        "MiniMax session lost (2013), rolled back tool round, retrying Think"
                    );
                }
                Err(e) => return Err(e),
            }
        };
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
        mut state: ReActState,
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
            should_stream, should_stream_tools, "think: invoking LLM"
        );

        let call_start = Instant::now();
        let llm = self.resolve_client(&state.model_config).await?;

        let mut session_retries = 0;
        let (response, streamed_chunks, first_token_at) = loop {
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

            match run_cancellable(llm_call, ctx.cancellation.as_ref()).await {
                Ok(Ok(triple)) => break triple,
                Ok(Err(ref e))
                    if is_minimax_session_lost(e)
                        && session_retries < MAX_SESSION_LOST_RETRIES =>
                {
                    session_retries += 1;
                    if !rollback_last_tool_round(&mut state) {
                        return Err(e.clone());
                    }
                    tracing::warn!(
                        attempt = session_retries,
                        max_retries = MAX_SESSION_LOST_RETRIES,
                        "MiniMax session lost (2013), rolled back tool round, retrying Think"
                    );
                }
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(e),
            }
        };

        if is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        let loom_llm::LlmResponse {
            content: resp_content,
            reasoning_content,
            tool_calls,
            usage,
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_llm::message::{AssistantPayload, AssistantToolCall, ToolCallContent};

    #[test]
    fn minimax_session_lost_detects_2013() {
        let err = AgentError::ExecutionFailed(
            "OpenAI-compat stream error 400 Bad Request: invalid params, tool result's tool id(call_xxx) not found (code: 2013) (type: bad_request_error)".to_string(),
        );
        assert!(is_minimax_session_lost(&err));
    }

    #[test]
    fn minimax_session_lost_ignores_other_execution_failed() {
        let err = AgentError::ExecutionFailed("rate limit exceeded (code: 429)".to_string());
        assert!(!is_minimax_session_lost(&err));
    }

    #[test]
    fn minimax_session_lost_ignores_cancelled() {
        assert!(!is_minimax_session_lost(&AgentError::Cancelled));
    }

    #[test]
    fn find_last_assistant_with_tool_calls_returns_correct_index() {
        let messages = vec![
            Message::user("hello"),
            Message::Assistant(AssistantPayload {
                content: String::new(),
                tool_calls: vec![AssistantToolCall {
                    id: "call_123".to_string(),
                    name: "read_file".to_string(),
                    arguments: "{}".to_string(),
                }],
                reasoning_content: None,
            }),
            Message::Tool {
                tool_call_id: "call_123".to_string(),
                content: ToolCallContent::Text("result".to_string()),
            },
        ];
        assert_eq!(find_last_assistant_with_tool_calls(&messages), Some(1));
    }

    #[test]
    fn find_last_assistant_with_tool_calls_finds_latest() {
        let messages = vec![
            Message::user("hello"),
            Message::Assistant(AssistantPayload {
                content: String::new(),
                tool_calls: vec![AssistantToolCall {
                    id: "call_1".to_string(),
                    name: "tool1".to_string(),
                    arguments: "{}".to_string(),
                }],
                reasoning_content: None,
            }),
            Message::Tool {
                tool_call_id: "call_1".to_string(),
                content: ToolCallContent::Text("result1".to_string()),
            },
            Message::Assistant(AssistantPayload {
                content: String::new(),
                tool_calls: vec![AssistantToolCall {
                    id: "call_2".to_string(),
                    name: "tool2".to_string(),
                    arguments: "{}".to_string(),
                }],
                reasoning_content: None,
            }),
            Message::Tool {
                tool_call_id: "call_2".to_string(),
                content: ToolCallContent::Text("result2".to_string()),
            },
        ];
        assert_eq!(find_last_assistant_with_tool_calls(&messages), Some(3));
    }

    #[test]
    fn find_last_assistant_without_tool_calls_returns_none() {
        let messages = vec![
            Message::user("hello"),
            Message::assistant("just text"),
        ];
        assert_eq!(find_last_assistant_with_tool_calls(&messages), None);
    }

    #[test]
    fn rollback_last_tool_round_truncates_messages() {
        let mut state = ReActState {
            messages: vec![
                Message::user("hello"),
                Message::Assistant(AssistantPayload {
                    content: String::new(),
                    tool_calls: vec![AssistantToolCall {
                        id: "call_abc".to_string(),
                        name: "read_file".to_string(),
                        arguments: "{}".to_string(),
                    }],
                    reasoning_content: None,
                }),
                Message::Tool {
                    tool_call_id: "call_abc".to_string(),
                    content: ToolCallContent::Text("file contents".to_string()),
                },
            ],
            message_count_after_last_think: Some(3),
            ..Default::default()
        };
        assert!(rollback_last_tool_round(&mut state));
        assert_eq!(state.messages.len(), 1);
        assert!(matches!(&state.messages[0], Message::User(_)));
        assert_eq!(state.message_count_after_last_think, Some(1));
        assert!(state.tool_calls.is_empty());
        assert!(state.tool_results.is_empty());
    }

    #[test]
    fn rollback_last_tool_round_returns_false_when_no_tool_round() {
        let mut state = ReActState {
            messages: vec![Message::user("hello")],
            ..Default::default()
        };
        assert!(!rollback_last_tool_round(&mut state));
        assert_eq!(state.messages.len(), 1);
    }
}
