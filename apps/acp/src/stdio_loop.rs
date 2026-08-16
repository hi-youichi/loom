//! Shared ACP connection logic.
//!
//! Exports [`run_agent_connection`] which drives the ACP JSON-RPC dispatch
//! loop over any line-based transport (stdin/stdout or WebSocket).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::client_capabilities::ClientCapabilitiesInfo;
use crate::connection::{AcpConnection, ConnectionOutbound};
use crate::runtime::AcpRuntime;
use crate::session::SessionId as LoomSessionId;
use agent_client_protocol::schema::v1::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, CloseSessionRequest,
    CloseSessionResponse, DeleteSessionRequest, DeleteSessionResponse, ForkSessionRequest,
    ForkSessionResponse, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse,
};
use agent_client_protocol::{
    on_receive_notification, on_receive_request, Agent, Client, ConnectionTo, Handled, Lines,
    Responder, UntypedMessage,
};

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
// `_loomdesk.dev/*` extension dispatch
// ---------------------------------------------------------------------------

/// Build the extension context for a connection-scoped extension call.
///
/// `working_directory` is resolved from well-known params (`cwd`, `directory`)
/// so a single browser connection can operate on multiple project roots.
/// Params carry absolute paths, so boundary checks remain meaningful when no
/// directory is provided.
async fn extension_context_for(
    connection: &AcpConnection,
    params: &serde_json::Value,
) -> crate::extensions::ExtensionContext {
    let capabilities = connection.require_capabilities().await.unwrap_or_default();
    let working_directory = ["cwd", "directory"]
        .iter()
        .find_map(|key| params.get(*key).and_then(|v| v.as_str()))
        .filter(|raw| !raw.trim().is_empty() && *raw != "/")
        .map(std::path::PathBuf::from)
        .and_then(|p| {
            if p.is_dir() {
                std::fs::canonicalize(&p).ok()
            } else {
                None
            }
        });
    crate::extensions::ExtensionContext {
        session_id: None,
        principal: connection.principal.clone(),
        connection_id: connection.id.clone(),
        working_directory,
        client_capabilities: capabilities,
    }
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
    runtime: Arc<AcpRuntime>,
    connection: Arc<AcpConnection>,
    outbound_rx: tokio::sync::mpsc::Receiver<ConnectionOutbound>,
    transport: Lines<S, St>,
    shutdown: F,
) -> Result<(), agent_client_protocol::Error>
where
    S: futures::Sink<String, Error = std::io::Error> + Send + 'static,
    St: futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    // Spawn the notification drain task.
    let drain_conn = connection.sdk_client_slot();
    let mut rx = outbound_rx;
    tokio::spawn(async move {
        while let Some(outbound) = rx.recv().await {
            let guard = drain_conn.read().await;
            if let Some(conn) = guard.as_ref() {
                match outbound {
                    ConnectionOutbound::Notification { value, enqueued } => {
                        if let Err(e) = conn.send_notification(value) {
                            tracing::error!(error = ?e, "Failed to send session notification");
                        } else if let Some(enqueued) = enqueued {
                            let _ = enqueued.send(());
                        }
                    }
                    ConnectionOutbound::GlobalNotification { method, params } => {
                        let message = agent_client_protocol::UntypedMessage {
                            method: method.clone(),
                            params,
                        };
                        if let Err(e) = conn.send_notification(message) {
                            tracing::debug!(error = ?e, method = %method, "Failed to send global notification");
                        }
                    }
                    ConnectionOutbound::Barrier(enqueued) => {
                        let _ = enqueued.send(());
                    }
                }
            } else {
                tracing::trace!("Session notification dropped (connection not initialized)");
            }
        }
        tracing::info!("Session notification channel closed");
    });

    let a_init = runtime.agent.clone();
    let a_auth = runtime.agent.clone();
    let r_new = runtime.clone();
    let r_prompt = runtime.clone();
    let r_fork = runtime.clone();
    let r_load = runtime.clone();
    let r_resume = runtime.clone();
    let r_close = runtime.clone();
    let r_delete = runtime.clone();
    let a_list = runtime.agent.clone();
    let r_config = runtime.clone();
    let r_mode = runtime.clone();
    let r_cancel = runtime.clone();
    let conn_for_init = connection.clone();
    let conn_for_new = connection.clone();
    let conn_for_prompt = connection.clone();
    let conn_for_fork = connection.clone();
    let conn_for_load = connection.clone();
    let conn_for_resume = connection.clone();
    let conn_for_close = connection.clone();
    let conn_for_delete = connection.clone();
    let conn_for_list = connection.clone();
    let conn_for_config = connection.clone();
    let conn_for_mode = connection.clone();
    let conn_for_cancel = connection.clone();
    let r_ext = runtime.clone();
    let conn_for_ext = connection.clone();

    let result = Agent.builder()
        .on_receive_request(
            move |req: InitializeRequest,
                  responder: Responder<InitializeResponse>,
                  conn: ConnectionTo<Client>| {
                let agent = a_init.clone();
                let conn_for_init = conn_for_init.clone();
                async move {
                    let caps = ClientCapabilitiesInfo::from_json(
                        serde_json::to_value(&req.client_capabilities).ok(),
                    );
                    let result = agent.initialize(req).await;
                    if result.is_ok() {
                        if let Err(error) = conn_for_init.bind_client(conn, caps).await {
                            let _ = responder.respond_with_result(Err(
                                agent_client_protocol::Error::invalid_request()
                                    .data(error.to_string()),
                            ));
                            return Ok(());
                        }
                    }
                    let _ = responder.respond_with_result(result);
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
                let runtime = r_new.clone();
                let connection = conn_for_new.clone();
                async move {
                    if !connection.is_initialized() {
                        let _ = responder.respond_with_result(Err(
                            agent_client_protocol::Error::invalid_request()
                                .data("initialize must complete before session/new"),
                        ));
                        return Ok(());
                    }
                    let result = runtime
                        .agent
                        .new_session_for_owner(req, &connection.principal)
                        .await;
                    if let Ok(response) = &result {
                        let session_id = LoomSessionId::new(response.session_id.to_string());
                        runtime
                            .bindings
                            .bind_new_session(session_id.clone(), connection.id.clone());
                    }
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
                let runtime = r_prompt.clone();
                let connection = conn_for_prompt.clone();
                let _ = conn.clone().spawn(async move {
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let result = if runtime.bindings.connection_for(&session_id).as_deref()
                        != Some(connection.id.as_str())
                    {
                        Err(agent_client_protocol::Error::new(
                            -32011,
                            "session is attached to another connection",
                        ))
                    } else {
                        match connection.require_capabilities().await {
                            Ok(capabilities) => {
                                let bridge = Arc::new(crate::tools::AcpClientBridge::new(
                                    session_id.to_string(),
                                    connection.sdk_client_slot(),
                                ));
                                match runtime.execute_prompt(req, capabilities, bridge).await {
                                    Ok(response) => {
                                        match runtime.flush_notifications(&session_id).await {
                                            Ok(()) => Ok(response),
                                            Err(error) => Err(
                                                agent_client_protocol::Error::internal_error()
                                                    .data(format!(
                                                        "failed to flush prompt updates: {error}"
                                                    )),
                                            ),
                                        }
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                            Err(error) => Err(
                                agent_client_protocol::Error::invalid_request()
                                    .data(error.to_string()),
                            ),
                        }
                    };
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
                let runtime = r_fork.clone();
                let connection = conn_for_fork.clone();
                async move {
                    let source_id = LoomSessionId::new(req.session_id.to_string());
                    let result = if runtime.bindings.connection_for(&source_id).as_deref()
                        != Some(connection.id.as_str())
                    {
                        Err(agent_client_protocol::Error::new(
                            -32011,
                            "session is attached to another connection",
                        ))
                    } else {
                        runtime.agent.fork_session(req).await
                    };
                    if let Ok(response) = &result {
                        let session_id = LoomSessionId::new(response.session_id.to_string());
                        runtime
                            .bindings
                            .bind_new_session(session_id.clone(), connection.id.clone());
                    }
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
                let runtime = r_load.clone();
                let connection = conn_for_load.clone();
                async move {
                    if !connection.is_initialized() {
                        let _ = responder.respond_with_result(Err(
                            agent_client_protocol::Error::invalid_request()
                                .data("initialize must complete before session/load"),
                        ));
                        return Ok(());
                    }
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let previous_lifecycle =
                        match runtime.agent.sessions().begin_restore(&session_id) {
                            Ok(lifecycle) => lifecycle,
                            Err(()) => {
                                let _ = responder.respond_with_result(Err(
                                    agent_client_protocol::Error::new(
                                        -32010,
                                        "a prompt is already in progress for this session",
                                    ),
                                ));
                                return Ok(());
                            }
                        };
                    let previous = runtime
                        .bindings
                        .rebind_session(&session_id, connection.id.clone());
                    let result = runtime
                        .agent
                        .load_session_for_owner(req, &connection.principal)
                        .await;
                    let result = match result {
                        Ok(response) => match runtime.flush_notifications(&session_id).await {
                            Ok(()) => Ok(response),
                            Err(error) => Err(agent_client_protocol::Error::internal_error()
                                .data(format!("failed to flush session history: {error}"))),
                        },
                        Err(error) => Err(error),
                    };
                    if result.is_ok() {
                        runtime.record_session_rebind();
                    }
                    if result.is_err() {
                        runtime
                            .agent
                            .sessions()
                            .restore_lifecycle(&session_id, previous_lifecycle);
                        runtime.bindings.unbind_session(&session_id);
                        if let Some(previous) = previous {
                            runtime
                                .bindings
                                .rebind_session(&session_id, previous.clone());
                        }
                    }
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: ResumeSessionRequest,
                  responder: Responder<ResumeSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let runtime = r_resume.clone();
                let connection = conn_for_resume.clone();
                async move {
                    if !connection.is_initialized() {
                        let _ = responder.respond_with_result(Err(
                            agent_client_protocol::Error::invalid_request()
                                .data("initialize must complete before session/resume"),
                        ));
                        return Ok(());
                    }
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let previous_lifecycle =
                        match runtime.agent.sessions().begin_restore(&session_id) {
                            Ok(lifecycle) => lifecycle,
                            Err(()) => {
                                let _ = responder.respond_with_result(Err(
                                    agent_client_protocol::Error::new(
                                        -32010,
                                        "a prompt is already in progress for this session",
                                    ),
                                ));
                                return Ok(());
                            }
                        };
                    let previous = runtime
                        .bindings
                        .rebind_session(&session_id, connection.id.clone());
                    let result = runtime
                        .agent
                        .resume_session_for_owner(req, &connection.principal)
                        .await;
                    if result.is_ok() {
                        runtime.record_session_rebind();
                    }
                    if result.is_err() {
                        runtime
                            .agent
                            .sessions()
                            .restore_lifecycle(&session_id, previous_lifecycle);
                        runtime.bindings.unbind_session(&session_id);
                        if let Some(previous) = previous {
                            runtime
                                .bindings
                                .rebind_session(&session_id, previous.clone());
                        }
                    }
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: CloseSessionRequest,
                  responder: Responder<CloseSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let runtime = r_close.clone();
                let connection = conn_for_close.clone();
                async move {
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let result = match runtime.bindings.connection_for(&session_id) {
                        Some(bound) if bound != connection.id => Err(
                            agent_client_protocol::Error::new(
                                -32011,
                                "session is attached to another connection",
                            ),
                        ),
                        _ => runtime
                            .agent
                            .close_session_for_owner(req, &connection.principal)
                            .await,
                    };
                    if result.is_ok() {
                        runtime.bindings.unbind_session(&session_id);
                        runtime.cleanup_session_resources(session_id.as_str()).await;
                    }
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            move |req: DeleteSessionRequest,
                  responder: Responder<DeleteSessionResponse>,
                  _conn: ConnectionTo<Client>| {
                let runtime = r_delete.clone();
                let connection = conn_for_delete.clone();
                async move {
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let result = match runtime.bindings.connection_for(&session_id) {
                        Some(bound) if bound != connection.id => Err(
                            agent_client_protocol::Error::new(
                                -32011,
                                "session is attached to another connection",
                            ),
                        ),
                        _ => runtime
                            .agent
                            .delete_session_for_owner(req, &connection.principal)
                            .await,
                    };
                    if result.is_ok() {
                        runtime.bindings.unbind_session(&session_id);
                        runtime.cleanup_session_resources(session_id.as_str()).await;
                    }
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
                let connection = conn_for_list.clone();
                async move {
                    let result = agent
                        .list_sessions_for_owner(req, &connection.principal)
                        .await;
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
                let runtime = r_config.clone();
                let connection = conn_for_config.clone();
                async move {
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let result = if runtime.bindings.connection_for(&session_id).as_deref()
                        != Some(connection.id.as_str())
                    {
                        Err(agent_client_protocol::Error::new(-32011, "session is attached to another connection"))
                    } else {
                        runtime.agent.set_session_config_option(req).await
                    };
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
                let runtime = r_mode.clone();
                let connection = conn_for_mode.clone();
                async move {
                    let session_id = LoomSessionId::new(req.session_id.to_string());
                    let result = if runtime.bindings.connection_for(&session_id).as_deref()
                        != Some(connection.id.as_str())
                    {
                        Err(agent_client_protocol::Error::new(-32011, "session is attached to another connection"))
                    } else {
                        runtime.agent.set_session_mode(req).await
                    };
                    let _ = responder.respond_with_result(result);
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            move |notif: CancelNotification, _conn: ConnectionTo<Client>| {
                let runtime = r_cancel.clone();
                let connection = conn_for_cancel.clone();
                async move {
                    let session_id = LoomSessionId::new(notif.session_id.to_string());
                    if runtime.bindings.connection_for(&session_id).as_deref()
                        == Some(connection.id.as_str())
                    {
                        if let Err(e) = runtime.agent.cancel(notif).await {
                            tracing::error!(error = ?e, "cancel notification handler failed");
                        }
                    } else {
                        tracing::warn!(session_id = %session_id, "ignoring cancel from unbound connection");
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .on_receive_request(
            move |req: UntypedMessage,
                  responder: Responder<serde_json::Value>,
                  _conn: ConnectionTo<Client>| {
                let runtime = r_ext.clone();
                let connection = conn_for_ext.clone();
                async move {
                    if !req.method.starts_with(crate::extensions::EXTENSION_PREFIX) {
                        return Ok(Handled::No {
                            message: (req, responder),
                            retry: false,
                        });
                    }
                    let method = req.method.clone();
                    let params = req.params.clone();
                    let ctx = extension_context_for(&connection, &params).await;
                    match runtime.extensions.dispatch(&method, params, &ctx).await {
                        Ok(result) => {
                            let _ = responder.respond_with_result(Ok(result));
                        }
                        Err(err) => {
                            tracing::warn!(method = %method, error = %err, "Extension dispatch failed");
                            let error = agent_client_protocol::Error::new(
                                err.code,
                                err.message,
                            );
                            let error = if let Some(data) = err.data {
                                error.data(data)
                            } else {
                                error
                            };
                            let _ = responder.respond_with_result(Err(error));
                        }
                    }
                    Ok(Handled::Yes)
                }
            },
            on_receive_request!(),
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

    result?;

    Ok(())
}
