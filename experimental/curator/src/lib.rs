//! Background review system for Loom.
//!
//! This crate provides the background review functionality that analyzes
//! completed conversations and updates memory/skills. It includes:
//!
//! - **Review**: ReAct-based agent fork that processes conversations using LLM tools
//! - **Tool Gate**: Whitelist filter for review-time tool access
//! - **Curator**: Skill lifecycle management and consolidation
//! - **Prompts**: Detailed review prompts aligned with Hermes
//! - **History / Observability**: Review record persistence and metrics
//! - **Workflow**: Global task registry and Curator auto-run

pub mod backfill_triggers;
pub mod curator;
pub mod curator_backup;
pub mod curator_llm;
pub mod history;
pub mod observability;
pub mod prompts;
pub mod review;
pub mod review_tool_gate;
pub mod security;
pub mod skill_registry;
pub mod workflow;

// Re-export key types for convenience
pub use review::{
    REVIEW_INSTRUCTION, ReviewActionSummary, ReviewConfig, ReviewOutcome, TokenUsageSummary,
    run_review, spawn_background_review, spawn_review_after_session,
};
pub use review_tool_gate::{ReviewToolGate, REVIEW_ALLOWED_TOOLS};

pub use skill_registry::{SkillRegistry, SkillContent, SkillMeta, SkillError, Lifecycle, Source};
pub use skill::{SkillUsage, SkillUsageStore};
pub use skill::sync::{
    sync_skills, BundledManifest, SyncResult, compute_hash, compute_content_hash,
    compute_dir_hash,
};
pub use curator::{
    Curator, CuratorConfig, CuratorReport,
    CuratorState, CuratorStateStore, FileStateStore, MemoryStateStore,
    AutoCounts, CuratorReviewResult, LlmPassResult,
    LlMReviewResult, SkillCluster, PruningDecision, ConsolidationDecision,
    CuratorToolCall, AbsorbedIntoDeclaration, ClassificationResult,
    SkillSnapshot, StateTransition, CuratorRunReport,
    parse_llm_review_response, build_llm_prompt, reconcile_classification,
};
pub use curator_backup::{CuratorBackup, BackupError, SnapshotMeta};
pub use history::{ReviewHistory, ReviewRecord, ReviewStatus};
pub use observability::ObservabilityStore;

pub use backfill_triggers::{
    run_backfill_triggers, BackfillTriggersOutcome, DEFAULT_BATCH_SIZE,
};
pub use workflow::{
    run_curator_if_needed,
    run_curator_llm_if_needed,
    skills_default_path_public, wait_for_pending_reviews,
};
