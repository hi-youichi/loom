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
//! - [`agent`]: [`agent::react`] — ReAct nodes ([`ThinkNode`], [`ActNode`], [`ObserveNode`]), [`run_react_graph`],
//!   [`tools_condition`], [`ReactRunner`], [`ReactBuildConfig`], [`build_react_runner`], [`build_react_run_context`].
//! - [`state`]: [`ReActState`], [`ToolCall`], [`ToolResult`] — state and tool types for ReAct.
//! - [`llm`]: [`LlmClient`] trait, [`MockLlm`], [`ChatOpenAI`].
//! - [`memory`]: Checkpointing ([`Checkpointer`], [`MemorySaver`], [`SqliteSaver`]), [`Store`]; optional LanceDB.
//! - [`tool_source`]: [`ToolSource`], [`ToolSpec`]; MCP ([`McpToolSource`]); [`WebToolsSource`], [`BashToolsSource`].
//! - [`traits`]: Core [`Agent`] trait — implement for custom agents.
//! - [`message`]: [`Message`] (System / User / Assistant).
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
//!             messages.push(Message::Assistant(s.clone()));
//!         }
//!         Ok(MyState { messages })
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut state = MyState::default();
//! state.messages.push(Message::User("hello, world!".to_string()));
//!
//! let agent = EchoAgent;
//! match agent.run(state).await {
//!     Ok(s) => {
//!         if let Some(Message::Assistant(content)) = s.messages.last() {
//!             println!("{}", content);
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

pub mod cache;
pub mod channels;
pub mod cli_run;
pub mod protocol;
pub mod compress;
pub mod model_spec;
pub mod config;
pub mod error;
pub mod export;
pub mod graph;
pub mod helve;
pub mod runner_common;
pub mod llm;
pub mod managed;
pub mod memory;
pub mod message;
pub mod openai_sse;
pub mod prompts;
pub mod agent;
pub mod state;
pub mod stream;
pub mod tool_source;
pub mod tools;
pub mod traits;

pub use cache::{Cache, CacheError, InMemoryCache};
pub use channels::{
    BinaryOperatorAggregate, Channel, ChannelError, EphemeralValue, FieldBasedUpdater, LastValue,
    NamedBarrierValue, StateUpdater, Topic,
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
    assemble_system_prompt, assemble_system_prompt_with_prompts, to_react_build_config,
    tools_requiring_approval, ApprovalPolicy, HelveConfig, APPROVAL_REQUIRED_EVENT_TYPE,
};
pub use llm::ChatOpenAI;
pub use llm::{LlmClient, LlmResponse, LlmUsage, MockLlm, ToolChoiceMode};
pub use model_spec::{
    CachedResolver, CompositeResolver, ConfigOverride, LocalFileResolver, ModelLimitResolver,
    ModelSpec, ModelsDevResolver, ResolverRefresher,
};
pub use managed::{IsLastStep, ManagedValue};
pub use memory::Embedder;
#[cfg(feature = "lance")]
pub use memory::LanceStore;
pub use memory::OpenAIEmbedder;
pub use memory::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointMetadata, CheckpointSource,
    Checkpointer, InMemoryStore, JsonSerializer, MemorySaver, Namespace, RunnableConfig, Store,
    StoreError, StoreSearchHit,
};
pub use memory::{SqliteSaver, SqliteStore};
pub use message::Message;
pub use openai_sse::{
    parse_chat_request, write_sse_line, ChatCompletionChunk, ChatCompletionRequest, ChatMessage,
    ChunkMeta, ChunkUsage, DeltaToolCall, MessageContent, ParseError, ParsedChatRequest,
    StreamOptions, StreamToSse,
};
pub use agent::react::{
    build_dup_runner, build_got_runner, build_react_initial_state, build_react_run_context,
    build_react_runner, build_react_runner_with_openai, build_tot_runner, run_react_graph,
    run_react_graph_stream, tools_condition, ActNode, BuildRunnerError, ErrorHandlerFn,
    GotRunnerConfig, HandleToolErrors, ObserveNode, ReactBuildConfig, ReactRunContext,
    ReactRunner, RunError as ReactRunError, TotRunnerConfig,
    STEP_PROGRESS_EVENT_TYPE, ThinkNode, ToolsConditionResult, WithNodeLogging,
    DEFAULT_EXECUTION_ERROR_TEMPLATE, DEFAULT_TOOL_ERROR_TEMPLATE, REACT_SYSTEM_PROMPT,
};
pub use cli_run::{
    build_helve_config, load_agents_md, load_soul_md, run_agent, AnyRunner, AnyStreamEvent,
    RunCmd, RunError, RunOptions, DEFAULT_WORKING_FOLDER,
};
pub use protocol::stream::{stream_event_to_protocol_format, Envelope};
pub use protocol::{
    AgentType, ClientRequest, ErrorResponse, PingRequest, PongResponse, RunEndResponse,
    RunRequest, RunStreamEventResponse, ServerResponse, ToolShowOutput, ToolShowRequest,
    ToolShowResponse, ToolsListRequest, ToolsListResponse,
};
pub use prompts::{
    default_from_embedded as default_agent_prompts_from_yaml, load as load_agent_prompts,
    load_or_default as load_agent_prompts_or_default, AgentPrompts, LoadError as PromptsLoadError,
};
pub use state::{ReActState, ToolCall, ToolResult};
pub use stream::{
    CheckpointEvent, MessageChunk, StreamEvent, StreamMetadata, StreamMode, StreamWriter,
    ToolStreamWriter,
};
pub use tool_source::McpToolSource;
pub use tool_source::{
    BashToolsSource, MemoryToolsSource, MockToolSource, ShortTermMemoryToolSource, StoreToolSource,
    ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec, WebToolsSource,
    TOOL_BASH, TOOL_GET_RECENT_MESSAGES, TOOL_LIST_MEMORIES, TOOL_RECALL, TOOL_REMEMBER,
    TOOL_SEARCH_MEMORIES, TOOL_WEB_FETCHER,
};
pub use tools::{register_mcp_tools, BashTool, McpToolAdapter};
pub use traits::Agent;

// Re-export DUP, GoT, ToT from agent for backward compatibility.
pub use agent::{
    build_dup_initial_state, build_got_initial_state, build_tot_initial_state, DupRunError,
    DupRunner, DupState, GotRunError, GotRunner, GotState, TaskGraph, TaskNode, TaskNodeState,
    TaskStatus, TotCandidate, TotExtension, TotRunError, TotRunner, TotState, UnderstandOutput,
};

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
