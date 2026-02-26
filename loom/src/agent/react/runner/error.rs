//! Error type for ReAct runner (run_agent, run_react_graph_stream).

use crate::error::AgentError;
use crate::graph::CompilationError;
use crate::memory::CheckpointError;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] AgentError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

impl From<std::io::Error> for RunError {
    fn from(e: std::io::Error) -> Self {
        RunError::Execution(AgentError::ExecutionFailed(e.to_string()))
    }
}
