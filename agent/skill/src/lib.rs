//! `skill` crate — skill discovery, storage, and usage tracking.
//!
//! This crate provides the core skill functionality for Loom, independent of
//! the main loom runtime. It includes:
//!
//! - **Discovery**: Scanning and parsing skill files from various locations
//! - **Storage**: CRUD operations for skill persistence
//! - **Usage**: Usage telemetry and lifecycle tracking
//!
//! ```text
//! agent/skill
//! ├── discovery  — SkillRegistry for finding/loading skills
//! ├── storage    — SkillRegistry for persisting skills
//! ├── usage      — Usage tracking and reporting
//! └── utils      — Frontmatter parsing, YAML utilities
//! ```

pub mod discovery;
pub mod provenance;
pub mod storage;
pub mod usage;
pub mod utils;

// Re-exports for convenience
pub use discovery::{SkillEntry, SkillRegistry, SkillSource};
pub use provenance::{WriteOrigin, WriteOriginGuard};
pub use storage::{Lifecycle, SkillContent, SkillError, SkillMeta, Source};
pub use usage::{SkillUsage, SkillUsageReport, SkillUsageStore};
pub use utils::SkillMetadata;