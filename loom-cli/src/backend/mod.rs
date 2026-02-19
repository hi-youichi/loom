//! RunBackend: abstract over local execution vs remote WebSocket.

mod auto_start;
mod local;
mod remote;

pub use auto_start::{ensure_server_or_spawn, spawn_serve, wait_for_server};
pub use local::LocalBackend;
pub use remote::RemoteBackend;

use async_trait::async_trait;
use loom::{Envelope, RunCmd, RunError, RunOptions};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use crate::ToolShowFormat;

/// Optional sink for streaming JSON: each event is passed to the closure as it arrives.
pub type StreamOut = Option<Arc<Mutex<dyn FnMut(Value) + Send>>>;

/// Result of a single agent run: either plain reply or events + reply for --json.
/// When json output is used, reply_envelope is set for the reply line (protocol_spec §5).
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
    /// When stream_out is Some, each event is written immediately via the closure; result is Reply(reply).
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
