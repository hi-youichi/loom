//! Skill storage — CRUD operations for persisting skills.
//!
//! This module provides `SkillStorageRegistry` for managing skill persistence,
//! including creation, reading, updating, and deletion of skills with metadata.

use crate::utils::{is_excluded_path, parse_frontmatter};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Skill lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    /// Skill is active and available.
    Active,
    /// Skill hasn't been used recently and may be archived.
    Stale,
    /// Skill has been archived and is no longer shown by default.
    Archived,
}

/// Skill source/origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Auto-generated skill (e.g., by background review).
    Auto,
    /// Manually created skill.
    Manual,
    /// Evolved skill (generated from usage patterns).
    Evolved,
}

/// Metadata for a skill in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub lifecycle: Lifecycle,
    pub source: Source,
    pub triggers: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_used: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub created_by: Option<String>,
}

/// Full skill content including body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContent {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub lifecycle: Lifecycle,
    pub source: Source,
    #[serde(default)]
    pub created_by: Option<String>,
    pub body: String,
    pub raw: String,
}

/// Errors that can occur during skill storage operations.
#[derive(Debug, Error)]
pub enum SkillError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Skill not found: {0}")]
    NotFound(String),
    #[error("Invalid skill format: {0}")]
    InvalidFormat(String),
}

/// Registry for persistent skill storage.
pub struct SkillStorageRegistry {
    base_dir: PathBuf,
}

impl SkillStorageRegistry {
    /// Create a new storage registry at the given base directory.
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Get the base directory for this registry.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Compute the storage directory for a skill given its source.
    ///
    /// Maps `Source` to a subdirectory under `base_dir`:
    /// - `Auto`    → `base_dir/auto/<name>/`
    /// - `Manual`  → `base_dir/curated/<name>/`
    /// - `Evolved` → `base_dir/evolved/<name>/`
    pub fn skill_dir(&self, source: Source, name: &str) -> PathBuf {
        let subdir = match source {
            Source::Auto => "auto",
            Source::Manual => "curated",
            Source::Evolved => "evolved",
        };
        self.base_dir.join(subdir).join(name)
    }

    pub fn skill_file_path(&self, source: Source, name: &str) -> PathBuf {
        self.skill_dir(source, name).join("SKILL.md")
    }

    /// Find the directory containing a skill by name, searching the entire tree.
    ///
    /// Walks `base_dir` recursively looking for `<dir>/SKILL.md` where
    /// `<dir>` has the given name. Excludes `.git`, `node_modules`, etc.
    /// Returns the path to the directory containing `SKILL.md`.
    fn find_skill_dir(&self, name: &str) -> Option<PathBuf> {
        if !self.base_dir.exists() {
            return None;
        }
        self.scan_for_skill_dir(&self.base_dir, name)
    }

