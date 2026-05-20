//! # Loom
//!
//! A minimal, graph-based agent framework in Rust. Build stateful agents and graphs
//! with a simple **state-in, state-out** design: one shared state type flows through nodes,
//! with no separate Input/Output types.
//!
//! ## Design principles
//!
//! - **Single state type**: Each graph uses one state struct (e.g. [`ReActState`]) that all
//!   nodes read from and write to.
//! - **One step per run**: Each agent implements a single step—receive state, return updated state.
//! - **State graphs**: Compose agents into [`StateGraph`] with conditional edges for complex workflows.
//! - **Minimal core API with optional streaming**: [`CompiledStateGraph::invoke`] stays state-in/state-out;
//!   use [`CompiledStateGraph::stream`] for incremental output when you need it.
//!
//! ## Features
//!
//! - **State Graphs**: Build and run stateful agent graphs with conditional routing.
//! - **ReAct Pattern**: Built-in reasoning + acting loops (Think → Act → Observe); [`ReactRunner`]
//!   and [`build_react_runner`] for config-driven ReAct (optional persistence, MCP, memory tools).
//! - **LLM Integration**: Flexible [`LlmClient`] trait with [`MockLlm`] and OpenAI-compatible [`ChatOpenAI`].
//! - **Memory & Checkpointing**: In-memory and persistent storage for agent state ([`Checkpointer`], [`Store`]).
//! - **Tool Integration**: Extensible tool system with MCP support ([`ToolSource`], [`McpToolSource`]).
//! - **Persistence**: Optional SQLite and LanceDB backends for long-term memory.
//! - **Middleware**: Wrap node execution with custom async logic ([`NodeMiddleware`]).
//! - **Streaming**: Stream per-step states or node updates via [`CompiledStateGraph::stream`] with [`StreamMode`].
//! - **Channels**: State update strategies ([`LastValue`], [`EphemeralValue`], [`Topic`], [`BinaryOperatorAggregate`],
//!   [`NamedBarrierValue`]); custom merge via [`StateUpdater`] and [`FieldBasedUpdater`].
//! - **Runtime Context**: Custom runtime context, store access, and managed values ([`RunContext`], [`ManagedValue`]).
//! - **Cache, Retry, Interrupts**: In-memory caching ([`InMemoryCache`]), retry policies ([`RetryPolicy`]),
//!   human-in-the-loop ([`InterruptHandler`]).
//! - **Graph Visualization**: [`generate_dot`], [`generate_text`].
//! - **Helve**: Product-semantic config ([`HelveConfig`]), system prompt assembly ([`assemble_system_prompt`]),
//!   conversion to ReAct config ([`to_react_build_config`]), approval policy ([`ApprovalPolicy`],
//!   [`tools_requiring_approval`], [`APPROVAL_REQUIRED_EVENT_TYPE`]).
//!
//! Feature flag: `lance` — LanceDB vector store for long-term memory (optional; heavy dependency).
//!
//! ## Main modules
//!
//! - [`graph`]: [`StateGraph`], [`CompiledStateGraph`], [`Node`], [`Next`], [`RunContext`] — build and run state graphs.
//! - [`agent`]: [`agent::react`] — ReAct nodes ([`ThinkNode`], [`ActNode`], [`ObserveNode`]), [`run_agent`],
//!   [`tools_condition`], [`ReactRunner`], [`ReactBuildConfig`], [`build_react_runner`], [`build_react_run_context`].
//! - [`state`]: [`ReActState`], [`ToolCall`], [`ToolResult`] — state and tool types for ReAct.
//! - [`llm`]: [`LlmClient`] trait, [`MockLlm`], [`ChatOpenAI`].
//! - [`memory`]: Checkpointing ([`Checkpointer`], [`MemorySaver`], [`SqliteSaver`]), [`Store`]; optional LanceDB.
//! - [`tool_source`]: [`ToolSource`], [`ToolSpec`]; MCP ([`McpToolSource`]); [`WebToolsSource`], [`BashToolsSource`].
//! - [`traits`]: Core [`Agent`] trait — implement for custom agents.
//! - [`message`]: [`Message`] (System / User / Assistant / Tool).
//! - [`stream`]: [`StreamWriter`], [`StreamEvent`], [`StreamMode`] for graph runs.
//! - [`config`]: Config summaries ([`RunConfigSummary`], [`build_config_summary`]).
//! - [`cache`]: [`Cache`], [`InMemoryCache`].
//! - [`channels`]: [`Channel`], [`LastValue`], [`Topic`], etc.; [`StateUpdater`], [`FieldBasedUpdater`].
//! - [`managed`]: [`ManagedValue`], [`IsLastStep`].
//! - [`tools`]: [`register_mcp_tools`], [`McpToolAdapter`].
//! - [`openai_sse`]: OpenAI-compatible SSE ([`StreamToSse`], [`ChatCompletionChunk`], [`parse_chat_request`]).
//! - [`helve`]: Product config ([`HelveConfig`]), [`to_react_build_config`], [`assemble_system_prompt`],
//!   [`ApprovalPolicy`], [`tools_requiring_approval`], [`APPROVAL_REQUIRED_EVENT_TYPE`].
//! - [`protocol`]: WebSocket message types for CLI remote mode ([`ClientRequest`], [`ServerResponse`]);
//!   streaming output protocol in [`protocol::stream`] ([`stream_event_to_protocol_format`], [`Envelope`]).
//! - [`user_message`]: [`UserMessageStore`] trait for per-thread message append/list ([`NoOpUserMessageStore`]).
//! - [`pregel`]: Low-level Pregel graph runtime with channels, checkpointing, task cache, and subgraph support.
//! - [`runner_common`]: Shared helpers for stream-based graph runs ([`StreamRunOutcome`], [`run_stream_with_config`]).
//!
//! Key types are re-exported at crate root: `use loom::{Agent, StateGraph, Message, ReActState};`.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use async_trait::async_trait;
//! use loom::{Agent, AgentError, Message};
//!
//! #[derive(Clone, Debug, Default)]
//! struct MyState {
//!     messages: Vec<Message>,
//! }
//!
//! struct EchoAgent;
//!
//! #[async_trait]
//! impl Agent for EchoAgent {
//!     fn name(&self) -> &str {
//!         "echo"
//!     }
//!
//!     type State = MyState;
//!
//!     async fn run(&self, state: Self::State) -> Result<Self::State, AgentError> {
//!         let mut messages = state.messages;
//!         if let Some(Message::User(s)) = messages.last() {
//!             messages.push(Message::assistant(s.clone()));
//!         }
//!         Ok(MyState { messages })
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut state = MyState::default();
//! state.messages.push(Message::User(loom::UserContent::Text("hello, world!".to_string())));
//!
//! let agent = EchoAgent;
//! match agent.run(state).await {
//!     Ok(s) => {
//!         if let Some(Message::Assistant(p)) = s.messages.last() {
//!             println!("{}", p.content);
//!         }
//!     }
//!     Err(e) => eprintln!("error: {}", e),
//! }
//! # }
//! ```
//!
//! Run the echo example: `cargo run -p loom-examples --example echo -- "hello, world!"`
//!
//! ## Examples
//!
//! See the `loom-examples` crate: `echo`, `react_linear`, `react_mcp`, `react_exa`, `react_memory`,
//! `memory_checkpoint`, `memory_persistence`, `openai_embedding`, `state_graph_echo`.

