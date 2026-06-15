use crate::agent::react::build::build_react_runner;
use crate::agent::react::{ReactBuildConfig, ReactRunner};
use crate::runner_common::StreamRunOutcome;
use loom_llm::MessageChunkKind;
use loom_stream::StreamEvent;
use loom_cli_types::ReActState;
use std::sync::Arc;

pub type AgentConfig = ReactBuildConfig;

#[derive(Debug, Clone)]
pub struct AgentResult {
    pub reply: String,
    pub reasoning: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("{0}")]
    Build(String),
    #[error("{0}")]
    Run(String),
    #[error("agent run cancelled")]
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextChunk(String),
    ReasoningChunk(String),
    ToolCallStart {
        name: String,
        arguments: String,
    },
    ToolOutput {
        name: String,
        content: String,
    },
    ToolEnd {
        name: String,
        result: String,
        is_error: bool,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
}

pub struct Agent {
    runner: ReactRunner,
}

impl Agent {
    pub async fn from_config(config: AgentConfig) -> Result<Self, AgentError> {
        let runner = build_react_runner(&config, None, false, None, None)
            .await
            .map_err(|e| AgentError::Build(e.to_string()))?;
        Ok(Self { runner })
    }

    pub async fn run<F>(&self, message: &str, on_event: F) -> Result<AgentResult, AgentError>
    where
        F: FnMut(AgentEvent) + Send + Sync + Clone + 'static,
    {
        let user_cb = Arc::new(on_event);
        let bridge = move |ev: StreamEvent<ReActState>| {
            if let Some(e) = map_stream_event(ev) {
                let mut cb = user_cb.as_ref().clone();
                cb(e);
            }
        };

        let outcome = self
            .runner
            .stream_with_config(message, None, Some(bridge))
            .await
            .map_err(|e| AgentError::Run(e.to_string()))?;

        match outcome {
            StreamRunOutcome::Finished(state) => Ok(AgentResult {
                reply: state.last_assistant_reply().unwrap_or_default(),
                reasoning: state.last_reasoning_content(),
            }),
            StreamRunOutcome::Cancelled => Err(AgentError::Cancelled),
        }
    }
}

fn map_stream_event(ev: StreamEvent<ReActState>) -> Option<AgentEvent> {
    match ev {
        StreamEvent::Messages { chunk, .. } => match chunk.kind {
            MessageChunkKind::Thinking => Some(AgentEvent::ReasoningChunk(chunk.content)),
            MessageChunkKind::Message => Some(AgentEvent::TextChunk(chunk.content)),
        },
        StreamEvent::ToolCall { name, arguments, .. } => Some(AgentEvent::ToolCallStart {
            name,
            arguments: arguments.to_string(),
        }),
        StreamEvent::ToolOutput { name, content, .. } => {
            Some(AgentEvent::ToolOutput { name, content })
        }
        StreamEvent::ToolEnd {
            name, result, is_error, ..
        } => Some(AgentEvent::ToolEnd {
            name,
            result,
            is_error,
        }),
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            ..
        } => Some(AgentEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_react_config::ReactBuildConfig;
    use crate::agent::react::build::build_react_runner;
    use loom_llm::client::{FixedLlmProvider, MockLlm};
    use loom_llm::MessageChunk;
    use loom_stream::StreamMetadata;
    use std::sync::Arc;

    fn base_config() -> ReactBuildConfig {
        let mut cfg = ReactBuildConfig::from_env();
        cfg.working_folder = Some(std::env::temp_dir());
        cfg
    }
    use loom_types::state::ReActState;

    fn stream_metadata() -> StreamMetadata {
        StreamMetadata {
            loom_node: String::new(),
            namespace: None,
        }
    }

    fn message_chunk(content: &str, kind: MessageChunkKind) -> MessageChunk {
        MessageChunk {
            content: content.to_string(),
            kind,
        }
    }

    #[test]
    fn map_messages_text_chunk() {
        let ev = StreamEvent::<ReActState>::Messages {
            chunk: message_chunk("hello", MessageChunkKind::Message),
            metadata: stream_metadata(),
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::TextChunk(s)) if s == "hello"
        ));
    }

    #[test]
    fn map_messages_reasoning_chunk() {
        let ev = StreamEvent::<ReActState>::Messages {
            chunk: message_chunk("thinking...", MessageChunkKind::Thinking),
            metadata: stream_metadata(),
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::ReasoningChunk(s)) if s == "thinking..."
        ));
    }

    #[test]
    fn map_tool_call() {
        let ev = StreamEvent::<ReActState>::ToolCall {
            call_id: Some("c1".into()),
            name: "read".into(),
            arguments: serde_json::json!({"path": "foo.rs"}),
        };
        let mapped = map_stream_event(ev).unwrap();
        match mapped {
            AgentEvent::ToolCallStart { name, arguments } => {
                assert_eq!(name, "read");
                assert!(arguments.contains("foo.rs"));
            }
            other => panic!("expected ToolCallStart, got {:?}", other),
        }
    }

    #[test]
    fn map_tool_output() {
        let ev = StreamEvent::<ReActState>::ToolOutput {
            call_id: None,
            name: "bash".into(),
            content: "running...".into(),
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::ToolOutput { name, content }) if name == "bash" && content == "running..."
        ));
    }

    #[test]
    fn map_tool_end_success() {
        let ev = StreamEvent::<ReActState>::ToolEnd {
            call_id: None,
            name: "read".into(),
            result: "file contents".into(),
            is_error: false,
            raw_result: None,
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::ToolEnd { name, is_error, .. })
                if name == "read" && !is_error
        ));
    }

    #[test]
    fn map_tool_end_error() {
        let ev = StreamEvent::<ReActState>::ToolEnd {
            call_id: None,
            name: "bash".into(),
            result: "command failed".into(),
            is_error: true,
            raw_result: None,
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::ToolEnd { is_error: true, .. })
        ));
    }

    #[test]
    fn map_usage() {
        let ev = StreamEvent::<ReActState>::Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prefill_duration: None,
            decode_duration: None,
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::Usage { prompt_tokens: 100, completion_tokens: 50, total_tokens: 150 })
        ));
    }

    #[test]
    fn map_values_discarded() {
        let ev = StreamEvent::<ReActState>::Values(ReActState::default());
        assert!(map_stream_event(ev).is_none());
    }

    #[test]
    fn map_task_start_discarded() {
        let ev = StreamEvent::<ReActState>::TaskStart {
            node_id: "think".into(),
            namespace: None,
        };
        assert!(map_stream_event(ev).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn agent_run_with_mock_llm_returns_reply() {
        let cfg = base_config();
        let runner = build_react_runner(
            &cfg,
            Some(Arc::new(FixedLlmProvider {
                client: Arc::new(MockLlm::with_no_tool_calls("hello from agent")),
                model_id: "mock".to_string(),
            })),
            false,
            None,
            None,
        )
        .await
        .unwrap();

        let agent = Agent { runner };
        let events: Arc<std::sync::Mutex<Vec<AgentEvent>>> = Arc::new(std::sync::Mutex::new(vec![]));
        let events_clone = events.clone();
        let result = agent
            .run("test message", move |ev| {
                events_clone.lock().unwrap().push(ev);
            })
            .await
            .unwrap();

        assert_eq!(result.reply, "hello from agent");
        let events = events.lock().unwrap();
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextChunk(_))));
    }


}
