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
//!
//! # Transport
//!
//! On Windows, `tokio::io::stdin()` and `blocking::Unblock::new(std::io::stdin())`
//! both use a dedicated blocking thread whose `ReadFile` call cannot be cancelled.
//! When the pipe is closed (EOF) the read returns 0, but the async side may never
//! observe it because the waker mechanism stalls, causing `connect_to` to hang
//! indefinitely.  To work around this, stdin is read by a plain [`std::thread`]
//! that pushes lines into a `futures::channel::mpsc` channel.  When stdin EOFs,
//! the thread exits naturally, the sender drops, and the channel-driven stream
//! ends — giving the transport a reliable EOF signal on every platform.

use std::future::Future;
use std::io::{BufRead, Write};
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
    Agent, Client, ConnectionTo, Lines, Responder, on_receive_notification,
    on_receive_request,
};
use futures::channel::mpsc;
use futures::sink::unfold;
use tokio::sync::mpsc as tokio_mpsc;

use crate::LoomAcpAgent;
use crate::logging;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn is_connection_closed_error_str(s: &str) -> bool {
    s.contains("receiver dropped")
        || s.contains("receiver is gone")
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
    (Arc<LoomAcpAgent>, tokio_mpsc::Receiver<SessionNotification>),
    agent_client_protocol::Error,
> {
    let (tx, rx) = tokio_mpsc::channel::<SessionNotification>(64);
    let agent = LoomAcpAgent::with_session_update_tx(tx)
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
    Ok((Arc::new(agent), rx))
}

/// Spawn a local task that drains session notifications from the channel and
/// forwards them to the client connection (once available).
fn spawn_drain_task(
    mut rx: tokio_mpsc::Receiver<SessionNotification>,
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

// ---------------------------------------------------------------------------
// Stdio transport (Windows-safe)
// ---------------------------------------------------------------------------

/// Build a [`Lines`] transport backed by plain OS threads.
///
/// **stdin (incoming):** a dedicated thread reads lines via blocking I/O and
/// pushes them into an unbounded `futures::channel::mpsc`.  When stdin reaches
/// EOF the thread exits, the sender is dropped, and the stream ends — giving the
/// transport a reliable EOF signal.
///
/// **stdout (outgoing):** writes are funnelled through a `std::sync::mpsc`
/// channel to a dedicated writer thread that flushes after every line.
fn build_stdio_transport() -> (
    Lines<
        impl futures::Sink<String, Error = std::io::Error> + Send + 'static,
        impl futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    >,
    impl Future<Output = ()>,
) {
    // --- Incoming: stdin reader thread → futures::channel::mpsc + EOF signal ---
    let (line_tx, line_rx) = mpsc::unbounded::<std::io::Result<String>>();
    let (eof_tx, eof_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("acp-stdin".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                if line_tx.unbounded_send(line).is_err() {
                    break; // receiver dropped — shutdown
                }
            }
            // EOF reached (or error).  Signal the main_fn so connect_with can
            // return cleanly instead of deadlocking on `future::pending()`.
            let _ = eof_tx.send(());
        })
        .expect("spawn acp-stdin reader thread");

    // --- Outgoing: stdout writer thread ← std::sync::mpsc ---
    let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("acp-stdout".into())
        .spawn(move || {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            for line in out_rx.iter() {
                let mut bytes = line.into_bytes();
                bytes.push(b'\n');
                if handle.write_all(&bytes).is_err() || handle.flush().is_err() {
                    break;
                }
            }
        })
        .expect("spawn acp-stdout writer thread");

    let outgoing = unfold(out_tx, |tx, line: String| async move {
        tx.send(line).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdout writer closed")
        })?;
        Ok::<_, std::io::Error>(tx)
    });

    let eof_signal = async move {
        let _ = eof_rx.await;
    };

    (Lines::new(outgoing, line_rx), eof_signal)
}

// ---------------------------------------------------------------------------
// Handler registration + connect
// ---------------------------------------------------------------------------

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

    let (transport, eof_signal) = build_stdio_transport();

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
        .connect_with(transport, move |_conn: ConnectionTo<Client>| async move {
            eof_signal.await;
            // Brief grace period for pending request handlers to finish and
            // flush their responses before the background actors are dropped.
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        })
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
