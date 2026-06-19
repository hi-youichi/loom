//! `skill` crate — skill discovery, storage, and usage tracking.
//!
//! This crate provides the core skill functionality for Loom, independent of
//! the main loom runtime. It includes:
//!
//! - **Discovery**: Scanning and parsing skill files from various locations
//! - **Storage**: CRUD operations for skill persistence
//! - **Usage**: Usage telemetry and lifecycle tracking
//! - **Cache**: Two-layer (LRU + disk snapshot) caching for discovery
//! - **Preprocessing**: Template variable substitution and inline shell expansion
//! - **Guard**: Static security scanning for external skills
//! - **Bundles**: Load multiple skills at once via YAML definitions
//! - **Sync**: Manifest-driven synchronization of built-in skills
//! - **Config vars**: Extract and resolve config variables from skill frontmatter
//!
//! ```text
//! agent/skill
//! ├── bundles       — Skill bundle registry
//! ├── cache         — Two-layer discovery cache
//! ├── config_vars   — Config variable extraction and resolution
//! ├── discovery     — SkillRegistry for finding/loading skills
//! ├── guard         — Security scanning (Skills Guard)
//! ├── security      — Post-write security scanning for agent-created skills
//! ├── preprocessing — Template vars + inline shell expansion
//! ├── provenance    — Write origin tracking
//! ├── storage       — SkillMeta CRUD
//! ├── sync          — Bundled skill synchronization
//! ├── usage         — Usage tracking and reporting
//! ├── utils         — Frontmatter parsing, YAML utilities
//! └── validation    — Runtime content validation for skill writes
//! ```

pub mod bundles;
pub mod cache;
pub mod config_vars;
pub mod discovery;
pub mod guard;
pub mod security;
pub mod preprocessing;
pub mod provenance;
pub mod storage;
pub mod sync;
pub mod usage;
pub mod utils;
pub mod validation;

pub use bundles::{BundleRegistry, SkillBundle};
pub use cache::SkillCache;
pub use config_vars::{ConfigVarDecl, extract_config_vars, resolve_config_values, inject_config_into_content};
pub use discovery::{SkillEntry, SkillRegistry, SkillSource};
    pub use guard::{scan_file, scan_skill, resolve_trust_level, should_allow_install, format_scan_report, ScanResult, TrustLevel, Verdict};
    pub use security::{security_scan_skill, assess_install, Assessment};
pub use preprocessing::{substitute_template_vars, expand_inline_shell};
pub use provenance::{WriteOrigin, WriteOriginGuard};
pub use storage::{Lifecycle, SkillContent, SkillError, SkillMeta, Source};
pub use sync::{sync_skills, SyncResult};
pub use usage::{SkillUsage, SkillUsageReport, SkillUsageStore};
pub use utils::{SkillMetadata, ReadinessStatus};
pub use validation::{
    validate_skill_create, validate_skill_name, validate_skill_path,
    validate_frontmatter, validate_name_match,
    validate_memory_content,
    ValidationResult, ValidationWarning, Severity,
};
