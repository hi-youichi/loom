//! Skill discovery — scanning and loading skills from various locations.
//!
//! This module provides `SkillRegistry` for discovering skills from:
//! - Project `.loom/skills` directory
//! - User `~/.loom/skills` directory  
//! - `~/.loom/data/skills` (recursive, for auto-generated and evolved skills)
//! - Agent-specific skill directories

use crate::utils::{parse_skill_frontmatter, SkillMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SKILLS_SUBDIR: &str = ".loom/skills";
const SKILL_MD: &str = "SKILL.md";
const SKILL_EXTENSIONS: &[&str] = &["md", "txt", "markdown"];

/// Errors that can occur during skill discovery.
#[derive(Debug, Error)]
pub enum SkillDiscoveryError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("read skill {path}: {source}")]
    ReadFailed { path: PathBuf, source: std::io::Error },
    #[error("parse skill {path}: {reason}")]
    ParseFailed { path: PathBuf, reason: String },
}

/// A discovered skill entry with metadata and file location.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// The skill's metadata from frontmatter.
    pub metadata: SkillMetadata,
    /// The directory containing the skill.
    pub base_path: PathBuf,
    /// The path to the skill file (SKILL.md or standalone file).
    pub skill_file: PathBuf,
    /// Where this skill was discovered from.
    pub source: SkillSource,
}

/// The source location of a discovered skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    /// Skills in the project's `.loom/skills` directory.
    Project,
    /// Skills in the user's `~/.loom/skills` directory.
    User,
    /// Skills from profile-specific extra directories.
    ProfileDir,
    /// Skills bundled inside an agent's own directory.
    Agent,
    /// Skills in `~/.loom/data/skills/` (auto-generated and evolved).
    Data,
}

/// Registry of discovered skills.
#[derive(Debug)]
pub struct SkillRegistry {
    skills: Vec<SkillEntry>,
}

impl SkillRegistry {
    /// Discover skills from various locations.
    ///
    /// Priority (first wins):
    /// 1. Project `.loom/skills` directory
    /// 2. Extra profile directories
    /// 3. User `~/.loom/skills` directory
    /// 4. `~/.loom/data/skills/` (recursive)
    pub fn discover(working_folder: &Path, extra_dirs: &[PathBuf]) -> Result<Self, SkillDiscoveryError> {
        let mut seen = HashSet::new();
        let mut skills = Vec::new();

        // 1. Project skills
        let project_skills = working_folder.join(SKILLS_SUBDIR);
        for entry in scan_skills_dir(&project_skills, SkillSource::Project) {
            if seen.insert(entry.metadata.name.clone()) {
                skills.push(entry);
            }
        }

        // 2. Extra profile directories
        for dir in extra_dirs {
            for entry in scan_skills_dir(dir, SkillSource::ProfileDir) {
                if seen.insert(entry.metadata.name.clone()) {
                    skills.push(entry);
                }
            }
        }

        // 3. User skills
        let user_skills = env_config::home::loom_home().join("skills");
        for entry in scan_skills_dir(&user_skills, SkillSource::User) {
            if seen.insert(entry.metadata.name.clone()) {
                skills.push(entry);
            }
        }

        // 4. Data skills (recursive)
        let data_skills_dir = env_config::home::loom_home().join("data").join("skills");
        scan_skills_dir_recursive(&data_skills_dir, SkillSource::Data, &mut seen, &mut skills);

        Ok(Self { skills })
    }

    /// Add skills from an agent-specific directory.
    pub fn add_agent_skills(&mut self, dir: &Path) -> Result<(), SkillDiscoveryError> {
        let mut seen: HashSet<String> = self
            .skills
            .iter()
            .map(|e| e.metadata.name.clone())
            .collect();

        for entry in scan_skills_dir(dir, SkillSource::Agent) {
            if seen.insert(entry.metadata.name.clone()) {
                self.skills.push(entry);
            }
        }
        Ok(())
    }

