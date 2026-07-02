//! Runner execution: build_runner + run_agent_from_config.
//!
//! Extracted from the former `loom::agent_run::dispatch` module.
//! The `run_agent` convenience wrapper that combined config building + execution
//! \+ app-side side effects (worktree, debug_llm, curator spawn) has been removed.
//! Consumers should call `build_react_config` then `run_agent_from_config`.

use std::sync::Arc;
use std::sync::Mutex;

use crate::agent::react::build::{build_react_runner, BuildRunnerError};
use crate::agent::react::ReactRunner;
use crate::runner_common;
use crate::agent::dup::build::build_dup_runner;
use crate::agent::dup::{DupRunner, DupState};
use crate::agent::got::build::build_got_runner;
use crate::agent::got::{GotRunner, GotState};
use crate::agent::tot::build::build_tot_runner;
use crate::agent::tot::{TotRunner, TotState};
use loom_llm::LlmClient;
use loom_llm::support::uuid6::uuid6;
use stream_event::convert::{
    stream_event_to_format_a, stream_event_to_protocol_envelope, ProtocolEventEnvelope,
};
use stream_event::envelope::EnvelopeState;
use crate::agent::ReactBuildConfig;
use loom_stream::StreamEvent;
use tool_core::active_operation::RunCancellation;
use loom_stream::state::ReActState;
use serde_json::Value;
use thiserror::Error;
use tracing::Instrument;

use super::types::{AgentRunResult, RunCompletion};

#[derive(Debug, Error)]
pub enum RunError {
    #[error("build runner: {0}")]
    Build(#[from] BuildRunnerError),
    #[error("run: {0}")]
    Run(#[from] crate::RunnerError),
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

/// Execution parameters passed alongside the config.
pub struct RunParams {
    pub message: loom_llm::message::UserContent,
    pub verbose: bool,
    pub cancellation: Option<RunCancellation>,
    pub any_stream_event_sender: Option<Arc<dyn Fn(loom_stream::TypedAnyStreamEvent) + Send + Sync>>,
    pub llm_override: Option<Box<dyn LlmClient>>,
}

#[derive(Debug)]
pub enum TypedAnyStreamEvent {
    React(StreamEvent<ReActState>),
    Dup(StreamEvent<DupState>),
    Tot(StreamEvent<TotState>),
    Got(StreamEvent<GotState>),
}

impl TypedAnyStreamEvent {
    pub fn to_format_a(&self) -> Result<Value, serde_json::Error> {
        match self {
            Self::React(ev) => stream_event_to_format_a(ev),
            Self::Dup(ev) => stream_event_to_format_a(ev),
            Self::Tot(ev) => stream_event_to_format_a(ev),
            Self::Got(ev) => stream_event_to_format_a(ev),
        }
    }

    pub fn to_protocol_event(
        &self,
        state: &mut EnvelopeState,
    ) -> Result<ProtocolEventEnvelope, serde_json::Error> {
        match self {
            Self::React(ev) => stream_event_to_protocol_envelope(ev, state),
            Self::Dup(ev) => stream_event_to_protocol_envelope(ev, state),
            Self::Tot(ev) => stream_event_to_protocol_envelope(ev, state),
            Self::Got(ev) => stream_event_to_protocol_envelope(ev, state),
        }
    }

    pub fn to_protocol_format(
        &self,
        state: &mut EnvelopeState,
    ) -> Result<Value, serde_json::Error> {
        let event = self.to_protocol_event(state)?;
        event.to_value()
    }

    pub fn from_loom(ev: loom_stream::TypedAnyStreamEvent) -> Self {
        match ev {
            loom_stream::TypedAnyStreamEvent::React(e) => Self::React(e),
            _ => Self::React(StreamEvent::Custom(serde_json::json!({"type": "noop"}))),
        }
    }
}

pub fn to_loom_any_stream_event(ev: &TypedAnyStreamEvent) -> Option<loom_stream::TypedAnyStreamEvent> {
    match ev {
        TypedAnyStreamEvent::React(e) => Some(loom_stream::TypedAnyStreamEvent::React(e.clone())),
        _ => None,
    }
}

#[allow(clippy::type_complexity)]
pub async fn run_agent_from_config(
    config: &ReactBuildConfig,
    cmd: &RunCmd,
    mut params: RunParams,
    on_event: Option<Box<dyn FnMut(TypedAnyStreamEvent) + Send>>,
) -> Result<RunCompletion, RunError> {
    let mut config = config.clone();

    if let RunCmd::Got { got_adaptive } = cmd {
        config.got_config.adaptive = *got_adaptive;
    }

    let runner = build_runner(&config, cmd, &mut params).await?;

    let on_event: Option<Arc<Mutex<Box<dyn FnMut(TypedAnyStreamEvent) + Send>>>> =
        on_event.map(|b| Arc::new(Mutex::new(b)));

    let message = &params.message;
    let result = match &runner {
        AnyRunner::React(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<ReActState>| {
                    if let Ok(mut f) = s.lock() {
                        f(TypedAnyStreamEvent::React(ev.clone()));
                    }
                }
            });
            let outcome = r
                .stream_with_config(message.clone(), None, on_ev)
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
                        f(TypedAnyStreamEvent::Dup(ev));
                    }
                }
            });
            let outcome = r.stream_with_config(message.clone(), None, on_ev).await?;
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
                        f(TypedAnyStreamEvent::Tot(ev));
                    }
                }
            });
            let outcome = r.stream_with_config(message.clone(), None, on_ev).await?;
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
                        f(TypedAnyStreamEvent::Got(ev));
                    }
                }
            });
            let outcome = r.stream_with_config(message.clone(), None, on_ev).await?;
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

    Ok(result)
}

