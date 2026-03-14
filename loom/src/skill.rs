//! Skill discovery and loading: scan .loom/skills (and ~/.loom/skills), parse SKILL.md
//! front matter, and provide registry for system prompt injection and the skill tool.

use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SKILLS_SUBDIR: &str = ".loom/skills";
const SKILL_MD: &str = "SKILL.md";
const SKILL_EXTENSIONS: &[&str] = &["md", "txt", "markdown"];

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("read skill {0}: {1}")]
    ReadFailed(PathBuf, std::io::Error),
    #[error("parse skill {0}: {1}")]
    ParseFailed(PathBuf, String),
}

/// Skill metadata parsed from SKILL.md front matter.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A discovered skill entry with metadata and file location.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub metadata: SkillMetadata,
    pub base_path: PathBuf,
    pub skill_file: PathBuf,
    pub source: SkillSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Project,
    User,
    ProfileDir,
}

/// Registry of discovered skills. Built by [`SkillRegistry::discover`].
#[derive(Debug)]
pub struct SkillRegistry {
    skills: Vec<SkillEntry>,
}

/// Splits content into (YAML block, body). If content starts with "---\n" and has a second "---",
/// returns (yaml_slice, body); otherwise (full_content, "").
fn parse_front_matter(content: &str) -> (&str, &str) {
    const DELIM: &str = "---";
    if !content.starts_with(DELIM) {
        return (content, "");
    }
    let rest = match content.get(DELIM.len()..) {
        Some(r) => r,
        None => return (content, ""),
    };
    if !rest.starts_with('\n') {
        return (content, "");
    }
    let after_first = &rest[1..];
    let sep = match after_first.find(DELIM) {
        Some(i) => i,
        None => return (content, ""),
    };
    let yaml_str = after_first[..sep].trim_start_matches('\n');
    let body = after_first[sep + DELIM.len()..].trim_start_matches('\n');
    (yaml_str, body)
}

/// Parses skill file content. Returns (Some(metadata), body) if front matter has name+description;
/// otherwise (None, full_content) for legacy single-file skills.
fn parse_skill_front_matter(content: &str) -> (Option<SkillMetadata>, String) {
    let (yaml_str, body) = parse_front_matter(content);
    if body.is_empty() {
        return (None, content.to_string());
    }
    let metadata: SkillMetadata = match serde_yaml::from_str::<SkillMetadata>(yaml_str) {
        Ok(m) if !m.name.is_empty() => m,
        _ => return (None, content.to_string()),
    };
    (Some(metadata), body.to_string())
}

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
                let (meta_opt, _body) = parse_skill_front_matter(&content);
                let metadata = meta_opt.unwrap_or_else(|| SkillMetadata {
                    name: path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    description: String::new(),
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
            let (meta_opt, _) = parse_skill_front_matter(&content);
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let metadata = meta_opt.unwrap_or_else(|| SkillMetadata {
                name: name.clone(),
                description: String::new(),
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

impl SkillRegistry {
    /// Discovers skills: project `.loom/skills`, then profile extra dirs, then user `~/.loom/skills`.
    /// Later sources do not override same name (first wins).
    pub fn discover(working_folder: &Path, extra_dirs: &[PathBuf]) -> Self {
        let mut seen = HashSet::new();
        let mut skills = Vec::new();

        let project_skills = working_folder.join(SKILLS_SUBDIR);
        for entry in scan_skills_dir(&project_skills, SkillSource::Project) {
            if seen.insert(entry.metadata.name.clone()) {
                skills.push(entry);
            }
        }

        for dir in extra_dirs {
            for entry in scan_skills_dir(dir, SkillSource::ProfileDir) {
                if seen.insert(entry.metadata.name.clone()) {
                    skills.push(entry);
                }
            }
        }

        if let Ok(home) = std::env::var("HOME") {
            let user_skills = PathBuf::from(home).join(SKILLS_SUBDIR);
            for entry in scan_skills_dir(&user_skills, SkillSource::User) {
                if seen.insert(entry.metadata.name.clone()) {
                    skills.push(entry);
                }
            }
        }

        Self { skills }
    }

    /// Applies enabled/disabled filters. If enabled is non-empty, only those names are kept;
    /// then any in disabled are removed.
    pub fn apply_filters(
        &mut self,
        enabled: Option<&[String]>,
        disabled: Option<&[String]>,
    ) {
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

    /// Builds the `<available_skills>` prompt block for system prompt injection.
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

    /// Loads full skill content by name (body after front matter). For directory skills,
    /// appends a note listing other files in the same directory.
    pub fn load_skill(&self, name: &str) -> Result<String, SkillError> {
        let entry = self
            .skills
            .iter()
            .find(|e| e.metadata.name == name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
        let content = std::fs::read_to_string(&entry.skill_file)
            .map_err(|e| SkillError::ReadFailed(entry.skill_file.clone(), e))?;
        let (_, body) = parse_skill_front_matter(&content);
        let mut out = body;
        if entry.skill_file.file_name().map(|f| f == SKILL_MD).unwrap_or(false) {
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

    pub fn list(&self) -> &[SkillEntry] {
        &self.skills
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_front_matter_splits() {
        let s = "---\nname: foo\ndescription: bar\n---\n# Body";
        let (yaml, body) = parse_front_matter(s);
        assert!(yaml.contains("name"));
        assert_eq!(body.trim(), "# Body");
    }

    #[test]
    fn parse_skill_front_matter_metadata() {
        let s = "---\nname: code-review\ndescription: Review code.\n---\n# Instructions";
        let (meta, body) = parse_skill_front_matter(s);
        let meta = meta.unwrap();
        assert_eq!(meta.name, "code-review");
        assert_eq!(meta.description, "Review code.");
        assert!(body.contains("Instructions"));
    }

    #[test]
    fn parse_skill_no_front_matter() {
        let s = "# Just markdown";
        let (meta, body) = parse_skill_front_matter(s);
        assert!(meta.is_none());
        assert_eq!(body, s);
    }
}
