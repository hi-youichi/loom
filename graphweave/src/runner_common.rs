//! Common stream execution logic shared by ReAct, DUP, ToT, and GoT runners.
//!
//! Extracts the repeated pattern: build initial state → compiled.stream → consume
//! events → return final state from last `StreamEvent::Values`.

use std::collections::HashSet;

use tokio_stream::StreamExt;

use crate::graph::CompiledStateGraph;
use crate::memory::RunnableConfig;
use crate::stream::{StreamEvent, StreamMode};

/// Error when the stream ends without producing a final `Values` state.
#[derive(Debug, thiserror::Error)]
#[error("stream ended without final state")]
pub struct StreamEndedWithoutState;

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
) -> Result<S, StreamEndedWithoutState>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: FnMut(StreamEvent<S>),
{
    let modes = HashSet::from([
        StreamMode::Messages,
        StreamMode::Tasks,
        StreamMode::Updates,
        StreamMode::Values,
        StreamMode::Custom,
    ]);
    let mut stream = compiled.stream(initial_state, run_config, modes);
    let mut final_state: Option<S> = None;
    while let Some(event) = stream.next().await {
        if let Some(ref mut f) = on_event {
            f(event.clone());
        }
        if let StreamEvent::Values(s) = event {
            final_state = Some(s);
        }
    }
    final_state.ok_or(StreamEndedWithoutState)
}
