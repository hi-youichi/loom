//! Unified agent runner: ReAct, DUP, ToT, GoT.

use crate::cli_run::build_helve_config;
use crate::export::stream_event_to_format_a;
use crate::{
    build_dup_runner, build_got_runner, build_react_runner, build_tot_runner, DupRunner, DupState,
    GotRunner, GotState, ReactBuildConfig, ReactRunner, ReActState, StreamEvent, TotRunner, TotState,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{info_span, Instrument};

/// Options for running the Helve agent.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub message: String,
    pub working_folder: Option<PathBuf>,
    pub thread_id: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    /// When true, stream events are collected and returned as JSON (CLI --json).
    pub output_json: bool,
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
}

/// Runs the agent. When `on_event` is Some, it is invoked for each stream event.
/// The server can pass a closure that converts to format A via `ev.to_format_a()` and sends over WebSocket.
/// The CLI can pass a closure that formats to stderr.
pub async fn run_agent(
    opts: &RunOptions,
    cmd: &RunCmd,
    on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>,
) -> Result<String, RunError> {
    let (_helve, mut config) = build_helve_config(opts);
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

    let runner = build_runner(&config, opts, cmd)
        .instrument(span.clone())
        .await?;

    let on_event: Option<Arc<Mutex<Box<dyn FnMut(AnyStreamEvent) + Send>>>> =
        on_event.map(|b| Arc::new(Mutex::new(b)));

    let reply = match &runner {
        AnyRunner::React(r) => {
            let sink = on_event.clone();
            let on_ev = sink.map(|s| {
                move |ev: StreamEvent<ReActState>| {
                    if let Ok(mut f) = s.lock() {
                        f(AnyStreamEvent::React(ev));
                    }
                }
            });
            let state = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
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
            let state = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
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
            let state = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
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
            let state = r
                .stream_with_config(opts.message.as_str(), None, on_ev)
                .instrument(span.clone())
                .await?;
            state.summary_result()
        }
    };

    Ok(reply)
}

/// Builds the runner for the given command.
pub async fn build_runner(
    config: &ReactBuildConfig,
    opts: &RunOptions,
    cmd: &RunCmd,
) -> Result<AnyRunner, RunError> {
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(config, None, opts.verbose, None).await?;
            Ok(AnyRunner::React(r))
        }
        RunCmd::Dup => {
            let r = build_dup_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Dup(r))
        }
        RunCmd::Tot => {
            let r = build_tot_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Tot(r))
        }
        RunCmd::Got { .. } => {
            let r = build_got_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Got(r))
        }
    }
}
