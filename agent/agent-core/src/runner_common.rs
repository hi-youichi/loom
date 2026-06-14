//! Common stream execution logic and checkpoint loading shared by ReAct, DUP, ToT, and GoT runners.
//!
//! - [`run_stream_with_config`]: build initial state → compiled.stream → consume events → return final state.
//! - [`load_from_checkpoint_or_build`]: try load from checkpointer, else run `build_fresh` future; merge user message when loaded.

use std::future::Future;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use loom_llm::error::AgentError;
use loom_graph::CompiledStateGraph;
use loom_memory::{CheckpointError, Checkpointer, RunnableConfig};
use loom_stream::{StreamEvent, StreamMode};

/// Tries to load state from checkpointer; if found, merges `user_message` via `merge` and returns.
/// Otherwise runs `build_fresh` and returns its result. Shared by ReAct, DUP, and ToT initial state builders.
pub async fn load_from_checkpoint_or_build<S, F, M>(
    checkpointer: Option<&dyn Checkpointer<S>>,
    runnable_config: Option<&RunnableConfig>,
    user_message: &str,
    build_fresh: F,
    merge: M,
) -> Result<S, CheckpointError>
where
    F: Future<Output = Result<S, CheckpointError>>,
    M: FnOnce(S, String) -> S,
    S: Clone + Send + Sync + 'static,
{
    let load_from_checkpoint =
        checkpointer.is_some() && runnable_config.and_then(|c| c.thread_id.as_ref()).is_some();

    if load_from_checkpoint {
        let cp = checkpointer.expect("checkpointer is Some");
        let config = runnable_config.expect("runnable_config is Some");
        tracing::debug!(
            thread_id = ?config.thread_id,
            "load_from_checkpoint_or_build: attempting to load checkpoint"
        );
        let tuple = cp.get_tuple(config).await?;
        if let Some((checkpoint, _)) = tuple {
            tracing::debug!("checkpoint found, merging user message");
            let merged = merge(checkpoint.channel_values.clone(), user_message.to_string());
            return Ok(merged);
        }
    }

    tracing::debug!("no checkpoint found, building fresh state");
    build_fresh.await
}

/// Unified result type for stream-based agent runs.
#[derive(Debug, Clone)]
pub enum StreamRunOutcome<S = (), E = ()> {
    /// Agent ran to completion; `S` is the final agent state.
    Completed(S),
    /// Agent was cancelled externally (e.g. user pressed Ctrl+C).
    Cancelled,
    /// Agent hit a fatal error; `E` is the error type.
    Error(E),
    /// Stream produced no events and never reached a final state.
    Empty,
}

/// Error types for stream execution.
#[derive(Debug, Clone)]
pub enum StreamRunError {
    Execution(AgentError),
}

impl std::fmt::Display for StreamRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamRunError::Execution(e) => write!(f, "{}", e),
        }
    }
}

/// Runs the graph stream with config-driven cancellation support.
///
/// Returns the final state if the stream ends normally,
/// or `StreamRunOutcome::Cancelled` if cancelled.
pub async fn run_stream_with_config<S, F>(
    compiled: CompiledStateGraph<S>,
    state: S,
    run_config: Option<RunnableConfig>,
    on_event: Option<F>,
    cancel_token: Option<CancellationToken>,
    _event_forwarder: Option<std::sync::Arc<dyn Fn(StreamEvent<S>) + Send + Sync>>,
) -> Result<StreamRunOutcome<S, StreamRunError>, StreamRunError>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: FnMut(StreamEvent<S>) + Send + 'static,
{
    let cancel_token = cancel_token.unwrap_or_default();
    let mut on_event = on_event;
    let stream = compiled.stream(
        state,
        run_config,
        [StreamMode::Values, StreamMode::Messages, StreamMode::Updates, StreamMode::Tasks, StreamMode::Tools],
        None,
        None,
    );
    let mut final_state: Option<S> = None;
    let error_state: Option<StreamRunError> = None;

    tokio::pin!(stream);

    loop {
        tokio::select! {
            biased;

            _ = cancel_token.cancelled() => {
                tracing::debug!("run_stream_with_config: cancelled");
                return Ok(StreamRunOutcome::Cancelled);
            }

            event = stream.events.next() => {
                match event {
                    Some(ev) => {
                        if let Some(ref mut cb) = on_event {
                            cb(ev.clone());
                        }
                        if let StreamEvent::Values(ref state) = ev {
                            final_state = Some(state.clone());
                        }
                    }
                    None => {
                        if let Some(err) = error_state {
                            return Err(err);
                        }
                        if let Some(state) = final_state {
                            return Ok(StreamRunOutcome::Completed(state));
                        }
                        tracing::warn!("stream ended without final state");
                        return Ok(StreamRunOutcome::Empty);
                    }
                }
            }
        }
    }
}