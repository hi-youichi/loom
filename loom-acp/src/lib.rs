//! # Loom ACP — Run Loom as an ACP Agent
//!
//! This crate implements the **Agent side** of the [Agent Client Protocol (ACP)](https://agentclientprotocol.com):
//! started by IDEs (Zed, JetBrains, etc.) as a subprocess, it communicates over **stdio** via JSON-RPC,
//! maps ACP requests (`initialize`, `session/new`, `session/prompt`, etc.) to [loom]'s
//! `run_agent_with_options`, and streams progress and tool calls back via `session/update` notifications.
//!
//! ## Design principles
//!
//! - **Reuse**: Do not reimplement ReAct/ToT/GoT; only the ACP ↔ Loom adapter layer.
//! - **Session consistency**: ACP `session_id` maps 1:1 to Loom `thread_id` for multi-turn and checkpointer consistency.
//! - **Single process, stdio**: Same as the existing CLI; no extra server or port.
//! - **Trusted**: Use `session/request_permission` before tool execution for user confirmation (ACP Trusted design).
//!
//! ## Architecture layers
//!
//! - **Transport**: `run_stdio_loop()` uses `agent_client_protocol::AgentSideConnection` for the JSON-RPC loop on stdin/stdout; stderr is for logs only.
//! - **Agent**: [`LoomAcpAgent`] implements ACP's `Agent` trait and forwards protocol requests to session, content parsing, and Loom execution.
//! - **Session**: [`SessionStore`] maintains session_id ↔ thread_id, working_directory, and per-session cancel flag.
//! - **Content**: [`content_blocks_to_message`] turns ACP ContentBlock list into a single user message string.
//! - **Stream bridge**: [`stream_bridge`] converts Loom's [`loom::AnyStreamEvent`] into ACP SessionUpdate and sends them.
//!
//! ### Diagram 1: Process and protocol layers
//!
//! ```text
//! +-------------------------------------------------------------------------------+
//! |  IDE (Zed / JetBrains / Neovim ...)              [Client]                    |
//! +-------------------------------------------------------------------------------+
//!          |                                                    ^
//!          | stdin (JSON-RPC Request/Notification)              | stdout (JSON-RPC Response/Notification)
//!          v                                                    |
//! +-------------------------------------------------------------------------------+
//! |  loom-acp process                                                             |
//! |  +-------------------------------------------------------------------------+  |
//! |  |  Transport   run_stdio_loop() / AgentSideConnection                    |  |
//! |  |  - read stdin -> parse JSON-RPC -> dispatch to Agent                    |  |
//! |  |  - Agent return/notifications -> serialize -> write stdout               |  |
//! |  +-------------------------------------------------------------------------+  |
//! |         |                                                      ^             |
//! |         v                                                      |             |
//! |  +-------------------------------------------------------------------------+  |
//! |  |  Agent   LoomAcpAgent (impl Agent)                                      |  |
//! |  |  - initialize / authenticate / new_session / prompt / cancel           |  |
//! |  +-------------------------------------------------------------------------+  |
//! |         |           |              |                    |                    |  |
//! |         v           v              v                    v                    |  |
//! |  +----------+ +----------+ +----------------+ +------------------+           |  |
//! |  | Session  | | Content  | | StreamBridge   | | (connection      | ----------+  |
//! |  | Store    | | Parser   | | AnyStreamEvent | | session/update   |             |
//! |  |          | |          | | -> StreamUpdate| | request_permission|             |
//! |  +----------+ +----------+ +----------------+ +------------------+             |
//! |         |           |              |                                              |
//! |         +-----------+--------------+                                              |
//! |                     |                                                             |
//! |                     v                                                             |
//! |  +-------------------------------------------------------------------------+     |
//! |  |  Loom   loom::run_agent_with_options / build_helve_config              |     |
//! |  |  - RunOptions, RunCmd, on_event(AnyStreamEvent) -> reply                 |     |
//! |  +-------------------------------------------------------------------------+     |
//! |         |  Tools MCP(new_session/config) / local; graph execution -> StreamBridge  |
//! |         |  When permission needed: connection request_permission -> execute or deny |
//! +-------------------------------------------------------------------------------+
//! ```
//!
//! ### Diagram 2: Request/response flow (single prompt turn)
//!
//! ```text
//!   IDE                    loom-acp Transport              LoomAcpAgent              Loom
//!    |                            |                              |                     |
//!    |  initialize                |                              |                     |
//!    |--------------------------->|  Agent::initialize()         |                     |
//!    |                            |----------------------------->|                     |
//!    |                            |<-----------------------------| InitializeResponse |
//!    |<---------------------------|                              |                     |
//!    |  session/new               |                              |                     |
//!    |--------------------------->|  Agent::new_session()        |                     |
//!    |                            |----------------------------->| SessionStore::create|
//!    |                            |<-----------------------------| NewSessionResponse |
//!    |<---------------------------|                              |                     |
//!    |  session/prompt            |                              |                     |
//!    |--------------------------->|  Agent::prompt()              |                     |
//!    |                            |----------------------------->| content_blocks_to_  |
//!    |                            |                              |   message()         |
//!    |                            |                              | SessionStore::get() |
//!    |                            |                              | run_agent_with_     |
//!    |                            |                              |   options() ------->|
//!    |                            |                              |  (on tool call)     |
//!    |                            |  session/update(tool_call) --->| request_permission->| (to IDE)
//!    |                            |<-- permission outcome -------| execute/deny        |
//!    |                            |  session/update(tool_call_   |                    |
//!    |                            |   update) ------------------>| (to IDE)            |
//!    |                            |                              |<--- reply           |
//!    |                            |  (on_event: StreamBridge)     |  session/update --->| (to IDE)
//!    |<------------------------------------------ ... ---------------------------------|
//!    |                            |<-----------------------------| PromptResponse     |
//!    |<---------------------------|                              |                     |
//! ```
//!
//! ### Diagram 3: Module and type dependencies
//!
//! ```text
//!                     +----------+
//!                     |  lib.rs  |  run_stdio_loop()
//!                     +----+-----+
//!                          |
//!          +---------------+---------------+
//!          v               v               v
//!    +----------+   +-----------+   +---------------+
//!    | agent.rs |   | session.rs|   | content.rs    |
//!    | LoomAcp  |   | SessionStore   | content_blocks_to_message
//!    | Agent    |   | SessionId  |   | ContentBlockLike
//!    +----+-----+   | SessionEntry   | ContentError  |
//!          |        +-----------+   +---------------+
//!          |                |
//!          |                |        +-------------------+
//!          |                |        | stream_bridge.rs  |
//!          +----------------+--------+ loom_event_to_   |
//!          |                |        |   updates()      |
//!          v                v        | StreamUpdate     |
//!    +------------------------------------------------------------------+
//!    |  loom crate   RunOptions, RunCmd, AnyStreamEvent, build_helve_config
//!    |  tools from MCP (new_session) / config                           |
//!    +------------------------------------------------------------------+
//! ```
//!
//! ### Diagram 4: session/update data flow
//!
//! ```text
//!   Loom graph                on_event callback           stream_bridge           connection
//!   (StreamEvent)                  |                         |                        |
//!        |                          |                         |                        |
//!        | AnyStreamEvent           |                         |                        |
//!        |------------------------->| loom_event_to_updates() |                        |
//!        |                          |------------------------>|                        |
//!        |                          |                         | Vec<StreamUpdate>      |
//!        |                          |<------------------------|                        |
//!        |                          |  for each update       |                        |
//!        |                          |------------------------------------------------->| session/update
//!        |                          |                         |                        | (notification
//!        |                          |                         |                        |  to IDE)
//! ```
//!
//! ### Diagram 5: Tool call and request_permission
//!
//! ```text
//!   Loom Act node             loom-acp                  connection              IDE / user
//!   (decides to call tool)         |                         |                        |
//!        |                     | session/update          |                        |
//!        |                     | (tool_call, Pending)   |------------------------>|
//!        |                     | request_permission      |------------------------>|
//!        |                     |                         |<-- Allow/Deny/Cancelled-|
//!        |                     |<------------------------|                        |
//!        | execute tool or     |                         |                        |
//!        | write denial        | session/update          |                        |
//!        |-------------------->| (tool_call_update,     |------------------------>|
//!        |                     |  Running->Success/Fail) |                        |
//! ```
//!
//! ## Entrypoint and configuration
//!
//! - **Binary**: `cargo build -p loom-acp` produces the `loom-acp` executable; it does not parse subcommands; all parameters come from `session/new`.
//! - **IDE config**: Set command to `loom-acp` (or full path), args empty; working directory optional.
//! - **Protocol params**: working directory, MCP, etc. are provided by ACP's `session/new` request.
//!
//! ## Errors and cancellation
//!
//! - Invalid session_id -> JSON-RPC invalid_params ("unknown session"); content_blocks parse failure -> invalid_params.
//! - run_agent internal error -> server error, message contains brief reason.
//! - **After session/cancel we must return PromptResponse(StopReason::Cancelled)**, not Finished.
//! - If Loom has no interruptible run_agent API yet: check cancel flag on next poll or node entry and return Cancelled, or add an extension point.
//!
//! Protocol and feature details are in the [`protocol`] module.
//!
//! ## Module overview
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`agent`] | [`LoomAcpAgent`]: ACP Agent implementation, calls loom in prompt |
//! | [`session`] | [`SessionStore`]: session table and cancel flag |
//! | [`content`] | Parse ContentBlock into user message string |
//! | [`stream_bridge`] | Loom stream events -> ACP SessionUpdate |
//! | [`protocol`] | Protocol and Loom mapping summary (initialize/prompt/update/cancel, etc.) |
//! | [`logging`] | Delayed log initialization with working_folder from ACP session |