    /// Recursive helper for `find_skill_dir`.
    fn scan_for_skill_dir(&self, dir: &Path, name: &str) -> Option<PathBuf> {
        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || is_excluded_path(&path) {
                continue;
            }
            // Check if this directory IS the skill (matches name + has SKILL.md)
            if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    return Some(path);
                }
            }
            // Recurse into subdirectories
            if let Some(found) = self.scan_for_skill_dir(&path, name) {
                return Some(found);
            }
        }
        None
    }

    /// Recursively collect all `SKILL.md` file paths under `base_dir`.
    fn collect_skill_files(&self) -> Vec<PathBuf> {
        if !self.base_dir.exists() {
            return Vec::new();
        }
        let mut result = Vec::new();
        self.collect_skill_files_recursive(&self.base_dir, &mut result);
        result
    }

    /// Recursive helper for `collect_skill_files`.
    fn collect_skill_files_recursive(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if is_excluded_path(&path) {
                        continue;
                    }
                    // Check if this dir has a SKILL.md
                    let skill_md = path.join("SKILL.md");
                    if skill_md.exists() {
                        out.push(skill_md);
                    } else {
                        // Recurse deeper
                        self.collect_skill_files_recursive(&path, out);
                    }
                }
            }
        }
    }

    /// List all skills in the registry.
    ///
    /// Recursively scans `base_dir` for all `SKILL.md` files, excluding
    /// directories like `.git`, `node_modules`, `.archive`, etc.
    /// This aligns with Hermes `iter_skill_index_files()` (rglob + EXCLUDED_DIRS).
    pub fn list(&self) -> Result<Vec<SkillMeta>, SkillError> {
        let mut skills = Vec::new();
        for skill_md in self.collect_skill_files() {
            if let Ok(content) = self.load_from_path(&skill_md) {
                let pinned = self
                    .read_pinned_from_frontmatter(&skill_md)
                    .unwrap_or(false);
                skills.push(SkillMeta {
                    name: content.name.clone(),
                    description: content.description.clone(),
                    lifecycle: content.lifecycle,
                    source: content.source,
                    triggers: content.triggers,
                    created_at: None,
                    last_used: None,
                    pinned,
                    created_by: content.created_by.clone(),
                });
            }
        }
        Ok(skills)
    }

    /// Load a skill by name.
    ///
    /// Searches the entire `base_dir` tree for `<name>/SKILL.md`,
    /// providing backward compatibility with both old (`auto/curated/evolved/`)
    /// and new (flat/category) layouts.
    pub fn load(&self, name: &str) -> Result<SkillContent, SkillError> {
        if let Some(dir) = self.find_skill_dir(name) {
            let path = dir.join("SKILL.md");
            if path.exists() {
                return self.load_from_path(&path);
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    fn load_from_path(&self, path: &Path) -> Result<SkillContent, SkillError> {
        let raw = fs::read_to_string(path)?;
        let raw_owned = raw.clone();
        let (frontmatter, body) = parse_frontmatter(&raw);

        let name = frontmatter
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SkillError::InvalidFormat("missing name".into()))?
            .to_string();

        let description = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let triggers = frontmatter
            .get("triggers")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let lifecycle = frontmatter
            .get("lifecycle")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_yaml::from_str(s).ok())
            .unwrap_or(Lifecycle::Active);

        let source = frontmatter
            .get("source")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_yaml::from_str(s).ok())
            .unwrap_or(Source::Manual);

        let created_by = frontmatter
            .get("created_by")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(SkillContent {
            name,
            description,
            triggers,
            lifecycle,
            source,
            created_by,
            body,
            raw: raw_owned,
        })
    }

    /// Save a skill to the registry.
    pub fn save(&self, name: &str, content: &SkillContent) -> Result<(), SkillError> {
        // If skill already exists somewhere in the tree, overwrite in place
        // to avoid creating duplicates when the layout has been migrated.
        let dir = match self.find_skill_dir(name) {
            Some(existing) => existing,
            None => self.skill_dir(content.source, name),
        };
        fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");

        let frontmatter = serde_yaml::to_string(&YamlValue::Mapping({
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                YamlValue::String("name".into()),
                YamlValue::String(content.name.clone()),
            );
            map.insert(
                YamlValue::String("description".into()),
                YamlValue::String(content.description.clone()),
            );
            map.insert(
                YamlValue::String("triggers".into()),
                YamlValue::Sequence(
                    content
                        .triggers
                        .iter()
                        .map(|t| YamlValue::String(t.clone()))
                        .collect(),
                ),
            );
            map.insert(
                YamlValue::String("lifecycle".into()),
                YamlValue::String(
                    serde_yaml::to_string(&content.lifecycle)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                ),
            );
            map.insert(
                YamlValue::String("source".into()),
                YamlValue::String(
                    serde_yaml::to_string(&content.source)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                ),
            );
            if let Some(ref by) = content.created_by {
                map.insert(
                    YamlValue::String("created_by".into()),
                    YamlValue::String(by.clone()),
                );
            }
            map
        }))?;

        let file_content = format!("---\n{}---\n{}", frontmatter, content.body);
        fs::write(&path, file_content)?;
        Ok(())
    }

    /// Delete a skill from the registry.
    ///
    /// Searches the entire tree for the skill directory and removes it.
    pub fn delete(&self, name: &str) -> Result<(), SkillError> {
        if let Some(dir) = self.find_skill_dir(name) {
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
                return Ok(());
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    /// Set the `pinned` flag on a skill by updating its SKILL.md frontmatter.
    ///
    /// This reads the current SKILL.md, modifies the `pinned` field in the
    /// YAML frontmatter, and writes it back — preserving the body unchanged.
    pub fn set_pinned(&self, name: &str, pinned: bool) -> Result<(), SkillError> {
        if let Some(dir) = self.find_skill_dir(name) {
            let path = dir.join("SKILL.md");
            if path.exists() {
                let raw = fs::read_to_string(&path)?;
                let (mut frontmatter, body) = parse_frontmatter(&raw);

                frontmatter.insert(
                    serde_yaml::Value::String("pinned".into()),
                    serde_yaml::Value::Bool(pinned),
                );

                let new_yaml = serde_yaml::to_string(&frontmatter)
                    .map_err(|e| SkillError::InvalidFormat(e.to_string()))?;
                let new_content = format!("---\n{}---\n{}", new_yaml, body);
                fs::write(&path, new_content)?;
                return Ok(());
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    /// Set the `lifecycle` of a skill by updating its SKILL.md frontmatter.
    ///
    /// Used by `curator restore` to move a skill back to Active.
    pub fn set_lifecycle(
        &self,
        name: &str,
        lifecycle: Lifecycle,
    ) -> Result<(), SkillError> {
        if let Some(dir) = self.find_skill_dir(name) {
            let path = dir.join("SKILL.md");
            if path.exists() {
                let raw = fs::read_to_string(&path)?;
                let (mut frontmatter, body) = parse_frontmatter(&raw);

                frontmatter.insert(
                    serde_yaml::Value::String("lifecycle".into()),
                    serde_yaml::Value::String(
                        serde_yaml::to_string(&lifecycle)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    ),
                );

                let new_yaml = serde_yaml::to_string(&frontmatter)
                    .map_err(|e| SkillError::InvalidFormat(e.to_string()))?;
                let new_content = format!("---\n{}---\n{}", new_yaml, body);
                fs::write(&path, new_content)?;
                return Ok(());
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    /// Patch a skill by replacing text.
    pub fn patch(&self, name: &str, old_string: &str, new_string: &str) -> Result<(), SkillError> {
        let mut content = self.load(name)?;
        if !content.raw.contains(old_string) {
            return Err(SkillError::InvalidFormat(format!(
                "old_string not found in skill '{}'",
                name
            )));
        }
        content.raw = content.raw.replacen(old_string, new_string, 1);
        let (frontmatter, body) = parse_frontmatter(&content.raw);
        let mut updated = SkillContent {
            name: content.name.clone(),
            description: content.description.clone(),
            triggers: content.triggers.clone(),
            lifecycle: content.lifecycle,
            source: content.source,
            created_by: content.created_by.clone(),
            body,
            raw: content.raw.clone(),
        };

        // Update description and triggers from frontmatter
        if let Some(desc) = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from)
        {
            updated.description = desc;
        }
        if let Some(triggers) = frontmatter
            .get("triggers")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        {
            updated.triggers = triggers;
        }

        self.save(name, &updated)
    }

    /// Patch a skill by replacing **every** occurrence of `old_string` with
    /// `new_string`. Fails if `old_string` is not present.
    pub fn patch_all(
        &self,
        name: &str,
        old_string: &str,
        new_string: &str,
    ) -> Result<(), SkillError> {
        let mut content = self.load(name)?;
        if !content.raw.contains(old_string) {
            return Err(SkillError::InvalidFormat(format!(
                "old_string not found in skill '{}'",
                name
            )));
        }
        content.raw = content.raw.replace(old_string, new_string);
        let (frontmatter, body) = parse_frontmatter(&content.raw);
        let mut updated = SkillContent {
            name: content.name.clone(),
            description: content.description.clone(),
            triggers: content.triggers.clone(),
            lifecycle: content.lifecycle,
            source: content.source,
            created_by: content.created_by.clone(),
            body,
            raw: content.raw.clone(),
        };

        if let Some(desc) = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from)
        {
            updated.description = desc;
        }
        if let Some(triggers) = frontmatter
            .get("triggers")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        {
            updated.triggers = triggers;
        }

        self.save(name, &updated)
    }

    /// Write an additional file inside a skill's directory.
    pub fn write_file(
        &self,
        skill_name: &str,
        path: &str,
        content: &str,
    ) -> Result<(), SkillError> {
        let dir = self
            .find_skill_dir(skill_name)
            .ok_or_else(|| SkillError::NotFound(skill_name.to_string()))?;
        let file_path = dir.join(path.trim_start_matches('/'));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, content)?;
        Ok(())
    }

    /// Remove a file from a skill's directory.
    pub fn remove_file(&self, skill_name: &str, path: &str) -> Result<(), SkillError> {
        let dir = self
            .find_skill_dir(skill_name)
            .ok_or_else(|| SkillError::NotFound(skill_name.to_string()))?;
        let file_path = dir.join(path.trim_start_matches('/'));
        if file_path.exists() {
            fs::remove_file(&file_path)?;
            Ok(())
        } else {
            Err(SkillError::NotFound(format!(
                "file '{}' in skill '{}'",
                path, skill_name
            )))
        }
    }

    /// Find skills matching a query string.
    pub fn find_matching(&self, query: &str, threshold: f64) -> Result<Vec<SkillContent>, SkillError> {
        let all = self.list()?;
        let query_lower = query.to_lowercase();
        let query_words: HashSet<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, String)> = Vec::new();
        for meta in &all {
            let score = compute_match_score(query_lower.as_str(), &query_words, meta);
            if score >= threshold {
                scored.push((score, meta.name.clone()));
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut results = Vec::new();
        for (_, name) in scored {
            if let Ok(content) = self.load(&name) {
                results.push(content);
            }
        }
        Ok(results)
    }

    /// Read the `pinned` field from a SKILL.md frontmatter.
    fn read_pinned_from_frontmatter(&self, skill_md: &Path) -> Option<bool> {
        let raw = fs::read_to_string(skill_md).ok()?;
        let (frontmatter, _) = parse_frontmatter(&raw);
        frontmatter
            .get("pinned")
            .and_then(|v| v.as_bool())
    }
}

/// Compute a match score between query and skill metadata.
fn compute_match_score(query: &str, query_words: &HashSet<&str>, meta: &SkillMeta) -> f64 {
    let trigger_lower: Vec<String> = meta.triggers.iter().map(|t| t.to_lowercase()).collect();
    let desc_lower = meta.description.to_lowercase();
    let name_lower = meta.name.to_lowercase();

    let mut max_score = 0.0_f64;

    for trigger in &trigger_lower {
        if trigger == query {
            return 1.0;
        }
        if trigger.contains(query) || query.contains(trigger.as_str()) {
            max_score = max_score.max(0.85);
        }
        let trigger_words: HashSet<&str> = trigger.split_whitespace().collect();
        let overlap = query_words.intersection(&trigger_words).count();
        let union = query_words.union(&trigger_words).count();
        if union > 0 {
            let jaccard = overlap as f64 / union as f64;
            max_score = max_score.max(jaccard);
        }
    }

    if desc_lower.contains(query) || name_lower.contains(query) {
        max_score = max_score.max(0.5);
    }

    max_score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_skill() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        let skill = SkillContent {
            name: "debug-rust".to_string(),
            description: "Debug Rust errors".to_string(),
            triggers: vec!["rust".into(), "cargo".into(), "compiler error".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            created_by: None,
            body: "1. Read the error\n2. Identify cause\n".to_string(),
            raw: String::new(),
        };
        registry.save("debug-rust", &skill).unwrap();
        let loaded = registry.load("debug-rust").unwrap();
        assert_eq!(loaded.name, "debug-rust");
        assert_eq!(loaded.triggers.len(), 3);
        assert_eq!(loaded.source, Source::Auto);
    }

    #[test]
    fn list_skills() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        let skill = SkillContent {
            name: "test-skill".to_string(),
            description: "A test".to_string(),
            triggers: vec!["test".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
            created_by: None,
            body: "Do stuff".to_string(),
            raw: String::new(),
        };
        registry.save("test-skill", &skill).unwrap();
        let list = registry.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-skill");
    }

    #[test]
    fn find_matching_exact_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        let skill = SkillContent {
            name: "rust-debug".to_string(),
            description: "Debug Rust".to_string(),
            triggers: vec!["rust compiler error".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            created_by: None,
            body: "Steps...".to_string(),
            raw: String::new(),
        };
        registry.save("rust-debug", &skill).unwrap();
        let matches = registry.find_matching("rust compiler error", 0.5).unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn delete_skill() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        let skill = SkillContent {
            name: "to-delete".to_string(),
            description: "Delete me".to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
            created_by: None,
            body: "...".to_string(),
            raw: String::new(),
        };
        registry.save("to-delete", &skill).unwrap();
        registry.delete("to-delete").unwrap();
        assert!(registry.load("to-delete").is_err());
    }

    #[test]
    fn load_nonexistent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        assert!(registry.load("nope").is_err());
    }

    #[test]
    fn write_and_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillStorageRegistry::new(dir.path());
        let skill = SkillContent {
            name: "test-write".to_string(),
            description: "Test write".to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
            created_by: None,
            body: "...".to_string(),
            raw: String::new(),
        };
        registry.save("test-write", &skill).unwrap();

        registry
            .write_file("test-write", "src/helper.rs", "fn helper() {}\n")
            .unwrap();

        let file_path = dir
            .path()
            .join("curated")
            .join("test-write")
            .join("src")
            .join("helper.rs");
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "fn helper() {}\n");
    }
}