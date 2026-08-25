//! `skill` crate — skill discovery, storage, and usage tracking.
//!
//! This crate provides the core skill functionality for anureo, independent of
//! the main anureo runtime. It includes:
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

pub mod archive;
pub mod bundles;
pub mod cache;
pub mod config_vars;
pub mod discovery;
pub mod guard;
pub mod preprocessing;
pub mod provenance;
pub mod security;
pub mod storage;
pub mod sync;
pub mod usage;
pub mod utils;
pub mod validation;

pub use bundles::{BundleRegistry, SkillBundle};
pub use cache::SkillCache;
pub use config_vars::{
    extract_config_vars, inject_config_into_content, resolve_config_values, ConfigVarDecl,
};
pub use discovery::{SkillEntry, SkillRegistry, SkillSource};
pub use guard::{
    format_scan_report, resolve_trust_level, scan_file, scan_skill, should_allow_install,
    ScanResult, TrustLevel, Verdict,
};
pub use preprocessing::{expand_inline_shell, substitute_template_vars};
pub use provenance::{with_write_origin, WriteOrigin};
pub use security::security_scan_skill;
pub use storage::{atomic_write_text, Lifecycle, SkillContent, SkillError, SkillMeta, Source};
pub use sync::{sync_skills, SyncResult};
pub use usage::{SkillUsage, SkillUsageReport, SkillUsageStore};
pub use utils::{ReadinessStatus, SkillMetadata};
pub use validation::{
    validate_frontmatter, validate_memory_content, validate_name_match, validate_skill_create,
    validate_skill_name, validate_skill_path, Severity, ValidationResult, ValidationWarning,
};
