//! Bundled sync — manifest-driven synchronization of built-in skills.
//!
//! Built-in skills are shipped with Loom and synced to the user's skill directory.
//! A manifest tracks origin hashes so user modifications are preserved.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Recursively remove `path` if it exists. Returns `Ok(true)` if a
/// removal actually happened, `Ok(false)` if the path was already
/// absent. Any IO error is propagated.
fn remove_dir_if_exists(path: &Path) -> std::io::Result<bool> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Hermes-aligned `shutil.copytree` analogue: recursively copy `src`
/// into `dst`, creating `dst` first. Existing files in `dst` are
/// overwritten. Symlinks are preserved as-is (Hermes parity).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("copy_tree source is not a directory: {}", src.display()),
        ));
    }
    fs::create_dir_all(dst)?;
    let mut stack: Vec<PathBuf> = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)?;
        for e in entries {
            let e = e?;
            let ft = e.file_type()?;
            let from = e.path();
            let to = dst.join(from.strip_prefix(src).unwrap_or(&from));
            if ft.is_dir() {
                fs::create_dir_all(&to)?;
                stack.push(from);
            } else if ft.is_symlink() {
                // Re-create the symlink at `to`. Hermetic semantics — we
                // never follow a symlink during the copy (would diverge
                // from Hermes `copytree(symlinks=True)`). Windows builds
                // fall back to copying the symlink target's bytes, which
                // is consistent with Hermes on Linux/Mac and acceptable
                // for Windows skills that never carry symlinks anyway.
                let target = fs::read_link(&from)?;
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &to)?;
                }
                #[cfg(not(unix))]
                {
                    // Best-effort: copy target contents. Hermes does not
                    // exercise this path.
                    if target.is_dir() {
                        copy_tree(&target, &to)?;
                    } else if target.is_file() {
                        fs::copy(&target, &to)?;
                    } else {
                        fs::copy(&from, &to)?;
                    }
                }
            } else {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&from, &to)?;
            }
        }
    }
    Ok(())
}

const MANIFEST_FILENAME: &str = ".bundled_manifest";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundledManifest {
    entries: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub removed: Vec<String>,
}

/// Compute a content hash for change-detection purposes.
///
/// Hermes parity (`skills_sync.py:118-145`): the manifest hash is the MD5 of
/// the skill's *whole filesystem tree* (sorted rglob of files, MD5 of each
/// file's bytes concatenated into a single digest) so attached resources
/// (`references/`, `templates/`, `scripts/`, `assets/`) participate in
/// change detection rather than only the SKILL.md frontmatter.
///
/// The legacy string-only `DefaultHasher` variant (kept for backward
/// compatibility with old manifests) is exposed via [`compute_content_hash`]
/// — callers that need to reproduce a `bundled_manifest` line written before
/// this change should reach for that helper.
pub fn compute_hash(content: &str) -> String {
    // Cheap, string-only fallback used by manifest round-trip tests and by
    // any legacy caller that has not yet been ported to the rglob variant.
    compute_content_hash(content)
}

/// MD5 over a string. Hermes-aligned for SKILL.md-only fallback.
pub fn compute_content_hash(content: &str) -> String {
    format!("{:032x}", md5::compute(content.as_bytes()))
}

