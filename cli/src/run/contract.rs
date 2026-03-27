//! CLI run result types, JSON streaming contract, and in-process dispatch to agent/tool helpers.

use crate::model_cmd::{list_all_models, list_provider_models};
use crate::tool_cmd::{list_tools, show_tool, ToolShowFormat};
use loom::{Envelope, RunCmd, RunError, RunOptions};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use super::run_agent_wrapper as run_agent;
use super::{RunAgentOutput, RunStopReason};

/// Optional sink for JSON stream output (used by `--json`).
///
/// - `Some(...)`: events are forwarded immediately as they arrive (stdout or a file).
/// - `None`: the runner collects events in memory and returns them at the end.
pub type StreamOut = Option<Arc<Mutex<dyn FnMut(Value) + Send>>>;

/// Output of a single run.
///
/// - Without `--json`: callers typically print only the final reply (keep stdout clean).
/// - With `--json`: the reply is accompanied by a list of stream events (or events are
///   emitted incrementally via [`StreamOut`]).
///
/// `reply_envelope`: when using the protocol envelope (`session_id`/`node_id`/`event_id`),
/// the reply line also includes an envelope so it can be correlated with the event stream.
#[derive(Debug)]
pub enum RunOutput {
    Reply {
        reply: String,
        reasoning_content: Option<String>,
        reply_envelope: Option<Envelope>,
        stop_reason: RunStopReason,
    },
    Json {
        events: Vec<Value>,
        reply: String,
        reasoning_content: Option<String>,
        reply_envelope: Option<Envelope>,
        stop_reason: RunStopReason,
    },
}

/// Streaming contract:
/// - `stream_out = Some`: MUST NOT accumulate events; forward each event immediately and
///   return `RunOutput::Reply { .. }`.
/// - `stream_out = None`: may accumulate events. If `opts.output_json` is true, return
///   `RunOutput::Json { .. }`; otherwise return `RunOutput::Reply`.
pub async fn run_cli_turn(
    opts: &RunOptions,
    cmd: &RunCmd,
    stream_out: StreamOut,
) -> Result<RunOutput, RunError> {
    let output = run_agent(opts, cmd, stream_out).await?;
    let RunAgentOutput {
        reply,
        reasoning_content,
        events,
        reply_envelope,
        stop_reason,
    } = output;
    Ok(match events {
        Some(ev) => RunOutput::Json {
            events: ev,
            reply,
            reasoning_content,
            reply_envelope,
            stop_reason,
        },
        None => RunOutput::Reply {
            reply,
            reasoning_content,
            reply_envelope,
            stop_reason,
        },
    })
}

pub async fn cli_list_tools(opts: &RunOptions) -> Result<(), RunError> {
    list_tools(opts).await
}

pub async fn cli_show_tool(
    opts: &RunOptions,
    name: &str,
    format: ToolShowFormat,
) -> Result<(), RunError> {
    show_tool(opts, name, format).await
}

pub async fn cli_list_models(
    opts: &RunOptions,
    provider_name: Option<&str>,
) -> Result<(), RunError> {
    match provider_name {
        Some(name) => list_provider_models(name, opts.output_json).await,
        None => list_all_models(opts.output_json).await,
    }
}
