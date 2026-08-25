//! Per-session statistics collection for the agent runner.
//!
//! Phase 1 exposes the [`session`] types and the in-memory
//! [`collector::InMemoryCollector`]. SQLite persistence ([`record`]) and the
//! query API ([`query`]) are placeholders scheduled for Phase 2.

pub mod collector;
pub mod event;
pub mod query;
pub mod record;
pub mod session;

use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

pub use collector::{InMemoryCollector, StatsCollector};
pub use session::{
    ErrorKind, ErrorRecord, LlmCallRecord, ModelUsage, SessionStats, SessionStatus,
    SessionSummary, ToolCallRecord, ToolUsage,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StatsError {
    #[error("stats database error: {0}")]
    Database(String),

    #[error("stats serialization error: {0}")]
    Serialization(String),

    #[error("unknown session: {0}")]
    UnknownSession(String),

    #[error("invalid stats query: {0}")]
    InvalidQuery(String),
}

pub fn default_db_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".anureo").join("agent-stats.db")
}

pub fn open_default_collector() -> Arc<dyn StatsCollector> {
    InMemoryCollector::new() as Arc<dyn StatsCollector>
}