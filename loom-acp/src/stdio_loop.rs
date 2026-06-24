//! Stdio JSON-RPC main loop.
//!
//! Reads JSON-RPC requests from stdin, dispatches to the ACP agent, and writes
//! responses/notifications to stdout.  The loop is split into focused helpers:
//!
//! - [`run_stdio_loop`] — thin orchestrator (logging, LocalSet, cleanup).
//! - [`build_agent_and_channel`] — construct [`LoomAcpAgent`] + notification channel.
//! - [`spawn_drain_task`] — background task forwarding notifications to the client.
//! - [`register_handlers_and_connect`] — wire all ACP request/notification handlers
//!   onto the builder and drive `connect_to` to completion.

use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, ForkSessionRequest,
    ForkSessionResponse, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionNotification,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
SetSessionModeResponse,
    // SetSessionModelRequest/Response: removed in agent-client-protocol-schema 0.14.0.
    // Model selection is now routed via SetSessionConfigOptionRequest (configId="model").
};
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectionTo, Responder, on_receive_notification,
    on_receive_request,
};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::LoomAcpAgent;
use crate::logging;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn is_connection_closed_error_str(s: &str) -> bool {
    s.contains("receiver dropped")
        || s.contains("failed to send response")
        || s.contains("broken pipe")
        || s.contains("unexpected eof")
}

fn is_connection_closed_error(e: &agent_client_protocol::Error) -> bool {
    let msg = &e.message;
    is_connection_closed_error_str(msg)
        || e
            .data
            .as_ref()
            .is_some_and(|d| d.as_str().is_some_and(is_connection_closed_error_str))
}

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Result of [`run_stdio_loop`].
#[derive(Debug)]
pub struct StdioLoopResult {
    /// `true` when the loop ended because the client closed the connection
    /// (normal shutdown), `false` when stdin reached EOF.
    pub connection_closed: bool,
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Run the ACP stdio main loop.
///
/// Reads JSON-RPC requests from stdin, dispatches to [`LoomAcpAgent`], and
/// writes responses/notifications to stdout.  Returns when stdin is closed
/// (EOF) or a fatal I/O / protocol error occurs.
///
/// # Errors
///
/// Returns `Err` on I/O or protocol errors that are not related to normal
/// connection closure.
pub async fn run_stdio_loop() -> Result<StdioLoopResult, Box<dyn std::error::Error + Send + Sync>>
{
    logging::init_logging(None);
    tracing::info!("run_stdio_loop starting");

    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async {
            let (agent, rx) = build_agent_and_channel()?;
            let conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>> =
                Arc::new(tokio::sync::RwLock::new(None));

            let drain_task = spawn_drain_task(rx, conn_shared.clone());

            register_handlers_and_connect(agent.clone(), conn_shared).await?;

            agent.cancel_all();
            // Give cancelled tasks and pending session notifications a brief grace period
            // to wind down before the LocalSet is dropped.
            let _ = tokio::time::timeout(Duration::from_millis(200), drain_task).await;
            Ok(())
        })
        .await;

