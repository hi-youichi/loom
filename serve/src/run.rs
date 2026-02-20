//! Handle `Run` request: execute agent (streaming or single reply).

use axum::extract::ws::WebSocket;
use loom::{
    run_agent, AnyStreamEvent, AgentType, EnvelopeState, ErrorResponse, RunCmd, RunEndResponse,
    RunOptions, RunStreamEventResponse, ServerResponse,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::response::send_response;

/// Bounded buffer size for run stream events. Prevents unbounded memory growth when
/// the websocket sender cannot keep up with the agent.
const EVENT_QUEUE_CAPACITY: usize = 128;

/// Returns `Some(response)` when a single response should be sent by the caller;
/// `None` when we already sent (streaming case).
pub(crate) async fn handle_run(
    r: loom::RunRequest,
    socket: &mut WebSocket,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>> {
    let id = r.id.clone();
    let opts = RunOptions {
        message: r.message,
        working_folder: r.working_folder.map(PathBuf::from),
        thread_id: r.thread_id,
        verbose: r.verbose.unwrap_or(false),
        got_adaptive: r.got_adaptive.unwrap_or(false),
        display_max_len: 2000,
        output_json: true,
    };
    let cmd = match r.agent {
        AgentType::React => RunCmd::React,
        AgentType::Dup => RunCmd::Dup,
        AgentType::Tot => RunCmd::Tot,
        AgentType::Got => RunCmd::Got {
            got_adaptive: opts.got_adaptive,
        },
    };

    let session_id = format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let (tx, mut rx) = mpsc::channel::<serde_json::Value>(EVENT_QUEUE_CAPACITY);
    let opts = opts.clone();
    let cmd = cmd.clone();
    let id_run = id.clone();
    let run_handle = tokio::spawn(async move {
        let state = Arc::new(Mutex::new(EnvelopeState::new(session_id)));
        let state_clone = state.clone();
        let on_event = Box::new(move |ev: AnyStreamEvent| {
            let v = match state_clone.lock() {
                Ok(mut s) => ev.to_protocol_format(&mut *s),
                Err(_) => return,
            };
            let v = match v {
                Ok(x) => x,
                Err(_) => return,
            };
            if let Err(e) = tx.try_send(v) {
                tracing::warn!(
                    "event queue full, dropping stream event (receiver likely disconnected): {:?}",
                    e
                );
            }
        });
        let result = run_agent(&opts, &cmd, Some(on_event)).await;
        (result, state)
    });

    let mut send_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    while let Some(event) = rx.recv().await {
        if let Err(e) = send_response(
            socket,
            &ServerResponse::RunStreamEvent(RunStreamEventResponse {
                id: id.clone(),
                event,
            }),
        )
        .await
        {
            send_err = Some(e);
            break;
        }
    }

    if let Some(e) = send_err {
        run_handle.abort();
        let _ = run_handle.await;
        return Err(e);
    }

    let (result, state) = run_handle
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    match result {
        Ok(reply) => {
            let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
            let (session_id, node_id, event_id) = reply_env
                .as_ref()
                .map(|e| {
                    (
                        e.session_id.clone(),
                        e.node_id.clone(),
                        e.event_id,
                    )
                })
                .unwrap_or((None, None, None));
            send_response(
                socket,
                &ServerResponse::RunEnd(RunEndResponse {
                    id: id_run,
                    reply,
                    usage: None,
                    total_usage: None,
                    session_id,
                    node_id,
                    event_id,
                }),
            )
            .await?;
        }
        Err(e) => {
            send_response(socket, &ServerResponse::Error(ErrorResponse {
                id: Some(id_run),
                error: e.to_string(),
            }))
            .await?;
        }
    }
    Ok(None)
}
