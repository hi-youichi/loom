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
    Lifecycle, SkillContent, SkillError, SkillMeta, SkillStorageRegistry as SkillRegistry, Source,
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

/// Extension methods for `SkillStorageRegistry` that the shared
/// `agent::skill::storage` crate does not provide but the curator needs.
pub trait SkillRegistryCuratorExt {
    /// Return the creation time of the skill library base directory.
    ///
    /// Used by the curator's first-run delay logic to avoid consolidating
    /// a freshly-installed skill set. Returns `None` if the directory
    /// metadata cannot be read (e.g. the directory does not exist yet).
    fn library_created_at(&self) -> Option<chrono::DateTime<chrono::Utc>>;
}

impl SkillRegistryCuratorExt for SkillRegistry {
    fn library_created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let meta = std::fs::metadata(self.base_dir()).ok()?;
        let created = meta.created().ok()?;
        chrono::DateTime::<chrono::Utc>::from(created).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_ends_with_skills() {
        let p = default_path();
        assert!(p.ends_with("skills"));
    }

    #[test]
    fn ext_default_path_ends_with_skills() {
        let p = <SkillRegistry as SkillRegistryExt>::default_path();
        assert!(p.ends_with("skills"));
    }

    #[test]
    fn ext_default_path_matches_module_function() {
        assert_eq!(
            default_path(),
            <SkillRegistry as SkillRegistryExt>::default_path()
        );
    }

    #[test]
    fn library_created_at_returns_some_for_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new(dir.path());
        assert!(registry.library_created_at().is_some());
    }

    #[test]
    fn library_created_at_returns_none_for_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-created");
        let registry = SkillRegistry::new(&missing);
        assert!(registry.library_created_at().is_none());
    }
}
