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

        let user_skills = env_config::home::loom_home().join("skills");
        for entry in scan_skills_dir(&user_skills, SkillSource::User) {
            if seen.insert(entry.metadata.name.clone()) {
                skills.push(entry);
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn parse_front_matter_no_second_delimiter() {
        let s = "---\nname: foo\nno closing";
        let (yaml, body) = parse_front_matter(s);
        assert_eq!(yaml, s);
        assert_eq!(body, "");
    }

    #[test]
    fn parse_front_matter_no_newline_after_first_delim() {
        let s = "---name: foo\n---\nbody";
        let (yaml, body) = parse_front_matter(s);
        assert_eq!(yaml, s);
        assert_eq!(body, "");
    }

    #[test]
    fn parse_front_matter_empty_string() {
        let (yaml, body) = parse_front_matter("");
        assert_eq!(yaml, "");
        assert_eq!(body, "");
    }

    #[test]
    fn parse_skill_front_matter_empty_name_returns_none() {
        let s = "---\nname: \"\"\ndescription: test\n---\nbody";
        let (meta, body) = parse_skill_front_matter(s);
        assert!(meta.is_none());
        assert_eq!(body, s);
    }

    #[test]
    fn parse_skill_front_matter_invalid_yaml_returns_none() {
        let s = "---\n[invalid yaml\n---\nbody";
        let (meta, body) = parse_skill_front_matter(s);
        assert!(meta.is_none());
        assert_eq!(body, s);
    }

    #[test]
    fn scan_skills_dir_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_skills_dir_nonexistent_dir() {
        let entries = scan_skills_dir(Path::new("/nonexistent/skills"), SkillSource::User);
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_skills_dir_finds_directory_skill() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: A test skill\n---\nInstructions here",
        )
        .unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.name, "my-skill");
        assert_eq!(entries[0].metadata.description, "A test skill");
        assert_eq!(entries[0].source, SkillSource::Project);
    }

    #[test]
    fn scan_skills_dir_finds_file_skill() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("quick-fix.md"),
            "---\nname: quick-fix\ndescription: Fix things\n---\nDo the fix",
        )
        .unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::User);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.name, "quick-fix");
        assert_eq!(entries[0].source, SkillSource::User);
    }

    #[test]
    fn scan_skills_dir_file_without_front_matter_uses_filename() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("raw-skill.md"), "# Just instructions").unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.name, "raw-skill");
    }

    #[test]
    fn scan_skills_dir_ignores_non_skill_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.json"), "{}").unwrap();
        std::fs::write(dir.path().join("data.csv"), "a,b").unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_skills_dir_directory_without_skill_md_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("no-skill");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("README.md"), "not a skill").unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert!(entries.is_empty());
    }

    #[test]
    fn skill_registry_discover_project_and_user() {
        let _g = ENV_LOCK.lock().unwrap();
        let project = tempfile::tempdir().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let project_skills = project.path().join(".loom").join("skills");
        std::fs::create_dir_all(&project_skills).unwrap();
        std::fs::write(
            project_skills.join("proj-skill.md"),
            "---\nname: proj-skill\ndescription: from project\n---\nbody",
        )
        .unwrap();

        let user_skills = loom_home.path().join("skills");
        std::fs::create_dir_all(&user_skills).unwrap();
        std::fs::write(
            user_skills.join("user-skill.md"),
            "---\nname: user-skill\ndescription: from user\n---\nbody",
        )
        .unwrap();

        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());
        let registry = SkillRegistry::discover(project.path(), &[]);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        assert_eq!(registry.list().len(), 2);
        let names: Vec<&str> = registry.list().iter().map(|e| e.metadata.name.as_str()).collect();
        assert!(names.contains(&"proj-skill"));
        assert!(names.contains(&"user-skill"));
    }

    #[test]
    fn skill_registry_project_wins_over_user_same_name() {
        let _g = ENV_LOCK.lock().unwrap();
        let project = tempfile::tempdir().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let project_skills = project.path().join(".loom").join("skills");
        std::fs::create_dir_all(&project_skills).unwrap();
        std::fs::write(
            project_skills.join("shared.md"),
            "---\nname: shared\ndescription: project version\n---\nproject",
        )
        .unwrap();

        let user_skills = loom_home.path().join("skills");
        std::fs::create_dir_all(&user_skills).unwrap();
        std::fs::write(
            user_skills.join("shared.md"),
            "---\nname: shared\ndescription: user version\n---\nuser",
        )
        .unwrap();

        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());
        let registry = SkillRegistry::discover(project.path(), &[]);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.list()[0].metadata.description, "project version");
        assert_eq!(registry.list()[0].source, SkillSource::Project);
    }

    #[test]
    fn skill_registry_extra_dirs() {
        let _g = ENV_LOCK.lock().unwrap();
        let project = tempfile::tempdir().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();

        std::fs::write(
            extra.path().join("extra-skill.md"),
            "---\nname: extra-skill\ndescription: from extra\n---\nbody",
        )
        .unwrap();

        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());
        let registry = SkillRegistry::discover(project.path(), &[extra.path().to_path_buf()]);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.list()[0].metadata.name, "extra-skill");
        assert_eq!(registry.list()[0].source, SkillSource::ProfileDir);
    }

    #[test]
    fn apply_filters_enabled_whitelist() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join(".loom").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        for name in &["alpha", "beta", "gamma"] {
            std::fs::write(
                skills_dir.join(format!("{}.md", name)),
                format!("---\nname: {}\ndescription: d\n---\nbody", name),
            )
            .unwrap();
        }

        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());
        let mut registry = SkillRegistry::discover(dir.path(), &[]);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        registry.apply_filters(Some(&["alpha".to_string(), "beta".to_string()]), None);
        let names: Vec<&str> = registry.list().iter().map(|e| e.metadata.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(!names.contains(&"gamma"));
    }

    #[test]
    fn apply_filters_disabled_blacklist() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join(".loom").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        for name in &["a", "b", "c"] {
            std::fs::write(
                skills_dir.join(format!("{}.md", name)),
                format!("---\nname: {}\ndescription: d\n---\nbody", name),
            )
            .unwrap();
        }

        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());
        let mut registry = SkillRegistry::discover(dir.path(), &[]);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        registry.apply_filters(None, Some(&["b".to_string()]));
        let names: Vec<&str> = registry.list().iter().map(|e| e.metadata.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(!names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn apply_filters_empty_enabled_keeps_all() {
        let mut registry = SkillRegistry { skills: vec![] };
        registry.apply_filters(Some(&[]), None);
        assert!(registry.list().is_empty());
    }

    #[test]
    fn apply_filters_empty_disabled_keeps_all() {
        let mut registry = SkillRegistry { skills: vec![] };
        registry.apply_filters(None, Some(&[]));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn available_skills_prompt_empty_registry() {
        let registry = SkillRegistry { skills: vec![] };
        assert_eq!(registry.available_skills_prompt(), "");
    }

    #[test]
    fn available_skills_prompt_with_skills() {
        let registry = SkillRegistry {
            skills: vec![
                SkillEntry {
                    metadata: SkillMetadata {
                        name: "code-review".to_string(),
                        description: "Review code quality".to_string(),
                    },
                    base_path: PathBuf::from("/tmp"),
                    skill_file: PathBuf::from("/tmp/SKILL.md"),
                    source: SkillSource::Project,
                },
                SkillEntry {
                    metadata: SkillMetadata {
                        name: "no-desc".to_string(),
                        description: String::new(),
                    },
                    base_path: PathBuf::from("/tmp"),
                    skill_file: PathBuf::from("/tmp/no-desc.md"),
                    source: SkillSource::User,
                },
            ],
        };
        let prompt = registry.available_skills_prompt();
        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("</available_skills>"));
        assert!(prompt.contains("code-review: Review code quality"));
        assert!(prompt.contains("no-desc: (no description)"));
    }

    #[test]
    fn load_skill_not_found() {
        let registry = SkillRegistry { skills: vec![] };
        let err = registry.load_skill("nonexistent").unwrap_err();
        assert!(matches!(err, SkillError::NotFound(_)));
    }

    #[test]
    fn load_skill_reads_body() {
        let dir = tempfile::tempdir().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\nname: my-skill\ndescription: test\n---\n# Full instructions\nDo things.",
        )
        .unwrap();

        let registry = SkillRegistry {
            skills: vec![SkillEntry {
                metadata: SkillMetadata {
                    name: "my-skill".to_string(),
                    description: "test".to_string(),
                },
                base_path: dir.path().to_path_buf(),
                skill_file: skill_file.clone(),
                source: SkillSource::Project,
            }],
        };
        let body = registry.load_skill("my-skill").unwrap();
        assert!(body.contains("Full instructions"));
        assert!(body.contains("Do things."));
    }

    #[test]
    fn load_skill_with_additional_resources() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: t\n---\nbody",
        )
        .unwrap();
        std::fs::write(skill_dir.join("extra.txt"), "extra content").unwrap();

        let registry = SkillRegistry {
            skills: vec![SkillEntry {
                metadata: SkillMetadata {
                    name: "my-skill".to_string(),
                    description: "t".to_string(),
                },
                base_path: skill_dir.clone(),
                skill_file: skill_dir.join("SKILL.md"),
                source: SkillSource::Project,
            }],
        };
        let body = registry.load_skill("my-skill").unwrap();
        assert!(body.contains("Additional resources"));
        assert!(body.contains("extra.txt"));
    }

    #[test]
    fn load_skill_file_skill_no_additional_resources() {
        let dir = tempfile::tempdir().unwrap();
        let skill_file = dir.path().join("standalone.md");
        std::fs::write(
            &skill_file,
            "---\nname: standalone\ndescription: t\n---\njust body",
        )
        .unwrap();

        let registry = SkillRegistry {
            skills: vec![SkillEntry {
                metadata: SkillMetadata {
                    name: "standalone".to_string(),
                    description: "t".to_string(),
                },
                base_path: dir.path().to_path_buf(),
                skill_file,
                source: SkillSource::User,
            }],
        };
        let body = registry.load_skill("standalone").unwrap();
        assert_eq!(body.trim(), "just body");
        assert!(!body.contains("Additional resources"));
    }

    #[test]
    fn skill_error_display() {
        let e = SkillError::NotFound("test".to_string());
        assert!(e.to_string().contains("test"));
        assert!(e.to_string().contains("not found"));
    }

    #[test]
    fn skill_source_equality() {
        assert_eq!(SkillSource::Project, SkillSource::Project);
        assert_ne!(SkillSource::Project, SkillSource::User);
        assert_ne!(SkillSource::User, SkillSource::ProfileDir);
    }

    #[test]
    fn scan_skills_dir_directory_skill_without_front_matter_uses_dirname() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-unnamed-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Just raw instructions").unwrap();
        let entries = scan_skills_dir(dir.path(), SkillSource::Project);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.name, "my-unnamed-skill");
    }
}