use agent_client_protocol::Client;
use std::sync::OnceLock;

pub mod agent;
pub mod agent_registry;
pub mod client_capabilities;
pub mod client_methods;
pub mod content;
pub mod last_model;
pub mod logging;
pub mod protocol;
pub mod session;
pub mod stream_bridge;
pub mod terminal;
pub mod tools;

pub use agent::LoomAcpAgent;
pub use content::{content_blocks_to_message, ContentBlockLike, ContentError};
pub use session::{SessionConfig, SessionEntry, SessionId, SessionStore};
pub use stream_bridge::{
    loom_event_to_updates, stream_update_to_session_notification, StreamUpdate,
};

static LOG_CONFIG: OnceLock<logging::LogConfig> = OnceLock::new();

/// Set log config from CLI args (called once at startup).
pub fn set_log_config(config: logging::LogConfig) {
    let _ = LOG_CONFIG.set(config);
}

/// Get log config (returns None if not set).
pub fn get_log_config() -> Option<&'static logging::LogConfig> {
    LOG_CONFIG.get()
}

/// Run the ACP stdio main loop: read JSON-RPC requests from stdin, dispatch to the Agent, write responses and notifications to stdout.
///
/// This function returns when:
/// - stdin is closed (EOF);
/// - a fatal I/O or protocol error occurs.
///
/// # Implementation
///
/// Loads config (same as loom) at startup, then constructs [`LoomAcpAgent`] and uses
/// `agent_client_protocol::AgentSideConnection::new(agent, stdin, stdout, spawn)` to run the I/O future.
///
/// # Errors
///
/// Returns `Err` on I/O or protocol errors; the concrete error type is implementation-defined.
///
/// # Example (integration test)
///
/// ```ignore
/// let rt = tokio::runtime::Runtime::new()?;
/// rt.block_on(loom_acp::run_stdio_loop())?;
/// ```
pub async fn run_stdio_loop() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    tracing::info!("run_stdio_loop starting");
    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async {
            let (tx, mut rx) = mpsc::channel::<agent_client_protocol::SessionNotification>(64);
            let agent = LoomAcpAgent::with_session_update_tx(tx);
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();
            let stdin_compat = stdin.compat();
            let stdout_compat =
                <tokio::io::Stdout as TokioAsyncWriteCompatExt>::compat_write(stdout);
            let (connection, io_future) = agent_client_protocol::AgentSideConnection::new(
                agent,
                stdout_compat,
                stdin_compat,
                |fut| {
                    tokio::task::spawn_local(fut);
                },
            );
            let conn = Arc::new(connection);
            let conn_drain = conn.clone();
            let drain = async move {
                while let Some(n) = rx.recv().await {
                    let _ = conn_drain.session_notification(n).await;
                }
            };
            tokio::select! {
                res = io_future => {
                    tracing::info!(?res, "io_future completed");
                    res
                },
                _ = drain => {
                    tracing::info!("drain completed");
                    Ok(())
                },
            }
        })
        .await
        .map_err(|e: agent_client_protocol::Error| {
            tracing::error!(?e, "run_stdio_loop error");
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        });
    tracing::info!("run_stdio_loop finished");
    result
}
