//! Skill discovery cache — two-layer caching for skill registry scans.
//!
//! Layer 1: In-memory LRU cache keyed by (dirs_hash, filters).
//! Layer 2: Disk snapshot with mtime/size manifest validation.

use crate::discovery::{SkillEntry, SkillSource};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

const CACHE_FILENAME: &str = ".skills_cache.json";
const MAX_LRU_ENTRIES: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheManifest {
    files: HashMap<String, String, std::collections::hash_map::RandomState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheSnapshot {
    manifest: CacheManifest,
    entries: Vec<CachedSkillEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSkillEntry {
    name: String,
    description: String,
    category: Option<String>,
    category_desc: Option<String>,
    base_path: PathBuf,
    skill_file: PathBuf,
    source: SkillSource,
}

pub struct SkillCache {
    lru: Vec<(String, Vec<SkillEntry>)>,
}

impl Default for SkillCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillCache {
    pub fn new() -> Self {
        Self { lru: Vec::with_capacity(MAX_LRU_ENTRIES) }
    }

    pub fn get_lru(&mut self, key: &str) -> Option<Vec<SkillEntry>> {
        if let Some(idx) = self.lru.iter().position(|(k, _)| k == key) {
            let (_, entries) = self.lru.remove(idx);
            self.lru.push((key.to_string(), entries.clone()));
            Some(entries)
        } else {
            None
        }
    }

    pub fn insert_lru(&mut self, key: String, entries: Vec<SkillEntry>) {
        if self.lru.len() >= MAX_LRU_ENTRIES {
            self.lru.remove(0);
        }
        self.lru.push((key, entries));
    }

    pub fn invalidate_all(&mut self) {
        self.lru.clear();
    }

    pub fn load_disk_snapshot(skills_dir: &Path) -> Option<Vec<SkillEntry>> {
        let snapshot_path = skills_dir.join(CACHE_FILENAME);
        if !snapshot_path.is_file() {
            return None;
        }

        let data = std::fs::read_to_string(&snapshot_path).ok()?;
        let snapshot: CacheSnapshot = serde_json::from_str(&data).ok()?;

        let current_manifest = build_manifest(skills_dir);
        if snapshot.manifest.files != current_manifest.files {
            debug!("skill cache manifest mismatch, invalidating");
            return None;
        }

        Some(
            snapshot
                .entries
                .into_iter()
                .map(|c| SkillEntry {
                    metadata: crate::utils::SkillMetadata {
                        name: c.name,
                        description: c.description,
                        category: c.category,
                        category_desc: c.category_desc,
                        ..Default::default()
                    },
                    base_path: c.base_path,
                    skill_file: c.skill_file,
                    source: c.source,
                })
                .collect(),
        )
    }

    pub fn save_disk_snapshot(skills_dir: &Path, entries: &[SkillEntry]) {
        let manifest = build_manifest(skills_dir);
        let cached: Vec<CachedSkillEntry> = entries
            .iter()
            .map(|e| CachedSkillEntry {
                name: e.metadata.name.clone(),
                description: e.metadata.description.clone(),
                category: e.metadata.category.clone(),
                category_desc: e.metadata.category_desc.clone(),
                base_path: e.base_path.clone(),
                skill_file: e.skill_file.clone(),
                source: e.source,
            })
            .collect();

        let snapshot = CacheSnapshot {
            manifest,
            entries: cached,
        };

        let snapshot_path = skills_dir.join(CACHE_FILENAME);
        if let Ok(data) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(&snapshot_path, data);
        }
    }

    pub fn invalidate_disk_snapshot(skills_dir: &Path) {
        let path = skills_dir.join(CACHE_FILENAME);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn build_manifest(skills_dir: &Path) -> CacheManifest {
    let mut files = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for e in entries.flatten() {
            collect_manifest_entries(&e.path(), skills_dir, &mut files);
        }
    }
    CacheManifest { files }
}

fn file_content_hash(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

fn collect_manifest_entries(
    dir: &Path,
    root: &Path,
    manifest: &mut HashMap<String, String>,
) {
    if crate::utils::is_excluded_path(dir) {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                collect_manifest_entries(&path, root, manifest);
            } else {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "SKILL.md" || name == "DESCRIPTION.md" {
                    if let (Some(hash), Ok(rel)) = (file_content_hash(&path), path.strip_prefix(root)) {
                        manifest.insert(rel.to_string_lossy().to_string(), hash);
                    }
                }
            }
        }
    }
}

pub fn make_cache_key(working_folder: &Path, extra_dirs: &[PathBuf]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    working_folder.hash(&mut hasher);
    for d in extra_dirs {
        d.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{SkillEntry, SkillSource};
    use crate::utils::SkillMetadata;
    use std::fs;

    fn make_entry(name: &str, dir: &std::path::Path) -> SkillEntry {
        SkillEntry {
            metadata: SkillMetadata {
                name: name.to_string(),
                description: format!("Desc for {}", name),
                category: Some("test".to_string()),
                ..Default::default()
            },
            base_path: dir.to_path_buf(),
            skill_file: dir.join("SKILL.md"),
            source: SkillSource::Project,
        }
    }

    #[test]
    fn lru_insert_and_get() {
        let mut cache = SkillCache::new();
        let dir = std::path::Path::new("/tmp/test");
        let entries = vec![make_entry("skill-a", dir)];
        cache.insert_lru("key1".to_string(), entries.clone());
        let result = cache.get_lru("key1");
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0].metadata.name, "skill-a");
    }

    #[test]
    fn lru_miss_returns_none() {
        let mut cache = SkillCache::new();
        assert!(cache.get_lru("nonexistent").is_none());
    }

    #[test]
    fn lru_invalidate_clears_all() {
        let mut cache = SkillCache::new();
        let dir = std::path::Path::new("/tmp/test");
        cache.insert_lru("k1".to_string(), vec![make_entry("a", dir)]);
        cache.insert_lru("k2".to_string(), vec![make_entry("b", dir)]);
        cache.invalidate_all();
        assert!(cache.get_lru("k1").is_none());
        assert!(cache.get_lru("k2").is_none());
    }

    #[test]
    fn lru_evicts_oldest_when_full() {
        let mut cache = SkillCache::new();
        let dir = std::path::Path::new("/tmp/test");
        for i in 0..=16 {
            cache.insert_lru(format!("k{}", i), vec![make_entry(&format!("s{}", i), dir)]);
        }
        assert!(cache.get_lru("k0").is_none());
        assert!(cache.get_lru("k16").is_some());
    }

    #[test]
    fn lru_access_promotes_to_front() {
        let mut cache = SkillCache::new();
        let dir = std::path::Path::new("/tmp/test");
        cache.insert_lru("old".to_string(), vec![make_entry("a", dir)]);
        for i in 1..=14 {
            cache.insert_lru(format!("k{}", i), vec![make_entry(&format!("s{}", i), dir)]);
        }
        assert!(cache.get_lru("old").is_some());
        cache.insert_lru("newest".to_string(), vec![make_entry("b", dir)]);
        assert!(cache.get_lru("old").is_some());
    }

    #[test]
    fn disk_snapshot_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: my-skill\n---\nBody").unwrap();

        let entries = vec![make_entry("my-skill", &skill_dir)];
        SkillCache::save_disk_snapshot(&skills_dir, &entries);

        let loaded = SkillCache::load_disk_snapshot(&skills_dir);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap()[0].metadata.name, "my-skill");
    }

    #[test]
    fn disk_snapshot_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(SkillCache::load_disk_snapshot(dir.path()).is_none());
    }

    #[test]
    fn disk_snapshot_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("s1");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: s1\n---\nBody").unwrap();

        SkillCache::save_disk_snapshot(&skills_dir, &[make_entry("s1", &skill_dir)]);
        SkillCache::invalidate_disk_snapshot(&skills_dir);
        assert!(SkillCache::load_disk_snapshot(&skills_dir).is_none());
    }

    #[test]
    fn disk_manifest_mismatch_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("s1");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: s1\n---\nBody").unwrap();

        SkillCache::save_disk_snapshot(&skills_dir, &[make_entry("s1", &skill_dir)]);

        fs::write(skill_dir.join("SKILL.md"), "---\nname: s1\n---\nChanged body").unwrap();

        assert!(SkillCache::load_disk_snapshot(&skills_dir).is_none());
    }

    #[test]
    fn make_cache_key_deterministic() {
        let p1 = std::path::Path::new("/foo");
        let p2 = std::path::PathBuf::from("/bar");
        let k1 = make_cache_key(p1, &[p2.clone()]);
        let k2 = make_cache_key(p1, &[p2]);
        assert_eq!(k1, k2);
    }
}
