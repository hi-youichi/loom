//! Unified agent runner: ReAct, DUP, ToT, GoT.

use crate::cli_run::build_helve_config;
use crate::export::stream_event_to_format_a;
use crate::llm::LlmClient;
use crate::protocol::stream::stream_event_to_protocol_envelope;
use crate::protocol::EnvelopeState;
use crate::protocol::ProtocolEventEnvelope;
use crate::{
    build_dup_runner, build_got_runner, build_react_runner, build_tot_runner, DupRunner, DupState,
    GotRunner, GotState, ReActState, ReactBuildConfig, ReactRunner, StreamEvent, TotRunner,
    TotState,
};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::{info_span, Instrument};

pub trait ActiveOperationCanceller: Send + Sync {
    fn cancel(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveOperationKind {
    Llm,
    ToolTask,
    McpRequest,
    ChildProcess,
}

#[derive(Clone)]
pub struct ActiveOperation {
    kind: ActiveOperationKind,
    canceller: Arc<dyn ActiveOperationCanceller>,
}

impl ActiveOperation {
    pub fn new(
        kind: ActiveOperationKind,
        canceller: Arc<dyn ActiveOperationCanceller>,
    ) -> Self {
        Self { kind, canceller }
    }

    pub fn kind(&self) -> ActiveOperationKind {
        self.kind
    }

    pub fn cancel(&self) {
        self.canceller.cancel();
    }
}

impl fmt::Debug for ActiveOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveOperation")
            .field("kind", &self.kind)
            .finish()
    }
}

#[derive(Debug)]
struct CancellationState {
    active_operation: Mutex<Option<ActiveOperation>>,
}

#[derive(Debug)]
struct AbortHandleCanceller {
    handle: futures_util::future::AbortHandle,
}

impl ActiveOperationCanceller for AbortHandleCanceller {
    fn cancel(&self) {
        self.handle.abort();
    }
}

/// Runtime cancellation handle for one run generation.
#[derive(Debug, Clone)]
pub struct RunCancellation {
    generation: u64,
    token: CancellationToken,
    state: Arc<CancellationState>,
}

impl RunCancellation {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            token: CancellationToken::new(),
            state: Arc::new(CancellationState {
                active_operation: Mutex::new(None),
            }),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn cancel(&self) {
        self.token.cancel();
        self.cancel_active_operation();
    }

    pub fn set_active_operation(&self, operation: ActiveOperation) {
        if let Ok(mut active_operation) = self.state.active_operation.lock() {
            *active_operation = Some(operation);
        }
    }

    pub fn set_abortable_operation(
        &self,
        kind: ActiveOperationKind,
        handle: futures_util::future::AbortHandle,
    ) {
        self.set_active_operation(ActiveOperation::new(
            kind,
            Arc::new(AbortHandleCanceller { handle }),
        ));
    }

    pub fn clear_active_operation(&self) {
        if let Ok(mut active_operation) = self.state.active_operation.lock() {
            *active_operation = None;
        }
    }

    pub fn active_operation_kind(&self) -> Option<ActiveOperationKind> {
        self.state
            .active_operation
            .lock()
            .ok()
            .and_then(|active_operation| active_operation.as_ref().map(ActiveOperation::kind))
    }

    pub fn cancel_active_operation(&self) {
        let active_operation = self
            .state
            .active_operation
            .lock()
            .ok()
            .and_then(|active_operation| active_operation.clone());
        if let Some(active_operation) = active_operation {
            active_operation.cancel();
            self.clear_active_operation();
        }
    }
}

/// Options for running the Helve agent.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub message: String,
    pub working_folder: Option<PathBuf>,
    pub session_id: Option<String>,
    /// Named agent profile (e.g. "coding"). Resolved from .loom/agents/<name> or ~/.loom/agents/<name>.
    pub agent: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    /// When true, stream events are collected and returned as JSON (CLI --json).
    pub output_json: bool,
    /// When set, overrides env/config for this run's LLM model (e.g. "gpt-4o", "gpt-4o-mini").
    pub model: Option<String>,
    /// Provider name to use for LLM (e.g. "openai", "anthropic"). Used to lookup base_url/api_key from config.
    pub provider: Option<String>,
    /// Direct base_url override (highest priority).
    pub base_url: Option<String>,
    /// Direct api_key override (highest priority).
    pub api_key: Option<String>,
    /// Provider type (e.g. "openai", "bigmodel").
    pub provider_type: Option<String>,
    /// When set, use this path as MCP config (overrides LOOM_MCP_CONFIG_PATH and default discovery).
    pub mcp_config_path: Option<PathBuf>,
    /// Optional cancellation handle for this run.
    pub cancellation: Option<RunCancellation>,
    /// Thread ID for checkpointer (conversation / run identity).
    pub thread_id: Option<String>,
    /// When true, print a timestamp line to stderr before each reply output (CLI --timestamp).
    pub output_timestamp: bool,
    /// When true, do not execute tools; LLM runs but tool calls return a placeholder (CLI --dry).
    pub dry_run: bool,
}

