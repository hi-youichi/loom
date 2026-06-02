//! Curator: Background review system for Loom.

pub mod background_review;

pub use background_review::{
    AgentReviewConfig, AgentReviewResult, AgentReviewRunner, BackgroundReviewConfig,
    BackgroundReviewHandle, build_background_config_from_opts, build_review_agent_client,
    Curator, CuratorConfig, CuratorReport, MemoryStore, parse_llm_review_response,
    PendingReviewRegistry, ReviewHistory, ReviewOutputFn, ReviewRecord, ReviewMode,
    SkillRegistry, spawn_background_review, wait_for_pending_reviews,
};