//! Common stream execution logic and checkpoint loading shared by ReAct, DUP, ToT, and GoT runners.
//!
//! - [`run_stream_with_config`]: build initial state → compiled.stream → consume events → return final state.
//! - [`load_from_checkpoint_or_build`]: try load from checkpointer, else run `build_fresh` future; merge user message when loaded.

use std::collections::HashSet;
use std::future::Future;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::cli_run::RunCancellation;
use crate::error::AgentError;
use crate::graph::CompiledStateGraph;
use crate::memory::{CheckpointError, Checkpointer, RunnableConfig};
use crate::stream::{StreamEvent, StreamMode};

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
        let tuple = cp.get_tuple(config).await?;
        if let Some((checkpoint, _)) = tuple {
            return Ok(merge(checkpoint.channel_values, user_message.to_string()));
        }
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
    mut on_event: Option<F>,
    cancellation: Option<CancellationToken>,
    run_cancellation: Option<RunCancellation>,
) -> Result<StreamRunOutcome<S>, StreamRunError>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: FnMut(StreamEvent<S>),
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
    let graph_stream = compiled.stream(
        initial_state,
        run_config,
        modes,
        cancellation,
        run_cancellation,
    );
    let mut stream = graph_stream.events;
    let mut final_state: Option<S> = None;
    while let Some(event) = stream.next().await {
        if let Some(ref mut f) = on_event {
            f(event.clone());
        }
        if let StreamEvent::Values(s) = event {
            final_state = Some(s);
        }
    }
    let completion = graph_stream
        .completion
        .await
        .map_err(|e| StreamRunError::Execution(AgentError::ExecutionFailed(format!(
            "graph stream task failed: {}",
            e
        ))))?;
    match completion {
        Ok(()) => final_state
            .map(StreamRunOutcome::Finished)
            .ok_or(StreamEndedWithoutState.into()),
        Err(AgentError::Cancelled) => Ok(StreamRunOutcome::Cancelled),
        Err(e) => Err(StreamRunError::Execution(e)),
    }
}