/// Error type for run operations.
#[derive(Debug, Error)]
pub enum RunError {
    #[error("build runner: {0}")]
    Build(#[from] crate::BuildRunnerError),
    #[error("run: {0}")]
    Run(#[from] crate::agent::react::RunError),
    #[error("dup run: {0}")]
    DupRun(#[from] crate::DupRunError),
    #[error("tot run: {0}")]
    TotRun(#[from] crate::TotRunError),
    #[error("got run: {0}")]
    GotRun(#[from] crate::GotRunError),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("remote: {0}")]
    Remote(String),
    #[error("config: {0}")]
    ConfigError(String),
}

/// Command mode for running an agent.
#[derive(Clone, Debug)]
pub enum RunCmd {
    React,
    Dup,
    Tot,
    Got { got_adaptive: bool },
}

/// Type-erased runner for any agent pattern.
pub enum AnyRunner {
    React(ReactRunner),
    Dup(DupRunner),
    Tot(TotRunner),
    Got(GotRunner),
}

/// Type-erased stream event for all agent types.
pub enum AnyStreamEvent {
    React(StreamEvent<ReActState>),
    Dup(StreamEvent<DupState>),
    Tot(StreamEvent<TotState>),
    Got(StreamEvent<GotState>),
}

/// Final result of a single agent run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRunResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
}

/// Final completion state of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCompletion {
    Finished(AgentRunResult),
    Cancelled,
}

impl AnyStreamEvent {
    /// Converts to format A JSON (EXPORT_SPEC §2).
    pub fn to_format_a(&self) -> Result<Value, serde_json::Error> {
        match self {
            AnyStreamEvent::React(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Dup(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Tot(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Got(ev) => stream_event_to_format_a(ev),
        }
    }

    /// Converts to protocol format with envelope as a typed event object.
    pub fn to_protocol_event(
        &self,
        state: &mut EnvelopeState,
    ) -> Result<ProtocolEventEnvelope, serde_json::Error> {
        match self {
            AnyStreamEvent::React(ev) => stream_event_to_protocol_envelope(ev, state),
            AnyStreamEvent::Dup(ev) => stream_event_to_protocol_envelope(ev, state),
            AnyStreamEvent::Tot(ev) => stream_event_to_protocol_envelope(ev, state),
            AnyStreamEvent::Got(ev) => stream_event_to_protocol_envelope(ev, state),
        }
    }

    /// Converts to protocol format (protocol_spec §4: type + payload) and injects envelope.
    /// Returns the JSON value to send in `RunStreamEventResponse.event`.
    pub fn to_protocol_format(
        &self,
        state: &mut EnvelopeState,
    ) -> Result<Value, serde_json::Error> {
        let event = self.to_protocol_event(state)?;
        event.to_value()
    }
}

/// Runs the agent. When `on_event` is Some, it is invoked for each stream event.
/// The server can pass a closure that converts to format A via `ev.to_format_a()` and sends over WebSocket.
/// The CLI can pass a closure that formats to stderr.
///
/// When `llm_override` is Some (e.g. in tests with [`crate::MockLlm`]), that client is used instead of
/// building one from config; otherwise the default LLM is built from env/OpenAI.
pub async fn run_agent(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<RunCompletion, RunError> {
    let (_helve, mut config, _resolved_agent) = build_helve_config(opts);
    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let kind = match cmd {
        RunCmd::React => "react",
        RunCmd::Dup => "dup",
        RunCmd::Tot => "tot",
        RunCmd::Got { .. } => "got",
    };
    let span = info_span!("run", kind = kind, thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if let RunCmd::Got { got_adaptive } = cmd {
        config.got_config.adaptive = *got_adaptive;
    }

    let runner = build_runner(&config, opts, cmd, llm_override)
        .instrument(span.clone())
        .await?;

    let on_event: Option<Arc<Mutex<Box<dyn FnMut(AnyStreamEvent) + Send>>>> =
        on_event.map(|b| Arc::new(Mutex::new(b)));

    let result = match &runner {
        AnyRunner::React(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<ReActState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::React(ev));
                    }
                }
            });
            let outcome = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            match outcome {
                crate::runner_common::StreamRunOutcome::Finished(state) => RunCompletion::Finished(
                    AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    },
                ),
                crate::runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
        AnyRunner::Dup(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<DupState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::Dup(ev));
                    }
                }
            });
            let outcome = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            match outcome {
                crate::runner_common::StreamRunOutcome::Finished(state) => RunCompletion::Finished(
                    AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    },
                ),
                crate::runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
        AnyRunner::Tot(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<TotState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::Tot(ev));
                    }
                }
            });
            let outcome = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            match outcome {
                crate::runner_common::StreamRunOutcome::Finished(state) => RunCompletion::Finished(
                    AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    },
                ),
                crate::runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
        AnyRunner::Got(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<GotState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::Got(ev));
                    }
                }
            });
            let outcome = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            match outcome {
                crate::runner_common::StreamRunOutcome::Finished(state) => RunCompletion::Finished(
                    AgentRunResult {
                        reply: state.summary_result(),
                        reasoning_content: None,
                    },
                ),
                crate::runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
    };

    Ok(result)
}

