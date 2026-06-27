//! LLM factory: model registry, client creation, and title generation.
//!
//! `create_llm_client` / `create_llm_provider` live in `loom_llm::factory`.
//! `LlmFactory` and `generate_title` live here (they need `loom_tier` for
//! provider config and tier resolution).

pub mod factory;
pub mod title_generator;

pub use factory::LlmFactory;
pub use loom_llm::factory::{create_llm_client, create_llm_provider};
pub use title_generator::generate_title;
