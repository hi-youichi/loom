//! Skill archive service — moves a skill's directory into `.archive/<name>-<ts>/`
//! instead of deleting it, so it can be inspected or `restore`d later.
//!
//! Extracted from `experimental/curator/src/curator.rs:608-673`
//! (`archive_skill`) per Hermes parity (`tools/skill_manager_tool.py` gap #4):
//! when `manage.rs::handle_delete` is called with `absorbed_into.is_some()`
//! or from the `BackgroundReview` origin, the skill must NOT be hard-deleted
//! — it goes under `.archive/` so the curator's restore path can find it.
//!
//! Collision handling: if `.archive/<name>` already exists, we append the
//! Hermes-aligned 14-digit UTC stamp (`%Y%m%d%H%M%S`,
//! `tools/skill_usage.py:506-512`) so re-archives never silently
//! overwrite each other and `restore_skill` can disambiguate by suffix
//! shape. The previous 10-digit `as_secs()` counter collided on rapid
//! re-archives.
//!
//! Cross-device fallback: `fs::rename` fails on Windows when src/dst
//! are on different volumes. Mirror Hermes `shutil.move` — fall back
//! to copy-tree + `remove_dir_all`.

use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::storage::SkillError;

/// Move `base_dir/<name>` to `base_dir/.archive/<name>[-<ts>]/`.
///
/// Returns the original source path on success (`Ok(src)`) so callers can
/// verify what disappeared from the active tree. The destination path is
/// available for callers that want to log or surface it (it is also
/// always derivable as `base_dir/.archive/<name>` when no collision).
///
/// Errors:
/// - `SkillError::NotFound` if the source directory doesn't exist.
/// - `SkillError::Io` for any other I/O failure after the
///   copy+remove fallback is exhausted.
pub fn archive_skill_to(base_dir: &Path, name: &str) -> Result<PathBuf, SkillError> {
    let src = find_skill_dir(base_dir, name).ok_or_else(|| SkillError::NotFound(name.into()))?;
    if !src.is_dir() {
        return Err(SkillError::NotFound(name.to_string()));
    }
    let archive_root = base_dir.join(".archive");
    std::fs::create_dir_all(&archive_root)?;
    let mut dst = archive_root.join(name);
    if dst.exists() {
        let ts = Utc::now().format("%Y%m%d%H%M%S").to_string();
        dst = archive_root.join(format!("{}-{}", name, ts));
    }
    if let Err(e) = std::fs::rename(&src, &dst) {
        if e.raw_os_error() == Some(17) || e.raw_os_error() == Some(18) {
            // 17=ERROR_NOT_SAME_DEVICE, 18=ERROR_NO_MORE_FILES. Fall back.
            copy_dir_recursive(&src, &dst)?;
            std::fs::remove_dir_all(&src)?;
        } else {
            return Err(SkillError::Io(e));
        }
    }
    Ok(src)
}

fn find_skill_dir(base_dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = base_dir.join(name);
    if direct.is_dir() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(base_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.file_name().and_then(|n| n.to_str()) == Some(".archive") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some(name)
            && path.join("SKILL.md").is_file()
        {
            return Some(path);
        }
        if let Some(found) = find_skill_dir(&path, name) {
            return Some(found);
        }
    }
    None
}

/// Recursively copy `src` directory to `dst`. Used as the fallback when
/// `fs::rename` is refused by the OS (different volume / cross-mount).
fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            {
                // Re-create the symlink at the destination rather than
                // copying the link target's contents — matches `cp -P`
                // semantics and avoids dragging an out-of-tree target
                // into the archive.
                let target = std::fs::read_link(&src_path)?;
                std::os::unix::fs::symlink(&target, &dst_path)?;
            }
            #[cfg(not(unix))]
            {
                // Windows path: copy the file bytes as a regular file.
                std::fs::copy(&src_path, &dst_path)?;
                let _ = entry; // suppress warning on Windows
            }
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anureo-archive-test-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn archives_existing_skill() {
        let base = unique_tmp("ok");
        let skill_dir = base.join("plan");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# plan").unwrap();

        let archived_src = archive_skill_to(&base, "plan").expect("archive should succeed");
        assert_eq!(archived_src, base.join("plan"));
        assert!(!base.join("plan").exists(), "active skill should be gone");
        let archived = base.join(".archive").join("plan");
        assert!(
            archived.join("SKILL.md").exists(),
            "archived skill.md should exist"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn missing_skill_returns_not_found() {
        let base = unique_tmp("missing");
        let err = archive_skill_to(&base, "does-not-exist").unwrap_err();
        match err {
            SkillError::NotFound(_) => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn collision_appends_timestamp() {
        let base = unique_tmp("col");
        let skill = base.join("plan");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "v1").unwrap();
        archive_skill_to(&base, "plan").unwrap();

        // Re-create the active skill with different content then
        // re-archive. The second archive should land in a
        // timestamp-suffixed path.
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "v2").unwrap();
        archive_skill_to(&base, "plan").unwrap();

        let archive_root = base.join(".archive");
        let entries: Vec<_> = fs::read_dir(&archive_root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("plan"))
            .collect();
        assert!(
            entries.len() >= 2,
            "both archives should survive; got {} entries",
            entries.len()
        );
        fs::remove_dir_all(&base).unwrap();
    }
}