/// Convenience wrapper that runs the agent with no LLM override (default LLM from config).
/// Used by CLI, serve, and ACP. For tests with a mock LLM, use [`run_agent_with_llm_override`].
pub async fn run_agent_with_options(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
) -> Result<RunCompletion, RunError> {
    run_agent(opts, cmd, on_event, None).await
}

/// Runs the agent with an optional LLM override (e.g. [`crate::MockLlm`] in tests).
/// Same as [`run_agent`] but exposed under a distinct name to avoid clash with [`crate::agent::react::run_agent`].
pub async fn run_agent_with_llm_override(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<RunCompletion, RunError> {
    run_agent(opts, cmd, on_event, llm_override).await
}

/// Builds the runner for the given command.
/// When `llm_override` is Some, it is used instead of building the default LLM from config.
pub async fn build_runner(
    config: &ReactBuildConfig,
    opts: &RunOptions,
    cmd: &RunCmd,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<AnyRunner, RunError> {
    let cancellation = opts.cancellation.as_ref().map(RunCancellation::token);
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(config, llm_override, opts.verbose, None)
                .await?
                .with_cancellation(opts.cancellation.clone());
            Ok(AnyRunner::React(r))
        }
        RunCmd::Dup => {
            let r = build_dup_runner(config, llm_override, opts.verbose)
                .await?
                .with_cancellation(cancellation.clone());
            Ok(AnyRunner::Dup(r))
        }
        RunCmd::Tot => {
            let r = build_tot_runner(config, llm_override, opts.verbose)
                .await?
                .with_cancellation(cancellation.clone());
            Ok(AnyRunner::Tot(r))
        }
        RunCmd::Got { .. } => {
            let r = build_got_runner(config, llm_override, opts.verbose)
                .await?
                .with_cancellation(cancellation);
            Ok(AnyRunner::Got(r))
        }
    }
}

