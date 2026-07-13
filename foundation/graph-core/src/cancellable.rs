//! Cancellable future execution utilities.
//!
//! Provides functionality for running futures with cancellation support.

use crate::error::GraphError;
use futures_util::future::abortable;
use tokio_util::sync::CancellationToken;

/// Runs a future with cancellation support.
///
/// This function provides two levels of cancellation:
/// 1. If a cancellation token is provided, the future can be cancelled externally
/// 2. If no cancellation token is provided, a 600-second timeout is applied
///
/// # Arguments
///
/// - `future`: The future to run
/// - `cancellation`: Optional cancellation token
///
/// # Returns
///
/// - `Ok(Ok(T))`: Future completed successfully
/// - `Ok(Err(E))`: Future returned an error
/// - `Err(GraphError::Cancelled)`: Future was cancelled or timed out
pub async fn run_cancellable<T, E>(
    future: impl std::future::Future<Output = Result<T, E>>,
    cancellation: Option<&CancellationToken>,
) -> Result<Result<T, E>, GraphError> {
    if let Some(token) = cancellation {
        let (task, _abort_handle) = abortable(future);
        tokio::select! {
            _ = token.cancelled() => Err(GraphError::Cancelled),
            r = task => match r {
                Ok(inner) => Ok(inner),
                Err(_aborted) => Err(GraphError::Cancelled),
            },
        }
    } else {
        let (task, abort_handle) = abortable(future);
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(600));
        tokio::select! {
            _ = timeout => {
                abort_handle.abort();
                Err(GraphError::Cancelled)
            }
            r = task => match r {
                Ok(inner) => Ok(inner),
                Err(_aborted) => Err(GraphError::Cancelled),
            },
        }
    }
}
