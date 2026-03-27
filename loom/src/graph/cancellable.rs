//! Abortable futures with graph cancellation and run-level active-operation tracking.

use futures_util::future::{abortable, Aborted};
use tokio_util::sync::CancellationToken;

use crate::cli_run::{ActiveOperationKind, RunCancellation};
use crate::error::AgentError;

/// Runs `future` as an abortable task, registers it with `run_cancellation` when set,
/// and races against `cancellation` when set.
///
/// Returns `Ok(Ok(value))` when the inner future completes successfully, `Ok(Err(e))` when
/// the inner future returns an error, and `Err(AgentError::Cancelled)` when the graph
/// token fires or the task is aborted.
pub async fn run_cancellable<T, E>(
    future: impl std::future::Future<Output = Result<T, E>>,
    cancellation: Option<&CancellationToken>,
    run_cancellation: Option<&RunCancellation>,
    op_kind: ActiveOperationKind,
) -> Result<Result<T, E>, AgentError> {
    let (task, abort_handle) = abortable(future);
    if let Some(rc) = run_cancellation {
        rc.set_abortable_operation(op_kind, abort_handle);
    }

    let outcome = if let Some(token) = cancellation {
        tokio::select! {
            _ = token.cancelled() => {
                if let Some(rc) = run_cancellation {
                    rc.cancel_active_operation();
                }
                return Err(AgentError::Cancelled);
            }
            r = task => r,
        }
    } else {
        task.await
    };

    if let Some(rc) = run_cancellation {
        rc.clear_active_operation();
    }

    match outcome {
        Ok(inner) => Ok(inner),
        Err(Aborted) => Err(AgentError::Cancelled),
    }
}
