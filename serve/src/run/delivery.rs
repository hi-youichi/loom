//! Delivering run stream to the client: RunStreamSender abstraction and handle_run_stream.

use async_trait::async_trait;
use axum::extract::ws::Message;
use futures::SinkExt;
use loom_protocol::{EnvelopeState, ErrorResponse, ProtocolEventEnvelope, RunEndResponse, RunStreamEventResponse, ServerResponse};
use loom::agent_run::{RunCompletion, RunError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::mpsc;

use crate::connection::SharedSink;

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

pub(super) struct WebSocketRunSender(pub(super) SharedSink);

#[async_trait]
impl RunStreamSender for WebSocketRunSender {
    async fn send_response(
        &mut self,
        response: &ServerResponse,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string(response).unwrap_or_else(|_| {
            serde_json::to_string(&ServerResponse::Error(ErrorResponse {
                id: None,
                error: "serialization error".to_string(),
            }))
            .unwrap()
        });
        let mut s = self.0.lock().await;
        s.send(Message::Text(json)).await?;
        Ok(())
    }
}

/// Result of the run task (result, state, dropped_events, dropped_appends).
pub(super) type RunTaskResult = (
    Result<RunCompletion, RunError>,
    Arc<StdMutex<EnvelopeState>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
);

/// Consumes the event stream from the run task: for each event sends RunStreamEvent via
/// `sender`, then awaits the run task. On success, sends RunEnd or Error. Logs when
/// events or appends were dropped.
pub(super) async fn handle_run_stream<S>(
    run_id: String,
    request_id: Option<String>,
    mut rx: mpsc::Receiver<ProtocolEventEnvelope>,
    run_handle: tokio::task::JoinHandle<RunTaskResult>,
    cancellation: Option<loom_cli_types::RunCancellation>,
    sender: &mut S,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>>
where
    S: RunStreamSender,
{
    tracing::info!("📡 Starting stream delivery for run: {}", run_id);
    let mut event_count = 0;
    let mut send_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;

    while let Some(event) = rx.recv().await {
        event_count += 1;
        tracing::debug!("📨 Sending event #{} for run: {}", event_count, run_id);

        if let Err(e) = sender
            .send_response(&ServerResponse::RunStreamEvent(RunStreamEventResponse {
                id: run_id.clone(),
                request_id: request_id.clone(),
                event,
            }))
            .await
        {
            tracing::error!(
                "❌ Failed to send event #{} for run {}: {}",
                event_count,
                run_id,
                e
            );
            send_err = Some(e);
            break;
        }
    }

    tracing::info!(
        "✅ Stream delivery complete for run: {} (sent {} events)",
        run_id,
        event_count
    );

    if let Some(e) = send_err {
        tracing::warn!("⚠️  Stream delivery failed, cancelling run: {}", run_id);
        // Try graceful cancellation first; fall back to abort after a grace period.
        let mut run_handle = run_handle;
        if let Some(ref cancellation) = cancellation {
            cancellation.cancel();
            tokio::select! {
                _ = &mut run_handle => {
                    tracing::info!("Run {} cancelled gracefully", run_id);
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                    tracing::warn!("⚠️  Run {} did not finish in 5s after cancel, aborting", run_id);
                    run_handle.abort();
                    let _ = run_handle.await;
                }
            }
        } else {
            run_handle.abort();
            let _ = run_handle.await;
        }
        return Err(e);
    }

    tracing::info!("⏳ Waiting for run task completion: {}", run_id);
    let (result, state, dropped_events, dropped_appends) = run_handle.await.map_err(|e| {
        tracing::error!("❌ Run task failed for {}: {:?}", run_id, e);
        Box::new(e) as Box<dyn std::error::Error + Send + Sync>
    })?;

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
        Ok(RunCompletion::Finished(result)) => {
            tracing::info!("✅ Run completed successfully: {}", run_id);
            let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
            let (session_id, node_id, event_id) = reply_env
                .as_ref()
                .map(|e| (e.session_id.clone(), e.node_id.clone(), e.event_id))
                .unwrap_or((None, None, None));

            tracing::debug!("📤 Sending RunEnd response for: {}", run_id);
            sender
                .send_response(&ServerResponse::RunEnd(RunEndResponse {
                    id: run_id.clone(),
                    request_id: request_id.clone(),
                    reply: result.reply,
                    reasoning_content: result.reasoning_content,
                    usage: None,
                    total_usage: None,
                    session_id,
                    node_id,
                    event_id,
                }))
                .await?;
        }
        Ok(RunCompletion::Cancelled) => {
            tracing::warn!("⚠️  Run cancelled: {}", run_id);
            sender
                .send_response(&ServerResponse::Error(ErrorResponse {
                    id: Some(run_id.clone()),
                    error: "run cancelled".to_string(),
                }))
                .await?;
        }
        Ok(RunCompletion::Error(e)) => {
            tracing::error!("❌ Run {} errored: {}", run_id, e.0);
            sender
                .send_response(&ServerResponse::Error(ErrorResponse {
                    id: Some(run_id.clone()),
                    error: e.0,
                }))
                .await?;
        }
        Err(e) => {
            tracing::error!("❌ Run {} failed with error: {}", run_id, e);
            sender
                .send_response(&ServerResponse::Error(ErrorResponse {
                    id: Some(run_id.clone()),
                    error: e.to_string(),
                }))
                .await?;
        }
    }

    tracing::info!("🎉 Run {} fully processed and response sent", run_id);
    Ok(None)
}