/// Simplified agent runner that only requires provider configuration and model name.
///
/// This is a convenience function for cases where you have a pre-configured provider
/// and model and just want to run the agent without setting up full RunOptions.
///
/// # Example
///
/// ```ignore
/// use loom::cli_run::{run_agent_with_provider, ProviderConfig};
///
/// let provider = ProviderConfig {
///     name: "openai".to_string(),
///     base_url: Some("https://api.openai.com/v1".to_string()),
///     api_key: Some("sk-...".to_string()),
///     provider_type: None,
/// };
///
/// let result = run_agent_with_provider(
///     provider,
///     "gpt-4o",
///     "What is the capital of France?",
///     None,
/// ).await?;
/// ```
pub async fn run_agent_with_provider(
    provider: crate::llm::ProviderConfig,
    model: &str,
    message: &str,
    working_folder: Option<PathBuf>,
) -> Result<RunCompletion, RunError> {
    // Create ModelEntry from provider config
    let entry = crate::llm::ModelEntry {
        id: format!("{}/{}", provider.name, model),
        name: model.to_string(),
        provider: provider.name.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        provider_type: provider.provider_type.clone(),
        ..Default::default()
    };

    // Create LLM client from ModelEntry
    let llm = crate::llm::create_llm_client(&entry)
        .map_err(|e| RunError::Build(crate::BuildRunnerError::Context(e)))?;

    // Create minimal RunOptions
    let opts = RunOptions {
        message: message.to_string(),
        working_folder,
        session_id: None,
        cancellation: None,
        thread_id: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        output_timestamp: false,
        model: Some(model.to_string()),
        mcp_config_path: None,
        dry_run: false,
        provider: Some(provider.name),
        base_url: provider.base_url,
        api_key: provider.api_key,
        provider_type: provider.provider_type,
    };

    // Run with LLM override
    run_agent(&opts, &RunCmd::React, None, Some(llm)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, TaskGraph, TaskNode, TaskNodeState, TaskStatus, TotExtension};

    fn opts_for_error(cmd: &RunCmd) -> RunOptions {
        let got_adaptive = matches!(cmd, RunCmd::Got { got_adaptive: true });
        RunOptions {
            message: "hello".to_string(),
            working_folder: Some(PathBuf::from(
                "/definitely/not/exist/loom-cli-run-agent-tests",
            )),
            session_id: None,
            cancellation: None,
            thread_id: None,
            agent: None,
            verbose: false,
            got_adaptive,
            display_max_len: 120,
            output_json: true,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
        }
    }

    fn minimal_config_with_invalid_working_folder() -> ReactBuildConfig {
        ReactBuildConfig {
            db_path: None,
            thread_id: None,
            user_id: None,
            system_prompt: None,
            exa_api_key: None,
            exa_codesearch_enabled: false,
            twitter_api_key: None,
            mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
            mcp_remote_cmd: "npx".to_string(),
            mcp_remote_args: "-y mcp-remote".to_string(),
            github_token: None,
            mcp_github_cmd: "npx".to_string(),
            mcp_github_args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-github".to_string(),
            ],
            mcp_github_url: None,
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            model: None,
            llm_provider: None,
            openai_tool_choice: None,
            openai_temperature: None,
            embedding_api_key: None,
            embedding_base_url: None,
            embedding_model: None,
            working_folder: Some(PathBuf::from(
                "/definitely/not/exist/loom-cli-run-agent-tests",
            )),
            approval_policy: None,
            compaction_config: None,
            tot_config: crate::TotRunnerConfig::default(),
            got_config: crate::GotRunnerConfig::default(),
            mcp_servers: None,
            skill_registry: None,
            max_sub_agent_depth: None,
            dry_run: false,
        }
    }

    #[test]
    fn any_stream_event_conversion_covers_all_variants() {
        let react = AnyStreamEvent::React(StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        });
        let dup = AnyStreamEvent::Dup(StreamEvent::TaskStart {
            node_id: "plan".to_string(),
            namespace: None,
        });
        let tot = AnyStreamEvent::Tot(StreamEvent::TaskStart {
            node_id: "think_expand".to_string(),
            namespace: None,
        });
        let got = AnyStreamEvent::Got(StreamEvent::TaskStart {
            node_id: "plan_graph".to_string(),
            namespace: None,
        });

        let mut env = EnvelopeState::new("sess-1".to_string());
        for ev in [react, dup, tot, got] {
            let a = ev.to_format_a().unwrap();
            assert!(a.is_object());
            let p = ev.to_protocol_format(&mut env).unwrap();
            assert_eq!(p["type"], "node_enter");
        }
    }

    #[tokio::test]
    async fn build_runner_errors_for_invalid_working_folder_for_all_modes() {
        let cfg = minimal_config_with_invalid_working_folder();
        let opts = RunOptions {
            message: "m".to_string(),
            working_folder: cfg.working_folder.clone(),
            session_id: None,
            cancellation: None,
            thread_id: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 200,
            output_json: true,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
        };
        assert!(build_runner(&cfg, &opts, &RunCmd::React, None)
            .await
            .is_err());
        assert!(build_runner(&cfg, &opts, &RunCmd::Dup, None).await.is_err());
        assert!(build_runner(&cfg, &opts, &RunCmd::Tot, None).await.is_err());
        assert!(
            build_runner(&cfg, &opts, &RunCmd::Got { got_adaptive: true }, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn run_agent_errors_for_invalid_working_folder_in_each_mode() {
        for cmd in [
            RunCmd::React,
            RunCmd::Dup,
            RunCmd::Tot,
            RunCmd::Got { got_adaptive: true },
        ] {
            let res = run_agent(&opts_for_error(&cmd), &cmd, None, None).await;
            assert!(res.is_err());
        }
    }

    #[test]
    fn run_error_display_variants_are_human_readable() {
        let e = RunError::Remote("boom".to_string());
        assert!(e.to_string().contains("remote"));
        let e2 = RunError::ToolNotFound("x".to_string());
        assert!(e2.to_string().contains("tool not found"));
    }

    #[test]
    fn got_state_summary_result_path_is_usable() {
        let s = GotState {
            input_message: "q".to_string(),
            task_graph: TaskGraph {
                nodes: vec![TaskNode {
                    id: "n1".to_string(),
                    description: "d".to_string(),
                    tool_calls: vec![],
                }],
                edges: vec![],
            },
            node_states: [(
                "n1".to_string(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("ok".to_string()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(s.summary_result(), "ok");

        let _tot = TotState {
            core: ReActState {
                messages: vec![Message::user("u"), Message::assistant("a")],
                ..ReActState::default()
            },
            tot: TotExtension::default(),
        };
    }
}
