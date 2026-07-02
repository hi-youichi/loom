use crate::agent::react::build::build_react_runner;
use crate::agent::react::{ReactBuildConfig, ReactRunner};
use crate::runner_common::StreamRunOutcome;
use checkpoint::RunnableConfig;
use loom_llm::MessageChunkKind;
use loom_stream::StreamEvent;
use crate::state::ReActState;
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
        /// Cached prompt tokens (OpenAI `prompt_tokens_details.cached_tokens`).
        /// `None` when the provider does not report cache hits, or when the
        /// current request had no cacheable prefix.
        cached_tokens: Option<u32>,
    },
}

pub struct Agent {
    runner: ReactRunner,
    config: AgentConfig,
}

impl Agent {
    pub async fn from_config(config: AgentConfig) -> Result<Self, AgentError> {
        let runner = build_react_runner(&config, None, false, None, None)
            .await
            .map_err(|e| AgentError::Build(e.to_string()))?;
        Ok(Self { runner, config })
    }

    /// Constructs an Agent from an existing runner. Primarily useful for
    /// testing with a custom (e.g. mock) LLM provider.
    pub fn from_runner(config: AgentConfig, runner: ReactRunner) -> Self {
        Self { config, runner }
    }

    /// Returns a clone of the configuration used to build this agent.
    /// Useful for forking the agent (e.g. for background review).
    pub fn config_snapshot(&self) -> AgentConfig {
        self.config.clone()
    }

    /// Returns the runnable config used by the underlying runner.
    /// This contains the `thread_id` and original `checkpoint_id`.
    pub fn runnable_config(&self) -> Option<RunnableConfig> {
        self.runner.runnable_config()
    }

    /// Returns the current thread id (typically the session id).
    pub fn current_thread_id(&self) -> Option<String> {
        self.runner.runnable_config()?.thread_id.clone()
    }

    pub async fn run<F>(&self, message: &str, on_event: F) -> Result<AgentResult, AgentError>
    where
        F: FnMut(AgentEvent) + Send + Sync + Clone + 'static,
    {
        self.run_with_config(message, None, on_event).await
    }

    pub async fn run_with_config<F>(
        &self,
        message: &str,
        config: Option<RunnableConfig>,
        on_event: F,
    ) -> Result<AgentResult, AgentError>
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
            .stream_with_config(message, config, Some(bridge))
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
            cached_tokens,
            ..
        } => Some(AgentEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cached_tokens,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReactBuildConfig;
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
    use crate::state::ReActState;

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
            cached_tokens: None,
            prefill_duration: None,
            decode_duration: None,
        };
        assert!(matches!(
            map_stream_event(ev),
            Some(AgentEvent::Usage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
                cached_tokens: None,
            })
        ));
    }

    #[test]
    fn map_usage_event_propagates_cached_tokens() {
        let ev = StreamEvent::<ReActState>::Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cached_tokens: Some(40),
            prefill_duration: None,
            decode_duration: None,
        };
        let mapped = map_stream_event(ev);
        if let Some(AgentEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cached_tokens,
        }) = mapped
        {
            assert_eq!(prompt_tokens, 100);
            assert_eq!(completion_tokens, 50);
            assert_eq!(total_tokens, 150);
            assert_eq!(cached_tokens, Some(40));
        } else {
            panic!("expected AgentEvent::Usage, got {mapped:?}");
        }
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

        let agent = Agent::from_runner(cfg.clone(), runner);
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

        let snapshot = agent.config_snapshot();
        assert_eq!(snapshot.working_folder, cfg.working_folder);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn agent_run_with_config_forwards_config_to_runner() {
        let cfg = base_config();
        let runner = build_react_runner(
            &cfg,
            Some(Arc::new(FixedLlmProvider {
                client: Arc::new(MockLlm::with_no_tool_calls("via config")),
                model_id: "mock".to_string(),
            })),
            false,
            None,
            None,
        )
        .await
        .unwrap();

        let agent = Agent::from_runner(cfg, runner);
        let fork_config = RunnableConfig {
            thread_id: Some("review-fork-test".to_string()),
            checkpoint_ns: "background-review".to_string(),
            ..Default::default()
        };

        let result = agent
            .run_with_config("test message", Some(fork_config), |_| {})
            .await
            .unwrap();

        assert_eq!(result.reply, "via config");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn agent_run_with_none_config_matches_run() {
        let cfg = base_config();
        let runner = build_react_runner(
            &cfg,
            Some(Arc::new(FixedLlmProvider {
                client: Arc::new(MockLlm::with_no_tool_calls("no config")),
                model_id: "mock".to_string(),
            })),
            false,
            None,
            None,
        )
        .await
        .unwrap();

        let agent = Agent::from_runner(cfg, runner);
        let result = agent
            .run_with_config("test message", None, |_| {})
            .await
            .unwrap();

        assert_eq!(result.reply, "no config");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn agent_exposes_config_snapshot_and_thread_id() {
        let cfg = base_config();
        let runner = build_react_runner(
            &cfg,
            Some(Arc::new(FixedLlmProvider {
                client: Arc::new(MockLlm::with_no_tool_calls("ok")),
                model_id: "mock".to_string(),
            })),
            false,
            None,
            None,
        )
        .await
        .unwrap();

        let agent = Agent::from_runner(cfg.clone(), runner);

        let snapshot = agent.config_snapshot();
        assert_eq!(snapshot.working_folder, cfg.working_folder);

        let runnable = agent.runnable_config();
        assert!(runnable.is_none() || runnable.unwrap().thread_id.is_none());

        assert!(agent.current_thread_id().is_none());
    }
}
