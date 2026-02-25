//! Backend abstraction used by the `loom` CLI.
//!
//! The CLI runs the agent in-process via [`LocalBackend`]. This module centralizes
//! the JSON/NDJSON streaming contract and [`RunBackend`] trait.

mod local;

pub use local::LocalBackend;

use crate::ToolShowFormat;
use async_trait::async_trait;
use loom::{Envelope, RunCmd, RunError, RunOptions};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Optional sink for JSON stream output (used by `--json`).
///
/// - `Some(...)`: events are forwarded immediately as they arrive (stdout or a file).
/// - `None`: the backend collects events in memory and returns them at the end.
pub type StreamOut = Option<Arc<Mutex<dyn FnMut(Value) + Send>>>;

/// Output of a single run.
///
/// - Without `--json`: callers typically print only the final reply (keep stdout clean).
/// - With `--json`: the reply is accompanied by a list of stream events (or events are
///   emitted incrementally via [`StreamOut`]).
///
/// `reply_envelope`: when using the protocol envelope (`session_id`/`node_id`/`event_id`),
/// the reply line also includes an envelope (see `docs/protocol_spec.md`, §5) so it can
/// be correlated with the event stream.
#[derive(Debug)]
pub enum RunOutput {
    Reply(String, Option<Envelope>),
    Json {
        events: Vec<Value>,
        reply: String,
        reply_envelope: Option<Envelope>,
    },
}

#[async_trait]
pub trait RunBackend: Send + Sync {
    /// Execute a single agent "turn".
    ///
    /// Streaming contract:
    /// - `stream_out = Some`: the backend MUST NOT accumulate events; it should forward
    ///   each event immediately and return `RunOutput::Reply(reply, envelope)`.
    /// - `stream_out = None`: the backend may accumulate events. If `opts.output_json` is
    ///   true, it should return `RunOutput::Json { events, reply, .. }`; otherwise it should
    ///   return `RunOutput::Reply`.
    async fn run(
        &self,
        opts: &RunOptions,
        cmd: &RunCmd,
        stream_out: StreamOut,
    ) -> Result<RunOutput, RunError>;
    async fn list_tools(&self, opts: &RunOptions) -> Result<(), RunError>;
    async fn show_tool(
        &self,
        opts: &RunOptions,
        name: &str,
        format: ToolShowFormat,
    ) -> Result<(), RunError>;
}
