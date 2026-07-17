//! CLI run contracts: structured output for `--json` mode.
//!
//! Provides the types used by `run_cli_turn` to serialize agent output as
//! structured JSON events. Each turn emits a sequence of `StreamOut` events
//! (one per agent event) followed by a final `RunOutput` containing the
//! reply and metadata.
//!
//! The `cli_list_tools` and `cli_show_tool` functions mirror the in-process
//! tool listing API but are usable from the server-transport runner when the
//! agent runs remotely.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::run::{RunCmd, RunError, RunOptions};

/// Callback type for streaming agent events to stdout/stderr.
type EventCallback = Arc<Mutex<dyn FnMut(Value) + Send>>;

/// Structured JSON output for a single CLI run turn.
///
/// Emitted by `run_cli_turn` when `--json` is active:
/// 1. One `StreamOut::Event` per agent stream event.
/// 2. One `StreamOut::Done` with the final reply + metadata.
///
/// The variant determines what appears in the JSON object:
/// ```json
/// // Event variant:
/// { "type": "event", "event": { ... } }
/// // Done variant:
/// { "type": "done", "reply": "...", "stop_reason": "end_turn", ... }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    /// An agent stream event (message chunk, tool call, etc.).
    Event { event: Value },
    /// The run completed with a final reply.
    Done { output: RunOutput },
}

/// Unified output type for a completed CLI run turn.
///
/// Always emitted as `StreamOut::Done` at the end of a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    /// The final assistant reply text.
    pub reply: String,
    /// Opaque reasoning content (e.g. chain-of-thought), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Why the run stopped: `end_turn` or `cancelled`.
    pub stop_reason: String,
    /// All stream events collected during the run (only when not streaming).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<Value>>,
    /// Token usage summary, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
    /// Reply envelope with additional metadata (not serialized, handled separately by output.rs).
    #[serde(skip)]
    pub reply_envelope: Option<crate::Envelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Runs one CLI agent turn and yields structured JSON events.
///
/// - **`output_json=false`**: runs in-process with stderr display, returns
///   `RunOutput` directly (for the normal non-JSON path).
/// - **`output_json=true`**: collects all stream events into a `Vec<Value>`
///   and returns `RunOutput` with `events` populated.
///
/// The `stream_sender` receives raw agent events (used for logging/debugging
/// during the run). The returned `RunOutput` always contains the final reply.
pub async fn run_cli_turn(
    opts: &RunOptions,
    cmd: &RunCmd,
    stream_sender: Option<EventCallback>,
) -> Result<RunOutput, RunError> {
    use super::agent::run_agent_wrapper;

    let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));

    // Wrap each call to `stream_sender` to also store events in `events`.
    let sink: Option<EventCallback> = match stream_sender {
        Some(sender) => {
            let events_clone = events.clone();
            let sender = sender;
            Some(Arc::new(Mutex::new(move |v: Value| {
                if let Ok(mut guard) = events_clone.lock() {
                    guard.push(v.clone());
                }
                if let Ok(mut s) = sender.lock() {
                    s(v);
                }
            })))
        }
        None => None,
    };

    let result = run_agent_wrapper(opts, cmd, sink).await?;

    let stop_reason_str = match result.stop_reason {
        super::agent::RunStopReason::EndTurn => "end_turn",
        super::agent::RunStopReason::Cancelled => "cancelled",
    };

    let output = RunOutput {
        reply: result.reply,
        reasoning_content: result.reasoning_content,
        stop_reason: stop_reason_str.to_string(),
        events: if opts.output_json {
            Some(events.lock().map(|v| v.clone()).unwrap_or_default())
        } else {
            None
        },
        usage: None,
        reply_envelope: result.reply_envelope,
    };

    Ok(output)
}

// ─── Re-export CLI entry points for server-transport runner ────────────────────
//
// These functions are defined in `tool_cmd` and `model_cmd` and re-exported here
// so the server-transport runner (and other consumers) can import them from one
// place: `crate::run::cli_list_tools`, etc.

pub use crate::tool_cmd::list_tools as cli_list_tools;
pub use crate::tool_cmd::show_tool as cli_show_tool;