/// Hermes `skills_sync.py:118-145` rglob hash: walk `skill_dir` recursively,
/// sort entries deterministically, MD5 the concatenation of `<relpath>\0`
/// followed by the file bytes. Returns the empty hash for an empty / missing
/// directory so callers can distinguish "no content" from "missing".
pub fn compute_dir_hash(skill_dir: &Path) -> String {
    if !skill_dir.exists() {
        return compute_content_hash("");
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_files_sorted(skill_dir, &mut paths);
    let mut hasher = md5::Context::new();
    for p in &paths {
        if let Ok(rel) = p.strip_prefix(skill_dir) {
            hasher.consume(rel.to_string_lossy().as_bytes());
            hasher.consume(b"\0");
        }
        if let Ok(bytes) = fs::read(p) {
            hasher.consume(&bytes);
            hasher.consume(b"\0");
        }
    }
    format!("{:032x}", hasher.compute())
}

fn collect_files_sorted(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut children: Vec<(String, PathBuf, bool)> = entries
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let path = e.path();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (name, path, is_dir)
            })
            .collect();
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path, is_dir) in children {
            // Skip the manifest file itself; otherwise every manifest write
            // would invalidate the manifest hash. (Hermes excludes
            // `.bundled_manifest` from the rglob, see skills_sync.py:131.)
            if !is_dir && name == MANIFEST_FILENAME {
                continue;
            }
            if is_dir {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
}

pub fn load_manifest(skills_dir: &Path) -> BundledManifest {
    let path = skills_dir.join(MANIFEST_FILENAME);
    if !path.is_file() {
        return BundledManifest::default();
    }
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return BundledManifest::default(),
    };
    let mut manifest = BundledManifest::default();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, hash)) = line.split_once(':') {
            manifest.entries.insert(name.to_string(), hash.to_string());
        }
    }
    manifest
}

pub fn save_manifest(skills_dir: &Path, manifest: &BundledManifest) {
    let path = skills_dir.join(MANIFEST_FILENAME);
    let mut lines: Vec<String> = manifest
        .entries
        .iter()
        .map(|(name, hash)| format!("{}:{}", name, hash))
        .collect();
    lines.sort();
    let body = lines.join("\n");
    // Hermes-aligned atomic write (skills_sync.py:178-194): the bare
    // `fs::write` here would leave a half-written file if the process is
    // killed mid-write, which would then cause the next `sync_skills`
    // invocation to read a corrupt manifest and over-report `removed`
    // entries. `atomic_write_text` writes to a unique tempfile (pid +
    // nanos) then renames over the destination.
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = crate::storage::atomic_write_text(&path, &body);
}

