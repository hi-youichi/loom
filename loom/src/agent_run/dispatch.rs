use std::sync::Arc;
use std::sync::Mutex;

use agent::run_types::{AgentRunResult, RunCompletion, RunOptions};
use agent::RunnerError;
use agent::agent::react::build::{build_react_runner, BuildRunnerError};
use agent::agent::react::ReactRunner;
use agent::runner_common;
use agent_extensions::dup::build::build_dup_runner;
use agent_extensions::dup::{DupRunner, DupState};
use agent_extensions::got::build::build_got_runner;
use agent_extensions::got::{GotRunner, GotState};
use agent_extensions::tot::build::build_tot_runner;
use agent_extensions::tot::{TotRunner, TotState};
use loom_llm::LlmClient;
use loom_llm::support::uuid6::uuid6;
use crate::agent_run::stream_convert::{
    stream_event_to_format_a, stream_event_to_protocol_envelope, ProtocolEventEnvelope,
};
use stream_event::EnvelopeState;
use loom_react_config::ReactBuildConfig;
use loom_stream::StreamEvent;
use loom_types::active_operation::RunCancellation;
use loom_types::state::ReActState;
use serde_json::Value;
use thiserror::Error;
use tracing::Instrument;

use crate::cli_run::build_react_config;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("build runner: {0}")]
    Build(#[from] BuildRunnerError),
    #[error("run: {0}")]
    Run(#[from] RunnerError),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("remote: {0}")]
    Remote(String),
    #[error("config: {0}")]
    ConfigError(String),
}

#[derive(Clone, Debug)]
pub enum RunCmd {
    React,
    Dup,
    Tot,
    Got { got_adaptive: bool },
}

pub enum AnyRunner {
    React(ReactRunner),
    Dup(DupRunner),
    Tot(TotRunner),
    Got(GotRunner),
}

#[derive(Debug)]
pub enum AnyStreamEvent {
    React(StreamEvent<ReActState>),
    Dup(StreamEvent<DupState>),
    Tot(StreamEvent<TotState>),
    Got(StreamEvent<GotState>),
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

    pub fn from_loom(ev: loom_cli_types::AnyStreamEvent) -> Self {
        match ev {
            loom_cli_types::AnyStreamEvent::React(e) => AnyStreamEvent::React(e),
            _ => AnyStreamEvent::React(StreamEvent::Custom(serde_json::json!({"type": "noop"}))),
        }
    }
}

pub fn to_loom_any_stream_event(ev: &AnyStreamEvent) -> Option<loom_cli_types::AnyStreamEvent> {
    match ev {
        AnyStreamEvent::React(e) => Some(loom_cli_types::AnyStreamEvent::React(e.clone())),
        _ => None,
    }
}

#[allow(clippy::type_complexity)]
pub async fn run_agent(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
    llm_override: Option<Box<dyn LlmClient>>,
) -> Result<RunCompletion, RunError> {
    let loom_opts = opts.to_cli_run_options();
    let (mut config, _resolved_agent) = build_react_config(&loom_opts);
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
        let wt_config = loom_worktree::WorktreeConfig::default();
        if let Ok(manager) = loom_worktree::WorktreeManager::from_working_dir(current_dir, wt_config) {
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

    let result = match &runner {
        AnyRunner::React(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<ReActState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::React(ev.clone()));
                    }
                }
            });
            let outcome = r
                .stream_with_config(opts.message.as_text().as_ref(), None, on_ev)
                .await?;
            match outcome {
                runner_common::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev).await?;
            match outcome {
                runner_common::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev).await?;
            match outcome {
                runner_common::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.last_assistant_reply().unwrap_or_default(),
                        reasoning_content: state.last_reasoning_content(),
                    })
                }
                runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
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
            let outcome = r.stream_with_config(opts.message.as_text().as_ref(), None, on_ev).await?;
            match outcome {
                runner_common::StreamRunOutcome::Finished(state) => {
                    RunCompletion::Finished(AgentRunResult {
                        reply: state.summary_result(),
                        reasoning_content: None,
                    })
                }
                runner_common::StreamRunOutcome::Cancelled => RunCompletion::Cancelled,
            }
        }
    };

    if let RunCompletion::Finished(ref _run_result) = result {
        // CLI path: spawn external review subprocess (preserves original behavior).
        // ACP path: skip — ACP triggers in-process background review itself.
        if opts.acp_session_id.is_none() {
            let session_id = opts
                .thread_id
                .clone()
                .or_else(|| opts.session_id.clone())
                .unwrap_or_else(|| uuid6().to_string());

            tracing::info!(
                session_id = %session_id,
                "Spawning background review subprocess after agent session"
            );

            loom_curator::spawn_review_after_session(
                session_id,
                config.model.clone(),
            );
        }
    }

    Ok(result)
}

pub async fn run_agent_with_options(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
) -> Result<RunCompletion, RunError> {
    let thread_id = opts.thread_id.clone();
    let root_span = match thread_id.as_deref() {
        Some(tid) => tracing::info_span!("agent_run", thread_id = tid),
        None => {
            let id = uuid6().to_string();
            tracing::info_span!("agent_run", thread_id = %id)
        }
    };

    run_agent(opts, cmd, on_event, None)
        .instrument(root_span)
        .await
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
    let config = loom_react_config::resolve_tier_and_build_config(config).await;
    let cancellation = opts.cancellation.as_ref().map(RunCancellation::token);
    let llm_override_provider: Option<Arc<dyn loom_llm::LlmProvider>> = llm_override.map(|llm| {
        Arc::new(loom_llm::client::FixedLlmProvider {
            client: Arc::from(llm),
            model_id: "override".to_string(),
        }) as Arc<dyn loom_llm::LlmProvider>
    });
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(&config, llm_override_provider, opts.verbose, None, opts.any_stream_event_sender.clone()).await?;
            Ok(AnyRunner::React(r.with_cancellation(opts.cancellation.clone())))
        }
        RunCmd::Dup => {
            let llm_boxed = llm_override_provider.map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_dup_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Dup(r.with_cancellation(cancellation.clone()).with_any_stream_event_sender(opts.any_stream_event_sender.clone())))
        }
        RunCmd::Tot => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_tot_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Tot(r.with_cancellation(cancellation.clone()).with_any_stream_event_sender(opts.any_stream_event_sender.clone())))
        }
        RunCmd::Got { .. } => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_got_runner(&config, llm_boxed, opts.verbose).await?;
            Ok(AnyRunner::Got(r.with_cancellation(cancellation).with_any_stream_event_sender(opts.any_stream_event_sender.clone())))
        }
    }
}
