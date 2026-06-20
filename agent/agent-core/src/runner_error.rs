use loom_graph::CompilationError;
use loom_graph::GraphError;
use loom_memory::CheckpointError;

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] GraphError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

impl From<std::io::Error> for RunnerError {
    fn from(e: std::io::Error) -> Self {
        RunnerError::Execution(GraphError::ExecutionFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts_to_execution_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let run_err: RunnerError = io_err.into();
        match run_err {
            RunnerError::Execution(e) => assert!(e.to_string().contains("pipe broke")),
            other => panic!("expected Execution, got {:?}", other),
        }
    }

    #[test]
    fn display_stream_ended_without_state() {
        let err = RunnerError::StreamEndedWithoutState;
        assert_eq!(err.to_string(), "stream ended without final state");
    }
}
