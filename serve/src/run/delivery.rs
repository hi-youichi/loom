//! Delivering run stream to the client: RunStreamSender abstraction and handle_run_stream.

use async_trait::async_trait;
use axum::extract::ws::WebSocket;
use loom::{
    EnvelopeState, ErrorResponse, ProtocolEventEnvelope, RunEndResponse, RunError,
    RunStreamEventResponse, ServerResponse,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::response::send_response;

/// Abstraction for sending run-related server responses (RunStreamEvent, RunEnd, Error).
#[async_trait]
pub(crate) trait RunStreamSender: Send {
    /// Serializes and sends one response. Failure (e.g. connection closed) is returned
    /// so the caller can abort the run task and stop streaming.
    async fn send_response(
        &mut self,
        response: &ServerResponse,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Wraps the WebSocket in [`RunStreamSender`] so stream handling can be tested with a mock.
pub(super) struct WebSocketRunSender<'a>(pub(super) &'a mut WebSocket);

#[async_trait]
impl RunStreamSender for WebSocketRunSender<'_> {
    async fn send_response(
        &mut self,
        response: &ServerResponse,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        send_response(self.0, response).await
    }
}

/// Result of the run task (result, state, dropped_events, dropped_appends).
pub(super) type RunTaskResult = (
    Result<String, RunError>,
    Arc<Mutex<EnvelopeState>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
);

/// Consumes the event stream from the run task: for each event sends RunStreamEvent via
/// `sender`, then awaits the run task. On success, sends RunEnd or Error. Logs when
/// events or appends were dropped.
pub(super) async fn handle_run_stream<S>(
    run_id: String,
    mut rx: mpsc::Receiver<ProtocolEventEnvelope>,
    run_handle: tokio::task::JoinHandle<RunTaskResult>,
    sender: &mut S,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>>
where
    S: RunStreamSender,
{
    let mut send_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    while let Some(event) = rx.recv().await {
        if let Err(e) = sender
            .send_response(&ServerResponse::RunStreamEvent(RunStreamEventResponse {
                id: run_id.clone(),
                event,
            }))
            .await
        {
            send_err = Some(e);
            break;
        }
    }

    if let Some(e) = send_err {
        // Client disconnected or send failed; abort the agent task. Graceful cancellation would
        // require loom to accept a CancellationToken so the runner can stop mid-run.
        run_handle.abort();
        let _ = run_handle.await;
        return Err(e);
    }

    let (result, state, dropped_events, dropped_appends) = run_handle
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    let de = dropped_events.load(Ordering::Relaxed);
    let da = dropped_appends.load(Ordering::Relaxed);
    if de > 0 || da > 0 {
        tracing::warn!(
            run_id = %run_id,
            dropped_events = de,
            dropped_appends = da,
            "run completed with dropped events or appends (slow client or full queue)"
        );
    }

    match result {
        Ok(reply) => {
            let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
            let (session_id, node_id, event_id) = reply_env
                .as_ref()
                .map(|e| (e.session_id.clone(), e.node_id.clone(), e.event_id))
                .unwrap_or((None, None, None));
            sender
                .send_response(&ServerResponse::RunEnd(RunEndResponse {
                    id: run_id.clone(),
                    reply,
                    usage: None,
                    total_usage: None,
                    session_id,
                    node_id,
                    event_id,
                }))
                .await?;
        }
        Err(e) => {
            sender
                .send_response(&ServerResponse::Error(ErrorResponse {
                    id: Some(run_id.clone()),
                    error: e.to_string(),
                }))
                .await?;
        }
    }
    Ok(None)
}
