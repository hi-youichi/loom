//! Unified agent runner: ReAct, DUP, ToT, GoT.
//!
//! This module defines the full run orchestration types and functions.
//! It uses types from loom for config/display and agent runners from loom-agent.

use loom::cli_run::build_helve_config;
use loom::llm::LlmClient;
use crate::agent::react::build::{
    build_dup_runner, build_got_runner, build_react_runner, build_tot_runner,
    BuildRunnerError,
};
use crate::agent::dup::{DupRunner, DupState, DupRunError};
use crate::agent::got::{GotRunner, GotState, GotRunError};
use crate::agent::tot::{TotRunner, TotState, TotRunError};
use crate::agent::react::ReactRunner;
use loom::export::stream_event_to_format_a;
use loom::protocol::stream::stream_event_to_protocol_envelope;
use loom::protocol::EnvelopeState;
use loom::protocol::ProtocolEventEnvelope;
use loom::{ReactBuildConfig, StreamEvent, ReActState};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use thiserror::Error;

// Re-export cancellation types from loom
pub use loom::active_operation::{ActiveOperationCanceller, ActiveOperationKind, ActiveOperation, RunCancellation};

/// Options for running the Helve agent.
#[derive(Clone)]
pub struct RunOptions {
    pub message: loom::message::UserContent,
    pub working_folder: Option<PathBuf>,
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    pub output_json: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub mcp_config_path: Option<PathBuf>,
    pub cancellation: Option<RunCancellation>,
    pub thread_id: Option<String>,
    pub output_timestamp: bool,
    pub dry_run: bool,
    pub debug_llm: bool,
    pub any_stream_event_sender: Option<Arc<dyn Fn(crate::AnyStreamEvent) + Send + Sync>>,
    pub bash_executor: Option<Arc<dyn loom::tools::CommandExecutor>>,
    pub extra_tools: Option<Arc<Vec<Arc<dyn loom::tools::Tool>>>>,
    pub acp_session_id: Option<String>,
    pub force_compact: bool,
    pub chat_id: Option<i64>,
    pub worktree: bool,
}

impl std::fmt::Debug for RunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunOptions")
            .field("message", &self.message)
            .field("working_folder", &self.working_folder)
            .field("session_id", &self.session_id)
            .field("agent", &self.agent)
            .field("verbose", &self.verbose)
            .finish()
    }
}

impl RunOptions {
    /// Convert to loom::RunOptions for use with build_helve_config and other loom functions.
    /// Note: any_stream_event_sender is dropped since loom uses a different AnyStreamEvent type.
    pub fn to_loom(&self) -> loom::RunOptions {
        loom::RunOptions {
            message: self.message.clone(),
            working_folder: self.working_folder.clone(),
            session_id: self.session_id.clone(),
            agent: self.agent.clone(),
            verbose: self.verbose,
            got_adaptive: self.got_adaptive,
            display_max_len: self.display_max_len,
            output_json: self.output_json,
            model: self.model.clone(),
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            provider_type: self.provider_type.clone(),
            mcp_config_path: self.mcp_config_path.clone(),
            cancellation: self.cancellation.clone(),
            thread_id: self.thread_id.clone(),
            output_timestamp: self.output_timestamp,
            dry_run: self.dry_run,
            debug_llm: self.debug_llm,
            any_stream_event_sender: None, // Cannot convert between AnyStreamEvent types
            bash_executor: self.bash_executor.clone(),
            extra_tools: self.extra_tools.clone(),
            acp_session_id: self.acp_session_id.clone(),
            force_compact: self.force_compact,
            chat_id: self.chat_id,
            worktree: self.worktree,
        }
    }
}

