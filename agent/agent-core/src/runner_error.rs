use checkpoint::CheckpointError;
use loom_graph_core::CompilationError;
use loom_graph_core::GraphError;

use loom_llm::error::LlmError;
use model_spec_core::error::ProviderError;

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] GraphError),
    /// LLM 结构化 provider 错误（保留分类与重试决策，供 UI/提示消费）。
    #[error("LLM provider error: {0}")]
    Llm(ProviderError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

impl From<std::io::Error> for RunnerError {
    fn from(e: std::io::Error) -> Self {
        RunnerError::Execution(GraphError::ExecutionFailed(e.to_string()))
    }
}

impl From<LlmError> for RunnerError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::Provider(err) => RunnerError::Llm(*err),
            other => RunnerError::Execution(GraphError::ExecutionFailed(other.to_string())),
        }
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

    #[test]
    fn llm_provider_error_keeps_structure() {
        let err = LlmError::Provider(Box::new(ProviderError {
            provider_id: "zhipuai".to_string(),
            kind: model_spec_core::error::ErrorKind::Billing,
            status: 429,
            code: Some("1113".to_string()),
            message: "arrears".to_string(),
            user_message: "账户余额不足，请充值后重试".to_string(),
            retry_policy: model_spec_core::error::RetryPolicy::NoRetry {
                action: model_spec_core::error::UserAction::TopUp,
            },
            request_id: None,
            partial_tokens: false,
        }));
        let run_err: RunnerError = err.into();
        match run_err {
            RunnerError::Llm(pe) => assert_eq!(pe.kind, model_spec_core::error::ErrorKind::Billing),
            other => panic!("expected Llm, got {:?}", other),
        }
    }
}
