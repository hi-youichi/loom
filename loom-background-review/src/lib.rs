//! Background review system for Loom.
//!
//! This crate provides the background review functionality that analyzes
//! completed conversations and updates memory/skills. It includes:
//!
//! - **Agent Loop**: Review agent that processes conversations using LLM tools
//! - **Curator**: Skill lifecycle management and consolidation
//! - **Memory Store**: Persistent memory for user/project facts
//! - **Skill Registry**: Skill library with lifecycle management
//! - **Tool Executor**: Review tool implementations for the agent
//! - **Workflow**: Background review task orchestration

pub mod agent_loop;
pub mod curator;
pub mod curator_backup;
pub mod history;
pub mod memory;
pub mod observability;
pub mod prompts;
pub mod security;
pub mod skill_registry;
pub mod tools;
pub mod workflow;

// Re-export key types for convenience
pub use agent_loop::{
    AgentReviewRunner, AgentReviewConfig, ReviewMode, AgentReviewResult,
    build_review_agent_client,
};
pub use memory::{MemoryStore, MemoryFile, MemoryConfig, MemoryError, MemoryProvenance};
pub use skill_registry::{SkillRegistry, SkillContent, SkillMeta, SkillError, Lifecycle, Source};
pub use loom_skill::{SkillUsage, SkillUsageStore};
pub use curator::{
    Curator, CuratorConfig, CuratorReport,
    CuratorState, CuratorStateStore, FileStateStore, MemoryStateStore,
    AutoCounts, CuratorReviewResult, LlmPassResult,
    LlMReviewResult, SkillCluster, PruningDecision, ConsolidationDecision,
    CuratorToolCall, AbsorbedIntoDeclaration, ClassificationResult,
    SkillSnapshot, CuratorRunReport,
    parse_llm_review_response, build_llm_prompt, reconcile_classification,
};
pub use curator_backup::{CuratorBackup, BackupError, SnapshotMeta};
pub use tools::{ReviewToolExecutor, ReviewAction, review_tool_specs};
pub use history::{ReviewHistory, ReviewRecord};
pub use observability::ObservabilityStore;

pub use workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle, PendingReviewRegistry,
    ReviewOutputFn, BackgroundReviewCallbacks, spawn_background_review, wait_for_pending_reviews,
    run_background_review_workflow, run_background_review_inner,
    build_background_config_from_opts_ext,
};