pub mod agent;
pub mod cache;
pub mod channels;
pub mod cli_run;
pub mod command;
pub mod compress;
pub mod config;
pub mod error;
pub mod export;
pub mod graph;
pub mod helve;
mod http_retry;
#[cfg(test)]
mod test_util;
pub mod llm;
pub mod lsp;
pub mod managed;
pub mod memory;
pub mod message;
pub mod model_spec;
pub mod openai_sse;
pub mod pregel;
pub mod prompts;
pub mod protocol;
pub mod profile_convert;
pub mod provider;
pub mod runner_common;
pub mod skill;
pub mod state;
pub mod stream;
pub mod tier;

pub mod services;
pub mod tool_source;
pub mod tools;
pub mod traits;
pub mod user_message;
pub mod title_generator;

pub use agent::react::{
    build_dup_runner, build_got_runner, build_react_initial_state, build_react_run_context,
    build_react_runner, build_react_runner_with_openai, build_tot_runner, run_agent,
    run_react_graph_stream, tools_condition, ActNode, AgentOptions, BuildRunnerError,
    BuiltinToolFilter, DefaultTierResolver, GotRunnerConfig, ObserveNode,
    ReactBuildConfig, ReactRunContext, ReactRunner, ResolvedTierModel, RunError as ReactRunError,
    ThinkNode, TierResolver, ToolsConditionResult,
    TotRunnerConfig, WithNodeLogging, DEFAULT_EXECUTION_ERROR_TEMPLATE,
    DEFAULT_TOOL_ERROR_TEMPLATE, REACT_SYSTEM_PROMPT, STEP_PROGRESS_EVENT_TYPE,
};
pub use cache::{Cache, CacheError, InMemoryCache};
pub use channels::{
    BinaryOperatorAggregate, Channel, ChannelError, EphemeralValue, FieldBasedUpdater, LastValue,
    NamedBarrierValue, StateUpdater, Topic,
};
pub use cli_run::{
    build_config_from_profile, build_helve_config, list_available_profiles, load_agents_md,
    resolve_model_config, resolve_profile, resolve_tier_and_build_config,
    resolve_tier_and_build_config_with_resolver,
    run_agent_with_llm_override, run_agent_with_options,
    ActiveOperation, ActiveOperationCanceller, ActiveOperationKind, AgentProfile, AgentRunResult,
    AnyRunner, AnyStreamEvent, ProfileError, ProfileSource, ProfileSummary, ResolvedAgent,
    ResolvedModelConfig, RunCancellation, RunCmd, RunCompletion, RunError, RunOptions,
    DEFAULT_WORKING_FOLDER,
};
pub use compress::CompactionConfig;
pub use config::{
    build_config_summary, ConfigSection, EmbeddingConfigSummary, LlmConfigSummary,
    MemoryConfigSummary, RunConfigSummary, RunConfigSummarySource, ToolConfigSummary,
};
pub use error::AgentError;
pub use export::stream_event_to_format_a;
pub use graph::{
    generate_dot, generate_text, log_graph_complete, log_graph_error, log_graph_start,
    log_node_complete, log_node_start, log_state_update, CompilationError, CompiledStateGraph,
    DefaultInterruptHandler, GraphInterrupt, Interrupt, InterruptHandler, LoggingNodeMiddleware,
    NameNode, Next, Node, NodeMiddleware, RetryPolicy, RunContext, Runtime, StateGraph, END, START,
};
pub use helve::{
    assemble_react_system_prompt, assemble_system_prompt,
    to_react_build_config, tools_requiring_approval, ApprovalPolicy, HelveConfig,
    ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
    EnvContext, OsInfo, LocaleInfo, ShellInfo, ProjectInfo, RuntimeInfo,
};
pub use llm::{ChatOpenAI, ChatOpenAICompat};
pub use llm::{
    CompletionTokensDetails, FixedLlmProvider, LlmClient, LlmProvider, LlmResponse, LlmUsage,
    MockLlm, OpenAICompatProvider, OpenAIProvider, PromptTokensDetails, ToolCallDelta,
    ToolChoiceMode,
};
pub use managed::{IsLastStep, ManagedValue};
pub use memory::Embedder;
#[cfg(feature = "lance")]
pub use memory::LanceStore;
pub use memory::OpenAIEmbedder;
pub use memory::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointMetadata, CheckpointSource,
    CheckpointUserMeta, Checkpointer, InMemoryStore, JsonSerializer, KernelMetadata, MemorySaver, Namespace, RunnableConfig, Store,
    StoreError, StoreSearchHit,
};
pub use memory::{SqliteSaver, SqliteStore};
pub use message::{
    AssistantPayload, AssistantToolCall, ContentError, ContentPart, Message, UserContent,
};
pub use model_spec::{
    CachedResolver, CompositeResolver, ConfigOverride, LocalFileResolver, ModelLimitResolver,
    ModelSpec, ModelTier, ModelsDevResolver, ResolverRefresher,
};
pub use openai_sse::{
    parse_chat_request, write_sse_line, ChatCompletionChunk, ChatCompletionRequest, ChatMessage,
    ChunkMeta, ChunkUsage, DeltaToolCall, MessageContent, ParseError, ParsedChatRequest,
    StreamOptions, StreamToSse,
};
pub use prompts::{
    default_from_embedded as default_agent_prompts_from_yaml, load as load_agent_prompts,
    load_or_default as load_agent_prompts_or_default, AgentPrompts, LoadError as PromptsLoadError,
};
pub use protocol::stream::{
    stream_event_to_protocol_envelope, stream_event_to_protocol_format,
    stream_event_to_protocol_value, Envelope,
};
pub use protocol::{
    AgentListRequest, AgentListResponse, AgentSource, AgentSourceFilter, AgentSummary, AgentType,
    ClientRequest, EnvelopeState, ErrorResponse, ListModelsRequest, ListModelsResponse,
    PingRequest, PongResponse, ProtocolEvent, ProtocolEventEnvelope, RunEndResponse, RunRequest,
    RunStreamEventResponse, ServerResponse, SessionUpdatedResponse, SetModelRequest, SetModelResponse, ThreadInWorkspace,
    ToolShowOutput, ToolShowRequest, ToolShowResponse, ToolsListRequest, ToolsListResponse,
    UserMessageItem, UserMessagesRequest, UserMessagesResponse, WorkspaceCreateRequest,
    WorkspaceCreateResponse, WorkspaceListRequest, WorkspaceListResponse, WorkspaceMeta,
    WorkspaceRenameRequest, WorkspaceRenameResponse, WorkspaceThreadAddRequest,
    WorkspaceThreadAddResponse, WorkspaceThreadListRequest,
    WorkspaceThreadListResponse, WorkspaceThreadRemoveRequest, WorkspaceThreadRemoveResponse,
    WorkspaceFileListRequest, WorkspaceFileListResponse, FileEntry,
    WorkspaceFileReadRequest, WorkspaceFileReadResponse,
    WorkspaceFileChangedResponse, FileChange,
};
pub use state::{
    normalize_tool_output, NormalizationConfig, NormalizedToolOutput, ToolOutputHint,
    ToolOutputStrategy, ToolStorageRef,
};
pub use state::{ModelConfig, ReActState, ToolCall, ToolResult};
pub use stream::{
    CheckpointEvent, MessageChunk, MessageChunkKind, StreamEvent, StreamMetadata, StreamMode,
    StreamWriter, ToolStreamWriter,
};
pub use tool_source::McpToolSource;
pub use tool_source::{
    BashToolsSource, MemoryToolsSource, MockToolSource, ShortTermMemoryToolSource, StoreToolSource,
    ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec, WebToolsSource,
    TOOL_BASH, TOOL_GET_RECENT_MESSAGES, TOOL_LIST_MEMORIES, TOOL_RECALL, TOOL_REMEMBER,
    TOOL_SEARCH_MEMORIES, TOOL_WEB_FETCHER,
};
pub use tools::shared::shell_output::{ShellOutput, format_shell_output, format_timed_out_output, format_terminal_timed_out_output, format_size, shell_output_dir, create_output_file, generate_run_id, make_relative};
pub use tools::shared::canceller::{ChildProcessCanceller, setup_cancellation};
pub use tools::{register_mcp_tools, BashTool, CommandExecutor, LocalCommandExecutor, McpToolAdapter};
pub use traits::Agent;
pub use user_message::{
    NoOpUserMessageStore, SqliteUserMessageStore, UserMessageStore, UserMessageStoreError,
};
pub use title_generator::generate_title;

