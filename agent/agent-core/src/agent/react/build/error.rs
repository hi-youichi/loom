//! Error type when building a ReactRunner from config.

use anureo_graph_core::CompilationError;
use anureo_graph_core::GraphError;

/// Error when building a ReactRunner from config.
#[derive(Debug, thiserror::Error)]
pub enum BuildRunnerError {
    #[error("failed to build run context: {0}")]
    Context(#[from] GraphError),
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("no LLM provided and config has no openai_api_key/model; pass Some(llm) or set OPENAI_API_KEY and OPENAI_MODEL")]
    NoLlm,
}
