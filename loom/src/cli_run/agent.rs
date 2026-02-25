//! Unified agent runner: ReAct, DUP, ToT, GoT.

use crate::cli_run::build_helve_config;
use crate::export::stream_event_to_format_a;
use crate::protocol::stream::stream_event_to_protocol_envelope;
use crate::protocol::EnvelopeState;
use crate::protocol::ProtocolEventEnvelope;
use crate::{
    build_dup_runner, build_got_runner, build_react_runner, build_tot_runner, DupRunner, DupState,
    GotRunner, GotState, ReActState, ReactBuildConfig, ReactRunner, StreamEvent, TotRunner,
    TotState,
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
    /// When set, path to a file whose content is used as the agent's role/persona (SOUL).
    /// Overrides SOUL.md and the built-in default. Read at build_helve_config time.
    pub role_file: Option<PathBuf>,
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
            thread_id: None,
            role_file: None,
            verbose: false,
            got_adaptive,
            display_max_len: 120,
            output_json: true,
        }
    }

    fn minimal_config_with_invalid_working_folder() -> ReactBuildConfig {
        ReactBuildConfig {
            db_path: None,
            thread_id: None,
            user_id: None,
            system_prompt: None,
            exa_api_key: None,
            twitter_api_key: None,
            mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
            mcp_remote_cmd: "npx".to_string(),
            mcp_remote_args: "-y mcp-remote".to_string(),
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            model: None,
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
        }
    }

    #[test]
    fn any_stream_event_conversion_covers_all_variants() {
        let react = AnyStreamEvent::React(StreamEvent::TaskStart {
            node_id: "think".to_string(),
        });
        let dup = AnyStreamEvent::Dup(StreamEvent::TaskStart {
            node_id: "plan".to_string(),
        });
        let tot = AnyStreamEvent::Tot(StreamEvent::TaskStart {
            node_id: "think_expand".to_string(),
        });
        let got = AnyStreamEvent::Got(StreamEvent::TaskStart {
            node_id: "plan_graph".to_string(),
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
            thread_id: None,
            role_file: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json: false,
        };
        assert!(build_runner(&cfg, &opts, &RunCmd::React).await.is_err());
        assert!(build_runner(&cfg, &opts, &RunCmd::Dup).await.is_err());
        assert!(build_runner(&cfg, &opts, &RunCmd::Tot).await.is_err());
        assert!(
            build_runner(&cfg, &opts, &RunCmd::Got { got_adaptive: true })
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
            let res = run_agent(&opts_for_error(&cmd), &cmd, None).await;
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
                messages: vec![Message::user("u"), Message::Assistant("a".to_string())],
                ..ReActState::default()
            },
            tot: TotExtension::default(),
        };
    }
}
