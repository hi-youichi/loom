//! Run configuration summary types for logging and verbose output.
//!
//! Used by CLI or other callers to aggregate LLM, memory, tools, and embedding
//! config into a single summary that can be printed (e.g. to stderr when `--verbose`).

pub mod summary;

pub use summary::{
    build_config_summary, ConfigSection, EmbeddingConfigSummary, LlmConfigSummary,
    MemoryConfigSummary, RunConfigSummary, RunConfigSummarySource, ToolConfigSummary,
};
