//! Bundled sync — manifest-driven synchronization of built-in skills.
//!
//! Built-in skills are shipped with Loom and synced to the user's skill directory.
//! A manifest tracks origin hashes so user modifications are preserved.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_FILENAME: &str = ".bundled_manifest";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundledManifest {
    entries: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub removed: Vec<String>,
}

pub fn compute_hash(content: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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
    let _ = fs::write(&path, lines.join("\n"));
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
        let src_hash = compute_hash(&src_content);

        if !dest_dir.is_dir() {
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
                let dest_hash = std::fs::read_to_string(&dest_skill)
                    .ok()
                    .map(|c| compute_hash(&c))
                    .unwrap_or_default();
                if dest_hash == *recorded_hash {
                    let _ = fs::write(&dest_skill, &src_content);
                    manifest.entries.insert(name.clone(), src_hash.clone());
                    result.updated.push(name.clone());
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

fn scan_bundled_skills(dir: &Path) -> HashMap<String, PathBuf> {
    let mut skills = HashMap::new();
    if !dir.is_dir() {
        return skills;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    skills.insert(name.to_string(), skill_file);
                }
            }
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
