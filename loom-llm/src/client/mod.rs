//! Client implementations for loom-llm.

mod openai_compat;

// Only export ChatOpenAICompat for now
// MockLlm and other test utilities are in loom's own module
pub use openai_compat::ChatOpenAICompat;