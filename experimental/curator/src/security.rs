//! Re-export of runtime skill validation from `agent::skill::validation`.
//!
//! Historically this module defined `validate_skill_create` /
//! `validate_skill_path` / `validate_memory_content` plus
//! `Severity` / `ValidationWarning` / `ValidationResult` types. Those
//! types and functions are now provided by `agent::skill::validation`,
//! which has identical behaviour (and a fixed uppercase-pattern bug).
//! New code should depend on `agent::skill::validation` directly.

pub use skill::validation::{
    validate_memory_content, validate_skill_create, validate_skill_path, Severity,
    ValidationResult, ValidationWarning,
};
