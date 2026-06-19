//! Re-export of skill storage types from `agent::skill::storage`.
//!
//! Historically this module defined a private `SkillRegistry` with
//! `list/load/save/delete/patch/write_file/remove_file/find_matching` and
//! `Lifecycle`/`Source`/`SkillContent`/`SkillMeta`/`SkillError` types.
//! Those types are byte-for-byte compatible with `agent::skill::storage`
//! (only difference: the manual-source subdirectory was renamed from
//! `manual/` to `curated/`), so this module now re-exports the shared
//! implementation. New code should depend on `agent::skill::storage`
//! directly.
//!
//! The `SkillRegistryExt` trait restores the `default_path()` associated
//! function that the private `SkillRegistry` previously provided, since
//! `agent::skill::storage` deliberately does not depend on `env_config`.

use std::path::PathBuf;

pub use skill::storage::{
    Lifecycle, SkillContent, SkillError, SkillMeta, SkillStorageRegistry as SkillRegistry,
    Source,
};

/// Returns the default on-disk path to the skill library, used by CLI
/// commands that don't have an explicit base directory. Mirrors the
/// private `SkillRegistry::default_path()` method that previously lived
/// on the now-deleted private `SkillRegistry`; kept as a module-level
/// function because `agent::skill::storage` deliberately does not
/// depend on `env_config`.
pub fn default_path() -> PathBuf {
    env_config::home::loom_home().join("data").join("skills")
}

pub trait SkillRegistryExt {
    fn default_path() -> PathBuf;
}

impl SkillRegistryExt for SkillRegistry {
    fn default_path() -> PathBuf {
        env_config::home::loom_home().join("data").join("skills")
    }
}