/// Error type for run operations.
#[derive(Debug, Error)]
pub enum RunError {
    #[error("build runner: {0}")]
    Build(#[from] BuildRunnerError),
    #[error("run: {0}")]
    Run(#[from] crate::agent::react::RunError),
    #[error("dup run: {0}")]
    DupRun(#[from] DupRunError),
    #[error("tot run: {0}")]
    TotRun(#[from] TotRunError),
    #[error("got run: {0}")]
    GotRun(#[from] GotRunError),
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

/// Type-erased stream event for all agent types (with real state types).
#[derive(Debug)]
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
    pub fn to_format_a(&self) -> Result<Value, serde_json::Error> {
        match self {
            AnyStreamEvent::React(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Dup(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Tot(ev) => stream_event_to_format_a(ev),
            AnyStreamEvent::Got(ev) => stream_event_to_format_a(ev),
        }
    }

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

    pub fn to_protocol_format(
        &self,
        state: &mut EnvelopeState,
    ) -> Result<Value, serde_json::Error> {
        let event = self.to_protocol_event(state)?;
        event.to_value()
    }

    /// Convert from loom's stub AnyStreamEvent to local AnyStreamEvent.
    /// Only React events can be converted; DUP/TOT/GOT stubs are ignored.
    pub fn from_loom(ev: loom::cli_run::AnyStreamEvent) -> Self {
        match ev {
            loom::cli_run::AnyStreamEvent::React(e) => AnyStreamEvent::React(e),
            // For stub variants (Dup/Tot/Got), we can't convert; use a no-op React event
            _ => AnyStreamEvent::React(StreamEvent::Custom(serde_json::json!({"type": "noop"}))),
        }
    }
}

/// Convert loom-agent AnyStreamEvent to loom's stub AnyStreamEvent.
pub fn to_loom_any_stream_event(ev: &AnyStreamEvent) -> Option<loom::cli_run::AnyStreamEvent> {
    match ev {
        AnyStreamEvent::React(e) => Some(loom::cli_run::AnyStreamEvent::React(e.clone())),
        // DUP/TOT/GOT use real state types that differ from stubs; skip conversion
        _ => None,
    }
}

/// Runs the agent.
#[allow(clippy::type_complexity)]
pub async fn run_agent(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<RunCompletion, RunError> {
    let loom_opts = opts.to_loom();
    let (_helve, mut config, _resolved_agent) = build_helve_config(&loom_opts);
    if opts.debug_llm {
        eprintln!("========== [DEBUG-LLM] System Prompt ==========");
        if let Some(ref sp) = config.system_prompt {
            eprintln!("{}", sp);
        }
        eprintln!("========== [DEBUG-LLM] User Message ==========");
        eprintln!("{}", opts.message.as_text());
        eprintln!("================================================");
    }

    if opts.worktree {
        let current_dir = config.working_folder.as_deref().unwrap_or_else(|| std::path::Path::new("."));
        let wt_config = loom::worktree::WorktreeConfig::default();
        if let Ok(manager) = loom::worktree::WorktreeManager::from_working_dir(current_dir, wt_config) {
            if let Ok(handle) = manager.create_for_agent("top-level", None, None).await {
                config.working_folder = Some(handle.path.clone());
            }
        }
    }

    if let Some(ref executor) = opts.bash_executor { config.bash_executor = Some(executor.clone()); }
    if let Some(ref tools) = opts.extra_tools { config.extra_tools = Some(tools.clone()); }
    if let Some(ref sid) = opts.acp_session_id { config.acp_session_id = Some(sid.clone()); }

    if let RunCmd::Got { got_adaptive } = cmd { config.got_config.adaptive = *got_adaptive; }

    let runner = build_runner(&config, opts, cmd, llm_override).await?;

    let on_event: Option<Arc<Mutex<Box<dyn FnMut(AnyStreamEvent) + Send>>>> =
        on_event.map(|b| Arc::new(Mutex::new(b)));

    // Bridge: local_sender accepts loom::cli_run::AnyStreamEvent (via crate re-export).
    // We pass loom::cli_run::AnyStreamEvent directly since loom_sender wraps loom events.
    let local_sender = opts.any_stream_event_sender.clone();
    let loom_sender: Option<Arc<dyn Fn(loom::cli_run::AnyStreamEvent) + Send + Sync>> =
        local_sender.clone().map(|ls| {
            Arc::new(move |ev: loom::cli_run::AnyStreamEvent| {
                ls(ev);
            }) as Arc<dyn Fn(loom::cli_run::AnyStreamEvent) + Send + Sync>
        });

    // any_stream_event_sender is disabled in loom-agent-patterns ActNode (hardcoded to None).
    // loom_types::cli_run::AnyStreamEvent differs from loom::cli_run::AnyStreamEvent,
    // so we cannot pass local_sender directly to stream_with_config.
    let _unused_local_sender = local_sender;

    let result = match &runner {
        AnyRunner::React(r) => {
            let sink = on_event.clone();
            let loom_sender_clone = loom_sender.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<ReActState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::React(ev.clone()));
                    }
                    if let Some(ref sender) = loom_sender_clone {
                        sender(loom::cli_run::AnyStreamEvent::React(ev));
                    }
                }
            });
            // Pass None for any_stream_event_sender: type mismatch with loom-agent-patterns
            // (expects loom_types::cli_run::AnyStreamEvent but we have loom::cli_run::AnyStreamEvent).
            // Event forwarding still works via on_ev callback + loom_sender.
            let outcome = r
                .stream_with_config(opts.message.as_text().as_ref(), None, on_ev, None)
                .await?;
            match outcome {
                loom_agent_patterns::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                loom_agent_patterns::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev, None).await?;
            match outcome {
                loom_agent_patterns::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                loom_agent_patterns::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev, None).await?;
            match outcome {
                loom_agent_patterns::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                loom_agent_patterns::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev, None).await?;
            match outcome {
                loom_agent_patterns::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.summary_result(),
                        reasoning_content: None,
                    })
                }
                loom_agent_patterns::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
    };

    Ok(result)
}

pub async fn run_agent_with_options(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
) -> Result<RunCompletion, RunError> {
    run_agent(opts, cmd, on_event, None).await
}

pub async fn run_agent_with_llm_override(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<RunCompletion, RunError> {
    run_agent(opts, cmd, on_event, llm_override).await
}

pub async fn build_runner(
    config: &ReactBuildConfig,
    opts: &RunOptions,
    cmd: &RunCmd,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<AnyRunner, RunError> {
    let config = loom::tier::resolve_tier_and_build_config(config).await;
    let cancellation = opts.cancellation.as_ref().map(RunCancellation::token);
    let llm_override_provider: Option<Arc<dyn loom::llm::LlmProvider>> = llm_override.map(|llm| {
        Arc::new(loom::llm::FixedLlmProvider {
            client: Arc::from(llm),
            model_id: "override".to_string(),
        }) as Arc<dyn loom::llm::LlmProvider>
    });
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(&config, llm_override_provider, opts.verbose).await?;
            // Skip with_cancellation: loom-agent-patterns ReactRunner expects
            // loom_types::cli_run::RunCancellation but we have loom::active_operation::RunCancellation.
            // Cancellation forwarding is disabled in loom-agent-patterns runner anyway (hardcoded None).
            Ok(AnyRunner::React(r))
        }
        RunCmd::Dup => {
            let llm_boxed = llm_override_provider.map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_dup_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Dup(r.with_cancellation(cancellation.clone())))
        }
        RunCmd::Tot => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_tot_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Tot(r.with_cancellation(cancellation.clone())))
        }
        RunCmd::Got { .. } => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_got_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Got(r.with_cancellation(cancellation)))
        }
    }
}

pub use loom::tier::{resolve_tier_and_build_config, resolve_tier_and_build_config_with_resolver};
