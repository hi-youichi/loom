//! Common stream execution logic and checkpoint loading shared by ReAct, DUP, ToT, and GoT runners.
//!
//! - [`run_stream_with_config`]: build initial state → compiled.stream → consume events → return final state.
//! - [`load_from_checkpoint_or_build`]: try load from checkpointer, else run `build_fresh` future; merge user message when loaded.

use std::collections::HashSet;
use std::future::Future;

use futures::FutureExt;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use checkpoint::{CheckpointError, Checkpointer, RunnableConfig};
use loom_graph_core::CompiledStateGraph;
use loom_graph_core::GraphError;
use loom_llm::message::UserContent;
use stream_event::{StreamEvent, StreamMode};

/// Tries to load state from checkpointer; if found, merges `user_message` via `merge` and returns.
/// Otherwise runs `build_fresh` and returns its result. Shared by ReAct, DUP, and ToT initial state builders.
pub async fn load_from_checkpoint_or_build<S, F, M>(
    checkpointer: Option<&dyn Checkpointer<S>>,
    runnable_config: Option<&RunnableConfig>,
    user_message: &UserContent,
    build_fresh: F,
    merge: M,
) -> Result<S, CheckpointError>
where
    F: Future<Output = Result<S, CheckpointError>>,
    M: FnOnce(S, UserContent) -> S,
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
            return Ok(merge(checkpoint.channel_values, user_message.clone()));
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
    Execution(#[from] GraphError),
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

    let graph_stream = compiled.stream(initial_state, run_config, modes, cancellation);
    let mut stream = graph_stream.events;
    // Poll completion concurrently with the event stream so the consumer loop
    // terminates even if a leaked Sender keeps the channel open. After completion
    // fires, drain non-blockingly (now_or_never) to capture buffered events
    // without risking an infinite hang.
    let mut completion = graph_stream.completion;
    let mut completion_result: Option<Result<Result<(), GraphError>, tokio::task::JoinError>> =
        None;
    let mut final_state: Option<S> = None;
    let mut completion_consumed = false;

    loop {
        let event = if completion_consumed {
            stream.next().now_or_never().flatten()
        } else {
            tokio::select! {
                biased;
                res = &mut completion => {
                    completion_result = Some(res);
                    completion_consumed = true;
                    continue;
                }
                next = stream.next() => next,
            }
        };

        match event {
            Some(e) => {
                if let StreamEvent::Values(s) = e.clone() {
                    final_state = Some(s);
                }
                if let Some(ref f) = on_event {
                    f(e);
                }
            }
            None => break,
        }
    }

    let join_result = match completion_result {
        Some(res) => res,
        None => completion.await,
    };
    match join_result {
        Ok(Ok(())) => final_state
            .map(StreamRunOutcome::Finished)
            .ok_or(StreamEndedWithoutState.into()),
        Ok(Err(GraphError::Cancelled)) => Ok(StreamRunOutcome::Cancelled),
        Ok(Err(e)) => Err(StreamRunError::Execution(e)),
        Err(e) => Err(StreamRunError::Execution(GraphError::ExecutionFailed(
            format!("graph stream task failed: {}", e),
        ))),
    }
}
