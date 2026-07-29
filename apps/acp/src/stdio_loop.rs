//! Stdio JSON-RPC main loop and shared ACP connection logic.
//!
//! Exports [`run_agent_connection`] which drives the ACP JSON-RPC dispatch
//! loop over any line-based transport (stdin/stdout or WebSocket).  The stdio
//! entrypoint [`run_stdio_loop`] is a thin wrapper that builds the transport
//! and agent, then delegates to [`run_agent_connection`].
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
};
use agent_client_protocol::{
    on_receive_notification, on_receive_request, Agent, Client, ConnectionTo, Lines, Responder,
};
use futures::channel::mpsc;
use futures::sink::unfold;
use tokio::sync::mpsc as tokio_mpsc;

use crate::logging;
use crate::LoomAcpAgent;

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
        || e.data
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
// Stdio orchestrator
// ---------------------------------------------------------------------------

/// Run the ACP stdio main loop.
///
/// Reads JSON-RPC requests from stdin, dispatches to [`LoomAcpAgent`], and
/// writes responses/notifications to stdout.  Returns when stdin is closed
/// (EOF) or a fatal I/O / protocol error occurs.
pub async fn run_stdio_loop() -> Result<StdioLoopResult, Box<dyn std::error::Error + Send + Sync>> {
    logging::init_logging(None);
    tracing::info!("run_stdio_loop starting");

    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async {
            let (agent, rx) = build_agent_and_channel()?;
            let (transport, eof_signal) = build_stdio_transport();

            let conn_result =
                run_agent_connection(agent.clone(), rx, transport, eof_signal).await;

            agent.cancel_all();
            tokio::time::sleep(Duration::from_millis(200)).await;

            conn_result
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
// Agent + channel factory
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

// ---------------------------------------------------------------------------
// Stdio transport (Windows-safe)
// ---------------------------------------------------------------------------

/// Build a [`Lines`] transport backed by plain OS threads.
fn build_stdio_transport() -> (
    Lines<
        impl futures::Sink<String, Error = std::io::Error> + Send + 'static,
        impl futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    >,
    impl Future<Output = ()>,
) {
    let (line_tx, line_rx) = mpsc::unbounded::<std::io::Result<String>>();
    let (eof_tx, eof_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("acp-stdin".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                if line_tx.unbounded_send(line).is_err() {
                    break;
                }
            }
            let _ = eof_tx.send(());
        })
        .expect("spawn acp-stdin reader thread");

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
// Shared connection logic (used by stdio and WebSocket transports)
// ---------------------------------------------------------------------------

/// Register all ACP handlers on the builder and drive `connect_with` to
/// completion over any `Lines`-compatible transport.
///
/// Creates an internal notification drain task that forwards
/// [`SessionNotification`]s from the agent channel to the client connection.
///
///
/// Both the stdio entrypoint and the WebSocket handler use this function.
pub async fn run_agent_connection<S, St, F>(
    agent: Arc<LoomAcpAgent>,
    notification_rx: tokio_mpsc::Receiver<SessionNotification>,
    transport: Lines<S, St>,
    shutdown: F,
) -> Result<(), agent_client_protocol::Error>
where
    S: futures::Sink<String, Error = std::io::Error> + Send + 'static,
    St: futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>> =
        Arc::new(tokio::sync::RwLock::new(None));

    // Spawn the notification drain task.
    let drain_conn = conn_shared.clone();
    let mut rx = notification_rx;
    tokio::spawn(async move {
        while let Some(n) = rx.recv().await {
            let guard = drain_conn.read().await;
            if let Some(conn) = guard.as_ref() {
                if let Err(e) = conn.send_notification(n) {
                    tracing::error!(error = ?e, "Failed to send session notification");
                }
            } else {
                tracing::trace!("Session notification dropped (no connection yet)");
            }
        }
        tracing::info!("Session notification channel closed");
    });

    let a_init = agent.clone();
    let a_auth = agent.clone();
    let a_new = agent.clone();
    let a_prompt = agent.clone();
    let a_fork = agent.clone();
    let a_load = agent.clone();
    let a_list = agent.clone();
    let a_config = agent.clone();
    let a_mode = agent.clone();
    let a_cancel = agent.clone();
    let conn_for_init = conn_shared.clone();

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
                    crate::tools::set_connection_for_session("default", conn_for_init.clone());
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
                let _ = conn.spawn(async move {
                    let result = agent.prompt(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                });
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
            shutdown.await;
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
