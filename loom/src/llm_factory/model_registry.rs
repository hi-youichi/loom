//! Re-export factory functions from `loom-llm`.
//!
//! `create_llm_client` and `create_llm_provider` live in `loom_llm::factory`
//! so that both `agent-core` (L3) and `loom` (L4) can use them.

pub use loom_llm::factory::{create_llm_client, create_llm_provider};
