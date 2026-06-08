use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Active,
    Stale,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Auto,
    Manual,
    Evolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub lifecycle: Lifecycle,
    pub source: Source,
    pub triggers: Vec<String>,
    pub created_at: Option<String>,
    pub last_used: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContent {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub lifecycle: Lifecycle,
    pub source: Source,
    pub body: String,
    pub raw: String,
}

#[derive(Debug, thiserror::Error)]
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

pub struct SkillRegistry {
    base_dir: PathBuf,
}

impl SkillRegistry {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn default_path() -> PathBuf {
        env_config::home::loom_home().join("data").join("skills")
    }

    fn skill_dir(&self, source: Source, name: &str) -> PathBuf {
        let subdir = match source {
            Source::Auto => "auto",
            Source::Manual => "curated",
            Source::Evolved => "evolved",
        };
        self.base_dir.join(subdir).join(name)
    }

    fn skill_file_path(&self, source: Source, name: &str) -> PathBuf {
        self.skill_dir(source, name).join("SKILL.md")
    }

    pub fn list(&self) -> Result<Vec<SkillMeta>, SkillError> {
        let mut skills = Vec::new();
        for source in [Source::Auto, Source::Manual, Source::Evolved] {
            let subdir = match source {
                Source::Auto => "auto",
                Source::Manual => "curated",
                Source::Evolved => "evolved",
            };
            let dir = self.base_dir.join(subdir);
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let skill_path = entry.path().join("SKILL.md");
                    if skill_path.exists() {
                        if let Ok(content) = self.load_from_path(&skill_path) {
                            skills.push(SkillMeta {
                                name: content.name.clone(),
                                description: content.description.clone(),
                                lifecycle: content.lifecycle,
                                source,
                                triggers: content.triggers,
                                created_at: None,
                                last_used: None,
                                pinned: false,
                            });
                        }
                    }
                }
            }
        }
        Ok(skills)
    }

    pub fn load(&self, name: &str) -> Result<SkillContent, SkillError> {
        for source in [Source::Auto, Source::Manual, Source::Evolved] {
            let path = self.skill_file_path(source, name);
            if path.exists() {
                return self.load_from_path(&path);
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    fn load_from_path(&self, path: &Path) -> Result<SkillContent, SkillError> {
        let raw = fs::read_to_string(path)?;
        let raw_owned = raw.clone();
        let (frontmatter, body) = parse_skill_frontmatter(&raw)?;

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

        Ok(SkillContent {
            name,
            description,
            triggers,
            lifecycle,
            source,
            body,
            raw: raw_owned,
        })
    }

    pub fn save(&self, name: &str, content: &SkillContent) -> Result<(), SkillError> {
        let dir = self.skill_dir(content.source, name);
        fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");

        let frontmatter = serde_yaml::to_string(&serde_yaml::Value::Mapping({
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("name".into()),
                serde_yaml::Value::String(content.name.clone()),
            );
            map.insert(
                serde_yaml::Value::String("description".into()),
                serde_yaml::Value::String(content.description.clone()),
            );
            map.insert(
                serde_yaml::Value::String("triggers".into()),
                serde_yaml::Value::Sequence(
                    content
                        .triggers
                        .iter()
                        .map(|t| serde_yaml::Value::String(t.clone()))
                        .collect(),
                ),
            );
            map.insert(
                serde_yaml::Value::String("lifecycle".into()),
                serde_yaml::Value::String(
                    serde_yaml::to_string(&content.lifecycle)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                ),
            );
            map.insert(
                serde_yaml::Value::String("source".into()),
                serde_yaml::Value::String(
                    serde_yaml::to_string(&content.source)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                ),
            );
            map
        }))?;

        let file_content = format!("---\n{}---\n{}", frontmatter, content.body);
        fs::write(&path, file_content)?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<(), SkillError> {
        for source in [Source::Auto, Source::Manual, Source::Evolved] {
            let dir = self.skill_dir(source, name);
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
                return Ok(());
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }

    pub fn patch(&self, name: &str, old_string: &str, new_string: &str) -> Result<(), SkillError> {
        let mut content = self.load(name)?;
        if !content.raw.contains(old_string) {
            return Err(SkillError::InvalidFormat(format!(
                "old_string not found in skill '{}'",
                name
            )));
        }
        content.raw = content.raw.replacen(old_string, new_string, 1);
        let (frontmatter, body) = parse_skill_frontmatter(&content.raw)?;
        let updated = SkillContent {
            name: content.name.clone(),
            description: content.description.clone(),
            triggers: content.triggers.clone(),
            lifecycle: content.lifecycle,
            source: content.source,
            body,
            raw: content.raw.clone(),
        };
        let description = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let triggers: Option<Vec<String>> = frontmatter
            .get("triggers")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
        let updated = SkillContent {
            description: description.unwrap_or(updated.description),
            triggers: triggers.unwrap_or(updated.triggers),
            ..updated
        };
        self.save(name, &updated)
    }

    pub fn write_file(
        &self,
        skill_name: &str,
        path: &str,
        content: &str,
    ) -> Result<(), SkillError> {
        let skill = self.load(skill_name)?;
        let dir = self.skill_dir(skill.source, skill_name);
        let normalized = path.trim_start_matches('/').replace('/', std::path::MAIN_SEPARATOR_STR);
        let file_path = dir.join(&normalized);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, content)?;
        Ok(())
    }

    pub fn remove_file(&self, skill_name: &str, path: &str) -> Result<(), SkillError> {
        let skill = self.load(skill_name)?;
        let dir = self.skill_dir(skill.source, skill_name);
        let normalized = path.trim_start_matches('/').replace('/', std::path::MAIN_SEPARATOR_STR);
        let file_path = dir.join(&normalized);
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
}

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

fn parse_skill_frontmatter(content: &str) -> Result<(serde_yaml::Mapping, String), SkillError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Ok((serde_yaml::Mapping::new(), content.to_string()));
    }

    let after_first = &trimmed[3..];
    let rest = after_first.trim_start_matches(['\n', '\r']);
    if let Some(end) = rest.find("---") {
        let yaml_str = &rest[..end];
        let body = rest[end + 3..].trim_start_matches(['\n', '\r']).to_string();
        let mapping: serde_yaml::Mapping = serde_yaml::from_str(yaml_str)?;
        return Ok((mapping, body));
    }

    Ok((serde_yaml::Mapping::new(), content.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_skill() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "debug-rust".to_string(),
            description: "Debug Rust errors".to_string(),
            triggers: vec!["rust".into(), "cargo".into(), "compiler error".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
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
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "test-skill".to_string(),
            description: "A test".to_string(),
            triggers: vec!["test".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
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
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "rust-debug".to_string(),
            description: "Debug Rust".to_string(),
            triggers: vec!["rust compiler error".into()],
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
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
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "to-delete".to_string(),
            description: "Delete me".to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
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
        let registry = SkillRegistry::new(dir.path());
        assert!(registry.load("nope").is_err());
    }

    #[test]
    fn write_and_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "test-write".to_string(),
            description: "Test write".to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
            body: "...".to_string(),
            raw: String::new(),
        };
        registry.save("test-write", &skill).unwrap();

        // Write a file inside the skill directory
        registry
            .write_file("test-write", "src/helper.rs", "fn helper() {}\n")
            .unwrap();

        // Read it back
        let file_path = dir
            .path()
            .join("curated")
            .join("test-write")
            .join("src")
            .join("helper.rs");
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "fn helper() {}\n");

        // Write with absolute path stripping
        registry
            .write_file("test-write", "/README.md", "# Test\n")
            .unwrap();
        let readme = dir
            .path()
            .join("curated")
            .join("test-write")
            .join("README.md");
        assert_eq!(fs::read_to_string(&readme).unwrap(), "# Test\n");
    }

    #[test]
    fn write_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new(dir.path());
        let skill = SkillContent {
            name: "nested".to_string(),
            description: "Nested".to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Manual,
            body: "...".to_string(),
            raw: String::new(),
        };
        registry.save("nested", &skill).unwrap();

        // Write to a deeply nested path - parent dirs should be created
        registry
            .write_file("nested", "a/b/c/deep.rs", "struct Deep;\n")
            .unwrap();

        let deep_path = dir
            .path()
            .join("curated")
            .join("nested")
            .join("a")
            .join("b")
            .join("c")
            .join("deep.rs");
        assert!(deep_path.exists());
        assert_eq!(fs::read_to_string(&deep_path).unwrap(), "struct Deep;\n");
    }
}
