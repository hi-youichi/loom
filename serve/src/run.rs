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

/// Returns `Some(response)` when a single response should be sent by the caller;
/// `None` when we already sent (streaming case).
pub(crate) async fn handle_run(
    r: loom::RunRequest,
    socket: &mut WebSocket,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>> {
    let id = r.id.clone();
    let output_json = r.output_json == Some(true);
    let opts = RunOptions {
        message: r.message,
        working_folder: r.working_folder.map(PathBuf::from),
        thread_id: r.thread_id,
        verbose: r.verbose.unwrap_or(false),
        got_adaptive: r.got_adaptive.unwrap_or(false),
        display_max_len: 2000,
        output_json,
    };
    let cmd = match r.agent {
        AgentType::React => RunCmd::React,
        AgentType::Dup => RunCmd::Dup,
        AgentType::Tot => RunCmd::Tot,
        AgentType::Got => RunCmd::Got {
            got_adaptive: opts.got_adaptive,
        },
    };

    if output_json {
        let session_id = format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (tx, mut rx) = mpsc::unbounded_channel::<serde_json::Value>();
        let opts = opts.clone();
        let cmd = cmd.clone();
        let id_run = id.clone();
        let run_handle = tokio::spawn(async move {
            let state = Arc::new(Mutex::new(EnvelopeState::new(session_id)));
            let state_clone = state.clone();
            let on_event = Box::new(move |ev: AnyStreamEvent| {
                let mut v = match ev.to_protocol_format() {
                    Ok(x) => x,
                    Err(_) => return,
                };
                if let Ok(mut s) = state_clone.lock() {
                    s.inject_into(&mut v);
                }
                let _ = tx.send(v);
            });
            let result = run_agent(&opts, &cmd, Some(on_event)).await;
            (result, state)
        });
        while let Some(event) = rx.recv().await {
            send_response(
                socket,
                &ServerResponse::RunStreamEvent(RunStreamEventResponse {
                    id: id.clone(),
                    event,
                }),
            )
            .await?;
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
        return Ok(None);
    }

    let result = run_agent(&opts, &cmd, None).await;
    Ok(Some(match result {
        Ok(reply) => ServerResponse::RunEnd(RunEndResponse {
            id,
            reply,
            usage: None,
            total_usage: None,
            session_id: None,
            node_id: None,
            event_id: None,
        }),
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }))
}