    match result {
        Ok(_) => Ok(StdioLoopResult {
            connection_closed: false,
        }),
        Err(e) => {
            if is_connection_closed_error(&e) {
                tracing::info!("run_stdio_loop finished (connection closed)");
                Ok(StdioLoopResult {
                    connection_closed: true,
                })
            } else {
                tracing::error!(?e, "run_stdio_loop error");
                Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create the [`LoomAcpAgent`] paired with the notification channel receiver.
fn build_agent_and_channel() -> Result<
    (Arc<LoomAcpAgent>, mpsc::Receiver<SessionNotification>),
    agent_client_protocol::Error,
> {
    let (tx, rx) = mpsc::channel::<SessionNotification>(64);
    let agent = LoomAcpAgent::with_session_update_tx(tx)
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
    Ok((Arc::new(agent), rx))
}

/// Spawn a local task that drains session notifications from the channel and
/// forwards them to the client connection (once available).
fn spawn_drain_task(
    mut rx: mpsc::Receiver<SessionNotification>,
    conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_local(async move {
        while let Some(n) = rx.recv().await {
            let guard = conn_shared.read().await;
            if let Some(conn) = guard.as_ref() {
                if let Err(e) = conn.send_notification(n) {
                    tracing::error!(error = ?e, "Failed to send session notification");
                }
            } else {
                tracing::trace!("Session notification dropped (no connection yet)");
            }
        }
        tracing::info!("Session notification channel closed");
    })
}

/// Register all ACP request / notification handlers on the builder, then drive
/// `connect_to` to completion over stdin/stdout.
async fn register_handlers_and_connect(
    agent: Arc<LoomAcpAgent>,
    conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>>,
) -> Result<(), agent_client_protocol::Error> {
    // Each handler closure is `move`, so we need one Arc clone per handler.
    let a_init = agent.clone();
    let a_auth = agent.clone();
    let a_new = agent.clone();
    let a_prompt = agent.clone();
    let a_fork = agent.clone();
let a_load = agent.clone();
    let a_list = agent.clone();
    let a_config = agent.clone();
    let a_mode = agent.clone();
    // a_model removed: SetSessionModelRequest is gone in 0.14.0; model selection flows through SetSessionConfigOptionRequest.
    let a_cancel = agent.clone();
    let conn_for_init = conn_shared.clone();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let stdin_compat = stdin.compat();
    let stdout_compat =
        <tokio::io::Stdout as TokioAsyncWriteCompatExt>::compat_write(stdout);

    Agent
        .builder()
        .on_receive_request(
            move |req: InitializeRequest,
                  responder: Responder<InitializeResponse>,
                  conn: ConnectionTo<Client>| {
                let agent = a_init.clone();
                let conn_for_init = conn_for_init.clone();
                async move {
                    let result = agent.initialize(req).await;
                    let _ = responder.respond_with_result(result);
                    {
                        let mut guard = conn_for_init.write().await;
                        *guard = Some(conn);
                    }
                    let conn_shared_clone = conn_for_init.clone();
                    crate::tools::set_connection(conn_shared_clone);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: AuthenticateRequest,
                  responder: Responder<AuthenticateResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_auth.clone();
                async move {
                    let result = agent.authenticate(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: NewSessionRequest,
                  responder: Responder<NewSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_new.clone();
                async move {
                    let result = agent.new_session(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: PromptRequest,
                  responder: Responder<PromptResponse>,
                  conn: ConnectionTo<Client>| {
                let agent = a_prompt.clone();
                // Spawn the prompt task to avoid blocking the event loop
                let _ = conn.spawn(async move {
                    let result = agent.prompt(req).await;
                    // Ignore "receiver dropped" errors - connection may have closed
                    let _ = responder.respond_with_result(result);
                    Ok(())
                });
                // Return immediately to unblock the IO loop
                async { Ok(()) }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: ForkSessionRequest,
                  responder: Responder<ForkSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_fork.clone();
                async move {
                    let result = agent.fork_session(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: LoadSessionRequest,
                  responder: Responder<LoadSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_load.clone();
                async move {
                    let result = agent.load_session(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: ListSessionsRequest,
                  responder: Responder<ListSessionsResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_list.clone();
                async move {
                    let result = agent.list_sessions(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: SetSessionConfigOptionRequest,
                  responder: Responder<SetSessionConfigOptionResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_config.clone();
                async move {
                    let result = agent.set_session_config_option(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: SetSessionModeRequest,
                  responder: Responder<SetSessionModeResponse>,
                  _conn: ConnectionTo<Client>| {
                let agent = a_mode.clone();
                async move {
                    let result = agent.set_session_mode(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
on_receive_request!(),
        )
        .on_receive_notification(
            move |notif: CancelNotification, _conn: ConnectionTo<Client>| {
                let agent = a_cancel.clone();
                async move {
                    if let Err(e) = agent.cancel(notif).await {
                        tracing::error!(error = ?e, "cancel notification handler failed");
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .connect_to(ByteStreams::new(stdout_compat, stdin_compat))
        .await
        .map_err(|e| {
            let err_str = format!("{:?}", e);
            if is_connection_closed_error_str(&err_str) {
                tracing::info!("connect_to finished: connection closed");
            } else {
                tracing::error!(?e, "connect_to failed");
            }
            agent_client_protocol::Error::internal_error().data(err_str)
        })?;

    Ok(())
}