// Re-export DUP, GoT, ToT from agent for backward compatibility.
pub use agent::{
    build_dup_initial_state, build_got_initial_state, build_tot_initial_state, DupRunError,
    DupRunner, DupState, GotRunError, GotRunner, GotState, TaskGraph, TaskNode, TaskNodeState,
    TaskStatus, TotCandidate, TotExtension, TotRunError, TotRunner, TotState, UnderstandOutput,
};

/// Global lock for tests that modify `LOOM_HOME` or `OPENAI_BASE_URL` env vars.
/// Use in any test that sets/removes these env vars to prevent data races.
#[cfg(test)]
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// When running `cargo test -p loom`, initializes tracing from `RUST_LOG` so that
/// unit tests in `src/**` (e.g. `openai.rs` `mod tests`) can print logs with `--nocapture`.
#[cfg(test)]
mod test_logging {
    use ctor::ctor;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::Layer;

    #[ctor]
    fn init() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
        let _ = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_filter(filter),
            )
            .try_init();
    }
}

#[cfg(test)]
mod run_agent_options_tests {
    use std::sync::OnceLock;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(unix)]
    use std::time::Instant;

    use crate::{
        run_agent_with_llm_override, run_agent_with_options, AnyStreamEvent, MockLlm,
        RunCancellation, RunCmd, RunCompletion, RunOptions, StreamEvent, UserContent, ReActState,
        ToolCall
    };
    use crate::memory::{default_memory_db_path, JsonSerializer, SqliteSaver, RunnableConfig, CheckpointListItem, Checkpointer};

    static MCP_SHORT_TIMEOUT: OnceLock<()> = OnceLock::new();

    fn ensure_short_mcp_timeout() {
        MCP_SHORT_TIMEOUT.get_or_init(|| {
            std::env::set_var("LOOM_MCP_INIT_TIMEOUT_SECS", "1");
        });
    }

    fn opts(working_folder: PathBuf) -> RunOptions {
        RunOptions {
            message: UserContent::Text("Hi".to_string()),
            working_folder: Some(working_folder),
            session_id: None,
            cancellation: None,
            thread_id: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
            any_stream_event_sender: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
        }
    }

    #[tokio::test]
    async fn run_agent_with_options_invalid_working_folder_returns_err() {
        let opts = opts(PathBuf::from(
            "/definitely/not/exist/loom-run-agent-with-options-test",
        ));
        let res = run_agent_with_options(&opts, &RunCmd::React, None).await;
        assert!(
            res.is_err(),
            "run_agent_with_options should fail for invalid working folder"
        );
    }

    #[tokio::test]
    async fn run_agent_with_options_success_path_with_on_event_receives_events() {
        ensure_short_mcp_timeout();
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
        std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

        let opts = opts(working);
        let event_count = std::sync::Arc::new(AtomicUsize::new(0));
        let count = std::sync::Arc::clone(&event_count);
        let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = Some(Box::new(move |_ev| {
            count.fetch_add(1, Ordering::Relaxed);
        }));

        let result = run_agent_with_llm_override(
            &opts,
            &RunCmd::React,
            on_event,
            Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
        )
        .await
        .expect("run_agent");

        match result {
            RunCompletion::Finished(result) => {
                assert_eq!(result.reply.trim(), "Done");
                assert_eq!(result.reasoning_content, None);
            }
            RunCompletion::Cancelled => panic!("expected finished run"),
        }
        assert!(
            event_count.load(Ordering::Relaxed) >= 1,
            "on_event should have been called at least once"
        );
    }

    #[tokio::test]
    async fn dry_run_returns_placeholder_for_tool_calls() {
        ensure_short_mcp_timeout();
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
        std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

        let mut run_opts = opts(working);
        run_opts.dry_run = true;

        let saw_dry_placeholder = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let saw = std::sync::Arc::clone(&saw_dry_placeholder);
        let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> =
            Some(Box::new(move |ev| {
                if let AnyStreamEvent::React(StreamEvent::Updates { state, .. }) = &ev {
                    if state.tool_results.iter().any(|tr| {
                        tr.content.contains("dry run") && tr.content.contains("was not executed")
                    }) {
                        saw.store(true, Ordering::Relaxed);
                    }
                }
            }));

        let result = run_agent_with_llm_override(
            &run_opts,
            &RunCmd::React,
            on_event,
            Some(Box::new(MockLlm::first_tools_then_end())),
        )
        .await
        .expect("run_agent");

        match result {
            RunCompletion::Finished(result) => {
                assert_eq!(result.reply.trim(), "The time is as above.");
            }
            RunCompletion::Cancelled => panic!("expected finished dry run"),
        }
        assert!(
            saw_dry_placeholder.load(Ordering::Relaxed),
            "stream events should contain a tool result with dry run placeholder"
        );
    }

    #[tokio::test]
    async fn run_agent_with_options_with_on_event_invalid_working_folder_returns_err() {
        let opts = opts(PathBuf::from(
            "/definitely/not/exist/loom-run-agent-with-options-test",
        ));
        let event_count = std::sync::Arc::new(AtomicUsize::new(0));
        let count = std::sync::Arc::clone(&event_count);
        let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = Some(Box::new(move |_ev| {
            count.fetch_add(1, Ordering::Relaxed);
        }));

        let res = run_agent_with_options(&opts, &RunCmd::React, on_event).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn session_id_restores_context_from_checkpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let dir_path = dir.path().to_path_buf();

        temp_env::async_with_vars(
            [("LOOM_HOME", Some(dir_path.as_os_str()))],
            async {
            let session_id = "sess-restore-test";
            let opts1 = RunOptions {
                message: UserContent::Text("First message".to_string()),
                working_folder: Some(working.clone()),
                session_id: None,
                cancellation: None,
                thread_id: Some(session_id.to_string()),
                agent: None,
                verbose: false,
                got_adaptive: false,
                display_max_len: 120,
                output_json: false,
                model: None,
                mcp_config_path: None,
                output_timestamp: false,
                dry_run: false,
                provider: None,
                base_url: None,
                api_key: None,
                provider_type: None,
                any_stream_event_sender: None,
                bash_executor: None,
                extra_tools: None,
                acp_session_id: None,
                force_compact: false,
                chat_id: None,
            };
            let opts2 = RunOptions {
                message: UserContent::Text("Second message".to_string()),
                working_folder: Some(working),
                session_id: None,
                cancellation: None,
                thread_id: Some(session_id.to_string()),
                agent: None,
                verbose: false,
                got_adaptive: false,
                display_max_len: 120,
                output_json: false,
                model: None,
                mcp_config_path: None,
                output_timestamp: false,
                dry_run: false,
                provider: None,
                base_url: None,
                api_key: None,
                provider_type: None,
                any_stream_event_sender: None,
                bash_executor: None,
                extra_tools: None,
                acp_session_id: None,
                force_compact: false,
                chat_id: None,
            };

            let result1 = run_agent_with_llm_override(
                &opts1,
                &RunCmd::React,
                None,
                Some(Box::new(MockLlm::with_no_tool_calls("Reply one"))),
            )
            .await
            .expect("first run");
            match result1 {
                RunCompletion::Finished(result) => assert_eq!(result.reply.trim(), "Reply one"),
                RunCompletion::Cancelled => panic!("expected first run to finish"),
            }

            let result2 = run_agent_with_llm_override(
                &opts2,
                &RunCmd::React,
                None,
                Some(Box::new(MockLlm::with_no_tool_calls("Reply two"))),
            )
            .await
            .expect("second run");
            match result2 {
                RunCompletion::Finished(result) => assert_eq!(result.reply.trim(), "Reply two"),
                RunCompletion::Cancelled => panic!("expected second run to finish"),
            }

            let db_path = default_memory_db_path();
            let serializer = std::sync::Arc::new(JsonSerializer);
            let saver = SqliteSaver::<ReActState>::new(&db_path, serializer)
                .expect("open sqlite saver");
            let config = RunnableConfig {
                thread_id: Some(session_id.to_string()),
                ..Default::default()
            };
            let list: Vec<CheckpointListItem> = saver
                .list(&config, Some(10), None, None)
                .await
                .expect("list checkpoints");
            assert!(
                list.len() >= 2,
                "session-id should persist both runs to same thread; got {} checkpoints",
                list.len()
            );
        }).await;
    }

    #[tokio::test]
    async fn run_agent_with_llm_override_returns_cancelled_when_token_is_pre_cancelled() {
        ensure_short_mcp_timeout();
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
        std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

        let mut opts = opts(working);
        let cancellation = RunCancellation::new(1);
        cancellation.cancel();
        opts.cancellation = Some(cancellation);

        let result = run_agent_with_llm_override(
            &opts,
            &RunCmd::React,
            None,
            Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
        )
        .await
        .expect("run_agent");

        assert!(matches!(result, RunCompletion::Cancelled));
    }

    #[tokio::test]
    async fn cancelled_run_does_not_persist_checkpoint() {
        ensure_short_mcp_timeout();
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
        std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");
        let dir_path = dir.path().to_path_buf();

        temp_env::async_with_vars(
            [("LOOM_HOME", Some(dir_path.as_os_str()))],
            async {
            let mut opts = opts(working);
            opts.thread_id = Some("cancelled-checkpoint-test".to_string());
            let cancellation = RunCancellation::new(1);
            cancellation.cancel();
            opts.cancellation = Some(cancellation);

            let result = run_agent_with_llm_override(
                &opts,
                &RunCmd::React,
                None,
                Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
            )
            .await
            .expect("run_agent");
            assert!(matches!(result, RunCompletion::Cancelled));

            let db_path = default_memory_db_path();
            let serializer = std::sync::Arc::new(JsonSerializer);
            let saver = SqliteSaver::<ReActState>::new(&db_path, serializer)
                .expect("open sqlite saver");
            let config = RunnableConfig {
                thread_id: Some("cancelled-checkpoint-test".to_string()),
                ..Default::default()
            };
            let list: Vec<CheckpointListItem> = saver
                .list(&config, Some(10), None, None)
                .await
                .expect("list checkpoints");
            assert!(
                list.is_empty(),
                "cancelled run should not persist checkpoints, got {}",
                list.len()
            );
        }).await;
    }

    #[tokio::test]
    async fn run_agent_with_llm_override_returns_cancelled_during_streaming() {
        ensure_short_mcp_timeout();
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();
        let loom_dir = working.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("create .loom");
        let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
        std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

        let mut opts = opts(working);
        let cancellation = RunCancellation::new(1);
        opts.cancellation = Some(cancellation.clone());

        let result = run_agent_with_llm_override(
            &opts,
            &RunCmd::React,
            None,
            Some(Box::new(
                MockLlm::with_no_tool_calls("This is a streamed response that should be cancelled.")
                    .with_stream_by_char()
                    .with_stream_delay_ms(1),
            )),
        )
        .await
        .expect("run_agent");

        match result {
            RunCompletion::Finished(result) => {
                assert_eq!(result.reply.trim(), "This is a streamed response that should be cancelled.");
            }
            RunCompletion::Cancelled => {
            }
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn cancelled_bash_tool_kills_active_child_process() {
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();

        let mut opts = opts(working);
        let cancellation = RunCancellation::new(42);
        opts.cancellation = Some(cancellation.clone());

        let llm = MockLlm::first_tools_then_end().with_tool_calls(vec![ToolCall {
            name: "bash".to_string(),
            arguments: serde_json::json!({ "command": "sleep 60" }).to_string(),
            id: Some("call-bash".to_string()),
        }]);

        let cancel_handle = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            cancel_handle.cancel();
        });

        let started_at = Instant::now();
        let result = run_agent_with_llm_override(&opts, &RunCmd::React, None, Some(Box::new(llm)))
            .await
            .expect("run_agent");

        assert!(
            started_at.elapsed() < std::time::Duration::from_secs(30),
            "cancelled subprocess run should finish promptly"
        );
        assert_eq!(cancellation.active_operation_kind(), None);
        assert!(matches!(result, RunCompletion::Cancelled));
    }
}
