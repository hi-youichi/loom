//! Background review system for Loom.
//!
//! Analyzes completed conversations and updates memory/skills.

pub mod memory;
pub mod skill_registry;
pub mod security;
pub mod prompts;
pub mod history;
pub mod observability;
pub mod curator;
pub mod tools;
pub mod agent_loop;
pub mod evolution;
pub mod workflow;

// Re-export key types for convenience
pub use agent_loop::{
    AgentReviewRunner, AgentReviewConfig, ReviewMode, AgentReviewResult,
    build_review_agent_client,
};
pub use memory::{MemoryStore, MemoryFile, MemoryConfig, MemoryError};
pub use skill_registry::{SkillRegistry, SkillContent, SkillMeta, SkillError, Lifecycle, Source};
pub use curator::{Curator, CuratorConfig, CuratorReport};
pub use tools::{ReviewToolExecutor, ReviewAction, review_tool_specs};
pub use history::{ReviewHistory, ReviewRecord};
pub use observability::ObservabilityStore;
pub use evolution::{EvolutionTriggerConfig, EvolutionOutcome};
pub use workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle, PendingReviewRegistry,
    ReviewOutputFn, spawn_background_review, wait_for_pending_reviews,
    build_background_config_from_opts,
};