    /// Apply enabled/disabled filters to the discovered skills.
    pub fn apply_filters(&mut self, enabled: Option<&[String]>, disabled: Option<&[String]>) {
        if let Some(en) = enabled {
            if !en.is_empty() {
                let set: HashSet<_> = en.iter().cloned().collect();
                self.skills.retain(|e| set.contains(&e.metadata.name));
            }
        }
        if let Some(dis) = disabled {
            if !dis.is_empty() {
                let set: HashSet<_> = dis.iter().cloned().collect();
                self.skills.retain(|e| !set.contains(&e.metadata.name));
            }
        }
    }

    /// Build the `<available_skills>` prompt block for system prompt injection.
    pub fn available_skills_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut lines = vec![
            "<available_skills>".to_string(),
            "When the user's task matches a known skill, use the `skill` tool to load its full instructions before proceeding.".to_string(),
            "".to_string(),
            "Available skills:".to_string(),
        ];
        for e in &self.skills {
            let desc = if e.metadata.description.is_empty() {
                "(no description)".to_string()
            } else {
                e.metadata.description.trim().to_string()
            };
            lines.push(format!("- {}: {}", e.metadata.name, desc));
        }
        lines.push("</available_skills>".to_string());
        lines.join("\n")
    }

    /// Load full skill content by name.
    pub fn load_skill(&self, name: &str) -> Result<String, SkillDiscoveryError> {
        let entry = self
            .skills
            .iter()
            .find(|e| e.metadata.name == name)
            .ok_or_else(|| SkillDiscoveryError::NotFound(name.to_string()))?;
        let content = std::fs::read_to_string(&entry.skill_file)
            .map_err(|source| SkillDiscoveryError::ReadFailed { path: entry.skill_file.clone(), source })?;
        let (_, body) = parse_skill_frontmatter(&content);
        let mut out = body;

        // Append info about additional resources if this is a directory skill
        if entry
            .skill_file
            .file_name()
            .map(|f| f == SKILL_MD)
            .unwrap_or(false)
        {
            if let Ok(rd) = std::fs::read_dir(&entry.base_path) {
                let others: Vec<String> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .filter(|n| n != SKILL_MD)
                    .collect();
                if !others.is_empty() {
                    out.push_str("\n\n## Additional resources\nThis skill includes these reference files (use `read` tool if needed):\n");
                    for o in others {
                        out.push_str("- ");
                        out.push_str(&o);
                        out.push('\n');
                    }
                }
            }
        }
        Ok(out)
    }

    /// Get all discovered skills.
    pub fn list(&self) -> &[SkillEntry] {
        &self.skills
    }

    /// Find a skill entry by name.
    pub fn find(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.iter().find(|e| e.metadata.name == name)
    }

    /// Create an empty registry (for testing).
    pub fn empty() -> Self {
        Self { skills: Vec::new() }
    }

    /// Create a registry from a pre-populated list of entries.
    pub fn from_entries(skills: Vec<SkillEntry>) -> Self {
        Self { skills }
    }
}

/// Scan a directory for skill entries.
fn scan_skills_dir(dir: &Path, source: SkillSource) -> Vec<SkillEntry> {
    let mut entries = Vec::new();
    let read_dir = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };
    for e in read_dir.flatten() {
        let path = e.path();
        if path.is_dir() {
            let skill_file = path.join(SKILL_MD);
            if skill_file.is_file() {
                let content = match std::fs::read_to_string(&skill_file) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let (meta_opt, _) = parse_skill_frontmatter(&content);
                let metadata = meta_opt.unwrap_or_else(|| SkillMetadata {
                    name: path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    version: None,
                    description: String::new(),
                    platforms: vec![],
                    tags: vec![],
                });
                entries.push(SkillEntry {
                    metadata,
                    base_path: path.clone(),
                    skill_file,
                    source,
                });
            }
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase());
            if let Some(ext) = ext {
                if !SKILL_EXTENSIONS.iter().any(|e| *e == ext) {
                    continue;
                }
            } else {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let (meta_opt, _) = parse_skill_frontmatter(&content);
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let metadata = meta_opt.unwrap_or_else(|| SkillMetadata {
                name: name.clone(),
                version: None,
                description: String::new(),
                platforms: vec![],
                tags: vec![],
            });
            entries.push(SkillEntry {
                metadata,
                base_path: path.parent().unwrap_or(dir).to_path_buf(),
                skill_file: path,
                source,
            });
        }
    }
    entries
}