pub async fn run_agent_from_config_traced(
    config: &ReactBuildConfig,
    cmd: &RunCmd,
    params: RunParams,
    on_event: Option<Box<dyn FnMut(TypedAnyStreamEvent) + Send>>,
    thread_id: Option<&str>,
) -> Result<RunCompletion, RunError> {
    let root_span = match thread_id {
        Some(tid) => tracing::info_span!("agent_run", thread_id = tid),
        None => {
            let id = uuid6().to_string();
            tracing::info_span!("agent_run", thread_id = %id)
        }
    };

    run_agent_from_config(config, cmd, params, on_event)
        .instrument(root_span)
        .await
}

pub async fn build_runner(
    config: &ReactBuildConfig,
    cmd: &RunCmd,
    params: &mut RunParams,
) -> Result<AnyRunner, RunError> {
    let config = crate::resolve_tier_and_build_config(config).await;
    let cancellation = params.cancellation.as_ref().map(RunCancellation::token);
    let llm_override_provider: Option<Arc<dyn loom_llm::LlmProvider>> = params.llm_override.take().map(|llm| {
        Arc::new(loom_llm::client::FixedLlmProvider {
            client: Arc::from(llm),
            model_id: "override".to_string(),
        }) as Arc<dyn loom_llm::LlmProvider>
    });
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(&config, llm_override_provider, params.verbose, params.cancellation.clone(), params.any_stream_event_sender.clone()).await?;
            Ok(AnyRunner::React(r))
        }
        RunCmd::Dup => {
            let llm_boxed = llm_override_provider.map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_dup_runner(&config, llm_boxed, params.verbose).await?;
            Ok(AnyRunner::Dup(r.with_cancellation(cancellation.clone()).with_any_stream_event_sender(params.any_stream_event_sender.clone())))
        }
        RunCmd::Tot => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_tot_runner(&config, llm_boxed, params.verbose).await?;
            Ok(AnyRunner::Tot(r.with_cancellation(cancellation.clone()).with_any_stream_event_sender(params.any_stream_event_sender.clone())))
        }
        RunCmd::Got { .. } => {
            let llm_boxed = llm_override_provider.as_ref().map(|p| p.create_client(p.default_model()).unwrap());
            let r = build_got_runner(&config, llm_boxed, params.verbose).await?;
            Ok(AnyRunner::Got(r.with_cancellation(cancellation).with_any_stream_event_sender(params.any_stream_event_sender.clone())))
        }
    }
}