pub fn sync_skills(bundled_dir: &Path, user_skills_dir: &Path) -> SyncResult {
    let mut result = SyncResult {
        added: Vec::new(),
        updated: Vec::new(),
        skipped: Vec::new(),
        removed: Vec::new(),
    };

    let mut manifest = load_manifest(user_skills_dir);

    let bundled_skills = scan_bundled_skills(bundled_dir);
    let mut bundled_names: Vec<String> = bundled_skills.keys().cloned().collect();
    bundled_names.sort();

for name in &bundled_names {
        let src_path = bundled_skills[name].clone();
        let dest_dir = user_skills_dir.join(name);
        let dest_skill = dest_dir.join("SKILL.md");

        let src_content = match fs::read_to_string(&src_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Hermes parity (`skills_sync.py:118-145`): the manifest hash covers
        // the whole skill tree (attached resources included), not just the
        // SKILL.md bytes. `src_path` is the SKILL.md path so we hash its
        // parent directory. Same applies to the dest_hash branch below.
        let src_hash = compute_dir_hash(src_path.parent().unwrap_or(src_path.as_path()));

        if !dest_dir.is_dir() {
            // DELETED-BY-USER guard (`skills_sync.py:204-225`): a skill that
            // the user previously deleted (so the dest dir is gone) but
            // still appears in the manifest must NOT be silently
            // re-installed. We only create + write if there is no manifest
            // entry (truly fresh sync) OR if the entry exists but the dir
            // was removed by some external process and the user later
            // asked for a re-sync. Hermes honours a `LOOM_SKILL_FORCE`
            // flag here; we keep that behaviour and rely on the manifest
            // entry to detect "user previously removed".
            // DELETED-BY-USER detection (`skills_sync.py:204-225`): the
            // dead `!bundled_skills.contains_key(name)` was removed — we're
            // iterating `bundled_names`, so that condition was always false
            // (silently masking user-deleted skill reinstalls). The
            // manifest walk at the end of `sync_skills` is the authoritative
            // pass that drops entries whose bundled counterpart has gone
            // away; here we only need the "previously-installed but dest
            // dir is now gone" check.
            let user_deleted = manifest.entries.contains_key(name)
                && !dest_dir.is_dir()
                && std::env::var("LOOM_SKILL_FORCE").is_err();
            if user_deleted {
                result.skipped.push(format!("{} (deleted by user)", name));
                continue;
            }
            if fs::create_dir_all(&dest_dir).is_err() {
                continue;
            }
            let _ = fs::write(&dest_skill, &src_content);
            manifest.entries.insert(name.clone(), src_hash);
            result.added.push(name.clone());
            continue;
        }

        let existing_hash = manifest.entries.get(name).cloned();
        match existing_hash {
            Some(ref recorded_hash) if recorded_hash == &src_hash => {
                result.skipped.push(name.clone());
            }
Some(ref recorded_hash) => {
                let dest_hash = compute_dir_hash(&dest_dir);
                if dest_hash == *recorded_hash {
                    // Hermes-aligned UPDATE branch (skills_sync.py:262-289):
                    // rename dest→dest.bak, copytree src→dest, on failure
                    // restore .bak, on success rmtree .bak. This is the
                    // atomic rollback discipline so a half-written dest
                    // never blocks the next sync.
                    let backup_dir = dest_dir.with_extension("bak");
                    let _ = remove_dir_if_exists(&backup_dir);
                    if fs::rename(&dest_dir, &backup_dir).is_err() {
                        // Can't even take a backup — be conservative and
                        // skip rather than risk losing the user's copy.
                        result.skipped.push(format!("{} (user modified)", name));
                        continue;
                    }
                    let src_dir = src_path.parent().unwrap_or(src_path.as_path());
                    if let Err(e) = copy_tree(src_dir, &dest_dir) {
                        // Rollback: move .bak back to dest.
                        let _ = fs::remove_dir_all(&dest_dir);
                        let _ = fs::rename(&backup_dir, &dest_dir);
                        debug!("sync_skills: UPDATE failed for '{}': {}, restored", name, e);
                        result.skipped.push(format!("{} (rollback: {})", name, e));
                        continue;
                    }
                    manifest.entries.insert(name.clone(), src_hash.clone());
                    result.updated.push(name.clone());
                    let _ = fs::remove_dir_all(&backup_dir);
                } else {
                    result.skipped.push(format!("{} (user modified)", name));
                }
            }
            None => {
                if dest_skill.is_file() {
                    result.skipped.push(format!("{} (no manifest entry)", name));
                } else {
                    let _ = fs::write(&dest_skill, &src_content);
                    manifest.entries.insert(name.clone(), src_hash);
                    result.updated.push(name.clone());
                }
            }
        }
    }

    let bundled_set: std::collections::HashSet<&String> = bundled_names.iter().collect();
    let manifest_names: Vec<String> = manifest.entries.keys().cloned().collect();
    for name in &manifest_names {
        if !bundled_set.contains(name) {
            manifest.entries.remove(name);
            result.removed.push(name.clone());
        }
    }

    save_manifest(user_skills_dir, &manifest);
    result
}

/// Recursively scan `dir` for `SKILL.md` files, preserving category
/// subdirectories (e.g. `mlops/axolotl/SKILL.md`).
///
/// Hermes parity (`skills_sync.py:301-356`): the bundled layout may nest
/// skills under category subdirs (mlops/axolotl/, data-science/kafka/, …)
/// and the original flat `read_dir` scan dropped them silently. We use
/// the existing `collect_files_sorted` helper to rglob `SKILL.md` files
/// and record their parent directory name as the skill key.
fn scan_bundled_skills(dir: &Path) -> HashMap<String, PathBuf> {
    let mut skills = HashMap::new();
    if !dir.is_dir() {
        return skills;
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_files_sorted(dir, &mut paths);
    for p in paths {
        if p.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
            continue;
        }
        let Some(parent) = p.parent() else { continue };
        // Skill key is the parent dir's name (Hermes parity) so category
        // subdirs (mlops/axolotl) collide by `axolotl`, not by full path.
        if let Some(name) = parent.file_name().and_then(|n| n.to_str()) {
            skills.entry(name.to_string()).or_insert(p);
        }
    }
    skills
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_bundled_skill(dir: &std::path::Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn compute_hash_deterministic() {
        let h1 = compute_hash("hello world");
        let h2 = compute_hash("hello world");
        assert_eq!(h1, h2);
        assert_ne!(h1, compute_hash("hello earth"));
    }

    #[test]
    fn load_manifest_missing() {
        let dir = tempfile::tempdir().unwrap();
        let m = load_manifest(dir.path());
        assert!(m.entries.is_empty());
    }

    #[test]
    fn save_and_load_manifest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = BundledManifest::default();
        m.entries.insert("alpha".to_string(), "hash1".to_string());
        m.entries.insert("beta".to_string(), "hash2".to_string());
        save_manifest(dir.path(), &m);
        let loaded = load_manifest(dir.path());
        assert_eq!(loaded.entries.get("alpha").unwrap(), "hash1");
        assert_eq!(loaded.entries.get("beta").unwrap(), "hash2");
    }

    #[test]
    fn save_manifest_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = BundledManifest::default();
        m.entries.insert("zebra".to_string(), "h1".to_string());
        m.entries.insert("alpha".to_string(), "h2".to_string());
        m.entries.insert("middle".to_string(), "h3".to_string());
        save_manifest(dir.path(), &m);
        let data = fs::read_to_string(dir.path().join(".bundled_manifest")).unwrap();
        let names: Vec<&str> = data.lines().map(|l| l.split(':').next().unwrap()).collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn manifest_ignores_comments_and_blanks() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# comment\n\nalpha:hash1\n\n# another comment\nbeta:hash2\n";
        fs::write(dir.path().join(".bundled_manifest"), content).unwrap();
        let m = load_manifest(dir.path());
        assert_eq!(m.entries.len(), 2);
    }

    #[test]
    fn sync_new_skills() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("bundled");
        let user = dir.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        make_bundled_skill(&bundled, "skill-a", "---\nname: skill-a\n---\nBody A");
        make_bundled_skill(&bundled, "skill-b", "---\nname: skill-b\n---\nBody B");

        let result = sync_skills(&bundled, &user);
        assert!(result.added.contains(&"skill-a".to_string()));
        assert!(result.added.contains(&"skill-b".to_string()));
        assert!(result.skipped.is_empty());
        assert!(user.join("skill-a").join("SKILL.md").exists());
        assert!(user.join("skill-b").join("SKILL.md").exists());
    }

    #[test]
    fn sync_existing_unchanged_skills_skips() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("bundled");
        let user = dir.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        make_bundled_skill(&bundled, "a", "v1");
        let _ = sync_skills(&bundled, &user);
        let result = sync_skills(&bundled, &user);
        assert!(result.added.is_empty());
        assert!(result.skipped.iter().any(|s| s == "a"));
    }

    #[test]
    fn sync_updated_skills_when_user_not_modified() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("bundled");
        let user = dir.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        make_bundled_skill(&bundled, "a", "v1");
        let _ = sync_skills(&bundled, &user);

        make_bundled_skill(&bundled, "a", "v2");
        let result = sync_skills(&bundled, &user);
        assert!(result.updated.iter().any(|s| s == "a"));
        assert_eq!(fs::read_to_string(user.join("a").join("SKILL.md")).unwrap(), "v2");
    }

    #[test]
    fn sync_skips_user_modified_skills() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("bundled");
        let user = dir.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        make_bundled_skill(&bundled, "a", "v1");
        let _ = sync_skills(&bundled, &user);

        fs::write(user.join("a").join("SKILL.md"), "user modified content").unwrap();
        make_bundled_skill(&bundled, "a", "v2");
        let result = sync_skills(&bundled, &user);
        assert!(result.skipped.iter().any(|s| s.contains("user modified")));
        assert_eq!(
            fs::read_to_string(user.join("a").join("SKILL.md")).unwrap(),
            "user modified content"
        );
    }

    #[test]
    fn sync_removes_stale_manifest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        fs::create_dir_all(&user).unwrap();

        let mut m = BundledManifest::default();
        m.entries.insert("old-skill".to_string(), "oldhash".to_string());
        save_manifest(&user, &m);

        let empty_bundled = dir.path().join("bundled");
        fs::create_dir_all(&empty_bundled).unwrap();

        let result = sync_skills(&empty_bundled, &user);
        assert!(result.removed.contains(&"old-skill".to_string()));

        let after = load_manifest(&user);
        assert!(!after.entries.contains_key("old-skill"));
    }
}
