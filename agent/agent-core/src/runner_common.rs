//! Common stream execution logic and checkpoint loading shared by ReAct, DUP, ToT, and GoT runners.
//!
//! - [`run_stream_with_config`]: build initial state → compiled.stream → consume events → return final state.
//! - [`load_from_checkpoint_or_build`]: try load from checkpointer, else run `build_fresh` future; merge user message when loaded.

use std::collections::HashSet;
use std::future::Future;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use loom_graph::CompiledStateGraph;
use loom_llm::error::AgentError;
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
            tracing::info!(
                thread_id = ?runnable_config.expect("runnable_config is Some").thread_id,
                "load_from_checkpoint_or_build: checkpoint found, merging user message"
            );
            return Ok(merge(checkpoint.channel_values, user_message.to_string()));
        }
        tracing::info!("load_from_checkpoint_or_build: no checkpoint found, building fresh state");
    }

    build_fresh.await
}

/// Error when the stream ends without producing a final `Values` state.
#[derive(Debug, thiserror::Error)]
#[error("stream ended without final state")]
pub struct StreamEndedWithoutState;

/// Final outcome of a stream run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamRunOutcome<S> {
    Finished(S),
    Cancelled,
}

/// Error when stream execution fails for reasons other than cancellation.
#[derive(Debug, thiserror::Error)]
pub enum StreamRunError {
    #[error(transparent)]
    Execution(#[from] AgentError),
    #[error(transparent)]
    StreamEndedWithoutState(#[from] StreamEndedWithoutState),
}

/// Runs the compiled graph in streaming mode, consuming events and returning the final state.
///
/// Uses fixed stream modes (Messages, Tasks, Updates, Values, Custom). When `on_event`
/// is provided, invokes it for each `StreamEvent`. Returns the state from the last
/// `StreamEvent::Values` in the stream.
pub async fn run_stream_with_config<S, F>(
    compiled: &CompiledStateGraph<S>,
    initial_state: S,
    run_config: Option<RunnableConfig>,
    on_event: Option<F>,
    cancellation: Option<CancellationToken>,
) -> Result<StreamRunOutcome<S>, StreamRunError>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(StreamEvent<S>) + Clone + Send + 'static,
{
    let modes = HashSet::from([
        StreamMode::Messages,
        StreamMode::Tasks,
        StreamMode::Tools,
        StreamMode::Updates,
        StreamMode::Values,
        StreamMode::Custom,
        StreamMode::Checkpoints,
    ]);

    let _has_cancellation = cancellation.is_some();
    let graph_stream = compiled.stream(
        initial_state,
        run_config,
        modes,
        cancellation,
    );
    let mut stream = graph_stream.events;
    // Take the completion handle out so we can poll it together with the event stream.
    // If the producer task finishes before the channel closes (e.g. due to a leaked
    // Sender clone in a node), we still break out of the consumer loop instead of
    // waiting the full 600s timeout.
    let mut completion = graph_stream.completion;
    // Tracks the resolved producer task result if/when it completes during the
    // consumer loop. We can't re-await a JoinHandle after it resolves (it panics),
    // so we capture its result the first time `tokio::select!` fires for it and
    // reuse it after the loop exits.
    let mut completion_result: Option<Result<Result<(), AgentError>, tokio::task::JoinError>> = None;
    let mut final_state: Option<S> = None;
    let mut iters: u64 = 0;
    loop {
        // --- hang probe: pre-next (producer side) ---
        let poll_start = std::time::Instant::now();
        // Poll the event stream and the producer completion concurrently. Whichever
        // resolves first wins. This makes the consumer loop terminate when the
        // producer task is finished, even if the mpsc channel happens to be held
        // open by a leaked Sender clone.
        let event: Option<StreamEvent<S>> = tokio::select! {
            biased;
            // Producer completion is checked first. If the producer is done, we can
            // safely break out of the loop regardless of channel state.
            res = &mut completion => {
                completion_result = Some(res);
                None
            }
            next = stream.next() => next,
        };

        let event = match event {
            Some(e) => e,
            None => break,
        };

        let poll_elapsed_ms = poll_start.elapsed().as_millis() as u64;
        if poll_elapsed_ms > 1_000 {
            tracing::debug!(
                poll_elapsed_ms,
                iters,
                "run_stream: slow event poll"
            );
        }

        iters += 1;

        // --- event processing (consumer side) ---
        let proc_start = std::time::Instant::now();
        if let StreamEvent::Values(s) = event.clone() {
            final_state = Some(s);
        }
        if let Some(ref f) = on_event {
            f(event);
        }
        let proc_elapsed_ms = proc_start.elapsed().as_millis() as u64;
        if proc_elapsed_ms > 1_000 {
            tracing::debug!(
                proc_elapsed_ms,
                iters,
                "run_stream: slow event processing"
            );
        }
    }
    tracing::debug!("finish");
    // The completion future has already been polled in the loop above. We must not
    // await it again here because that would panic (JoinHandle polled after
    // completion). Instead, if the producer completion was captured in the loop,
    // reuse it; otherwise the loop exited via stream.next() returning None (the
    // channel closed cleanly), and we still need to wait for the producer task to
    // finish so we surface any error.
    let join_result = match completion_result {
        Some(res) => res,
        None => completion.await,
    };
    match join_result {
        Ok(Ok(())) => final_state
            .map(StreamRunOutcome::Finished)
            .ok_or(StreamEndedWithoutState.into()),
        Ok(Err(AgentError::Cancelled)) => Ok(StreamRunOutcome::Cancelled),
        Ok(Err(e)) => Err(StreamRunError::Execution(e)),
        Err(e) => Err(StreamRunError::Execution(AgentError::ExecutionFailed(
            format!("graph stream task failed: {}", e),
        ))),
    }
}