/// Recursively scan a directory tree for skills.
fn scan_skills_dir_recursive(
    dir: &Path,
    source: SkillSource,
    seen: &mut HashSet<String>,
    skills: &mut Vec<SkillEntry>,
) {
    // First, scan direct entries at this level
    for entry in scan_skills_dir(dir, source) {
        if seen.insert(entry.metadata.name.clone()) {
            skills.push(entry);
        }
    }

    // Then recurse into sub-directories that are not skill directories themselves
    let read_dir = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return,
    };
    for e in read_dir.flatten() {
        let path = e.path();
        if path.is_dir() && !path.join(SKILL_MD).exists() {
            scan_skills_dir_recursive(&path, source, seen, skills);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_entry(name: &str, source: SkillSource) -> SkillEntry {
        let skill_dir = tempfile::tempdir().unwrap();
        let skill_file = skill_dir.path().join(SKILL_MD);
        std::fs::write(
            &skill_file,
            format!("---\nname: {}\ndescription: Test skill {}\n---\nBody content", name, name),
        )
        .unwrap();
        SkillEntry {
            metadata: SkillMetadata {
                name: name.to_string(),
                version: None,
                description: format!("Test skill {}", name),
                platforms: vec![],
                tags: vec![],
            },
            base_path: skill_dir.path().to_path_buf(),
            skill_file,
            source,
        }
    }

    #[test]
    fn available_skills_prompt_empty() {
        let registry = SkillRegistry::empty();
        assert_eq!(registry.available_skills_prompt(), "");
    }

    #[test]
    fn available_skills_prompt_with_skills() {
        let entries = vec![
            make_test_entry("code-review", SkillSource::Project),
            make_test_entry("test-driven", SkillSource::User),
        ];
        let registry = SkillRegistry::from_entries(entries);
        let prompt = registry.available_skills_prompt();
        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("</available_skills>"));
        assert!(prompt.contains("code-review: Test skill code-review"));
    }

    #[test]
    fn load_skill_not_found() {
        let registry = SkillRegistry::empty();
        let err = registry.load_skill("nonexistent").unwrap_err();
        assert!(matches!(err, SkillDiscoveryError::NotFound(_)));
    }

    #[test]
    fn apply_filters_enabled() {
        let entries = vec![
            make_test_entry("alpha", SkillSource::Project),
            make_test_entry("beta", SkillSource::Project),
            make_test_entry("gamma", SkillSource::Project),
        ];
        let mut registry = SkillRegistry::from_entries(entries);
        registry.apply_filters(Some(&["alpha".to_string(), "beta".to_string()]), None);
        let names: Vec<&str> = registry
            .list()
            .iter()
            .map(|e| e.metadata.name.as_str())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(!names.contains(&"gamma"));
    }

    #[test]
    fn apply_filters_disabled() {
        let entries = vec![
            make_test_entry("a", SkillSource::Project),
            make_test_entry("b", SkillSource::Project),
            make_test_entry("c", SkillSource::Project),
        ];
        let mut registry = SkillRegistry::from_entries(entries);
        registry.apply_filters(None, Some(&["b".to_string()]));
        let names: Vec<&str> = registry
            .list()
            .iter()
            .map(|e| e.metadata.name.as_str())
            .collect();
        assert!(names.contains(&"a"));
        assert!(!names.contains(&"b"));
        assert!(names.contains(&"c"));
    }
}