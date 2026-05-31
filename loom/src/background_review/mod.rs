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
pub mod curator_backup;
pub mod skill_usage;
pub mod tools;
pub mod agent_loop;

pub mod workflow;

// Re-export key types for convenience
pub use agent_loop::{
    AgentReviewRunner, AgentReviewConfig, ReviewMode, AgentReviewResult,
    build_review_agent_client,
};
pub use memory::{MemoryStore, MemoryFile, MemoryConfig, MemoryError};
pub use skill_registry::{SkillRegistry, SkillContent, SkillMeta, SkillError, Lifecycle, Source};
pub use curator::{
    Curator, CuratorConfig, CuratorReport,
    CuratorState,          // +new: Hermes alignment + state fields
    CuratorStateStore,    // +new: storage abstraction
    FileStateStore,        // +new: production store
    MemoryStateStore,      // +new: test store
    AutoCounts,            // +new: Hermes alignment
    CuratorReviewResult,   // +new: Hermes alignment
    LlmPassResult,         // +new: Hermes alignment
    // LLM review types
    LlMReviewResult, SkillCluster, PruningDecision, ConsolidationDecision,
    ToolCall, AbsorbedIntoDeclaration, ClassificationResult, SkillSnapshot,
    CuratorRunReport,
    // Public functions
    parse_llm_review_response, build_llm_prompt, reconcile_classification,
};
pub use curator_backup::{CuratorBackup, BackupError, SnapshotMeta};
pub use tools::{ReviewToolExecutor, ReviewAction, review_tool_specs};
pub use history::{ReviewHistory, ReviewRecord};
pub use observability::ObservabilityStore;

pub use workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle, PendingReviewRegistry,
    ReviewOutputFn, spawn_background_review, wait_for_pending_reviews,
    build_background_config_from_opts,
};

#[cfg(feature = "testing")]
pub use workflow::{run_background_review_workflow, run_background_review_inner};
