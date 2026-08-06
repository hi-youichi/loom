//! Shared ACP connection logic.
//!
//! Exports [`run_agent_connection`] which drives the ACP JSON-RPC dispatch
//! loop over any line-based transport (stdin/stdout or WebSocket).

use std::future::Future;
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
use tokio::sync::mpsc as tokio_mpsc;

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

// ---------------------------------------------------------------------------
// Shared connection logic (used by WebSocket transport on the server side)
// ---------------------------------------------------------------------------

/// Register all ACP handlers on the builder and drive `connect_with` to
/// completion over any `Lines`-compatible transport.
///
/// Creates an internal notification drain task that forwards
/// [`SessionNotification`]s from the agent channel to the client connection.
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

    let result = Agent.builder()
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
                let _ = conn.clone().spawn(async move {
                    let result = agent.prompt(req).await;
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }).map_err(|e| {
                    tracing::error!(error = ?e, "Failed to spawn prompt task — client will not receive a response");
                    e
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
        });

    crate::tools::remove_session_bridge("default");

    result?;

    Ok(())
}
