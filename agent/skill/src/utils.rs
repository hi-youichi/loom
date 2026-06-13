//! Utility functions for skill processing.
//!
//! Provides frontmatter parsing, YAML utilities, and platform matching.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReadinessStatus {
    Available,
    SetupNeeded(Vec<String>),
    Unsupported(String),
}

pub fn parse_frontmatter(content: &str) -> (serde_yaml::Mapping, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (serde_yaml::Mapping::new(), content.to_string());
    }

    let after_first = &trimmed[3..];
    let rest = after_first.trim_start_matches(['\n', '\r']);
    if let Some(end) = rest.find("---") {
        let yaml_str = &rest[..end];
        let body = rest[end + 3..].trim_start_matches(['\n', '\r']).to_string();
        match serde_yaml::from_str::<serde_yaml::Mapping>(yaml_str) {
            Ok(mapping) => return (mapping, body),
            Err(_) => return (serde_yaml::Mapping::new(), content.to_string()),
        }
    }

    (serde_yaml::Mapping::new(), content.to_string())
}

pub fn split_frontmatter(content: &str) -> (&str, &str) {
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
    (&content[..DELIM.len() + 1 + sep + DELIM.len()], &after_first[sep + DELIM.len()..])
}

/// Skill conditions for toolset-aware activation.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct SkillConditions {
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
    #[serde(default)]
    pub requires_toolsets: Vec<String>,
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
    #[serde(default)]
    pub requires_tools: Vec<String>,
}

/// Nested metadata block in SKILL.md frontmatter.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillMetadataBlock {
    #[serde(default)]
    pub conditions: SkillConditions,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub related_skills: Vec<String>,
    #[serde(default)]
    pub required_env_vars: Vec<EnvVarDecl>,
}

/// A declared required environment variable.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnvVarDecl {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
}

/// Prerequisites declared in frontmatter.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillPrerequisites {
    #[serde(default)]
    pub commands: Vec<String>,
}

/// Parsed skill metadata from SKILL.md frontmatter.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillMetadata {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "platforms")]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub category_desc: Option<String>,
    #[serde(default)]
    pub metadata: Option<SkillMetadataBlock>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub prerequisites: Option<SkillPrerequisites>,
}

impl SkillMetadata {
    pub fn conditions(&self) -> &SkillConditions {
        static EMPTY: SkillConditions = SkillConditions {
            fallback_for_toolsets: vec![],
            requires_toolsets: vec![],
            fallback_for_tools: vec![],
            requires_tools: vec![],
        };
        self.metadata.as_ref().map(|m| &m.conditions).unwrap_or(&EMPTY)
    }

    pub fn required_env_vars(&self) -> &[EnvVarDecl] {
        static EMPTY: &[EnvVarDecl] = &[];
        self.metadata.as_ref().map(|m| m.required_env_vars.as_slice()).unwrap_or(EMPTY)
    }

    pub fn readiness_status(&self) -> ReadinessStatus {
        let vars = self.required_env_vars();
        if vars.is_empty() {
            return ReadinessStatus::Available;
        }
        let missing: Vec<&EnvVarDecl> = vars
            .iter()
            .filter(|v| std::env::var(&v.name).is_err() && v.default.is_none())
            .collect();
        if missing.is_empty() {
            ReadinessStatus::Available
        } else {
            ReadinessStatus::SetupNeeded(missing.iter().map(|v| v.name.clone()).collect())
        }
    }

    pub fn matches_platform(&self, current_platform: &str) -> bool {
        if self.platforms.is_empty() {
            return true;
        }
        let normalized = current_platform.to_lowercase();
        self.platforms.iter().any(|p| {
            let p_lower = p.to_lowercase();
            p_lower == normalized
                || p_lower == "macos" && normalized == "darwin"
                || p_lower == "linux" && normalized == "linux"
                || p_lower == "windows" && normalized == "windows"
        })
    }
}

pub fn parse_skill_frontmatter(content: &str) -> (Option<SkillMetadata>, String) {
    let (mapping, body) = parse_frontmatter(content);
    if mapping.is_empty() {
        return (None, body);
    }
    match serde_yaml::from_value::<SkillMetadata>(serde_yaml::Value::Mapping(mapping)) {
        Ok(meta) => (Some(meta), body),
        Err(_) => (None, body),
    }
}

pub fn is_excluded_path(path: &Path) -> bool {
    const EXCLUDED_DIRS: &[&str] = &[
        ".git", ".github", ".hub", ".archive", ".venv", "venv",
        "node_modules", "site-packages", "__pycache__",
        ".tox", ".nox", ".pytest_cache", ".mypy_cache", ".ruff_cache",
    ];
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name.starts_with('.') || EXCLUDED_DIRS.contains(&name))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_basic() {
        let content = "---\nname: test\ndescription: desc\n---\nBody";
        let (fm, body) = parse_frontmatter(content);
        assert!(!fm.is_empty());
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_frontmatter_no_frontmatter() {
        let content = "Just body";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, "Just body");
    }

    #[test]
    fn parse_skill_frontmatter_basic() {
        let content = "---\nname: test\ndescription: desc\n---\nBody";
        let (meta_opt, _body) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert_eq!(meta.name, "test");
        assert_eq!(meta.description, "desc");
        assert!(meta.category.is_none());
        assert!(meta.conditions().fallback_for_toolsets.is_empty());
    }

    #[test]
    fn parse_skill_frontmatter_with_category() {
        let content = "---\nname: test\ndescription: desc\ncategory: research\ncategory_desc: Research tools\n---\nBody";
        let (meta_opt, _) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert_eq!(meta.category.as_deref(), Some("research"));
        assert_eq!(meta.category_desc.as_deref(), Some("Research tools"));
    }

    #[test]
    fn parse_skill_frontmatter_with_conditions() {
        let content = "---\nname: duckduckgo\ndescription: Fallback search\nmetadata:\n  conditions:\n    fallback_for_toolsets:\n      - web\n    requires_tools:\n      - bash\n---\nBody";
        let (meta_opt, _) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert_eq!(meta.conditions().fallback_for_toolsets, vec!["web"]);
        assert_eq!(meta.conditions().requires_tools, vec!["bash"]);
        assert!(meta.conditions().requires_toolsets.is_empty());
    }

    #[test]
    fn parse_skill_frontmatter_with_prerequisites() {
        let content = "---\nname: test\ndescription: desc\nprerequisites:\n  commands:\n    - docker\n---\nBody";
        let (meta_opt, _) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert_eq!(meta.prerequisites.unwrap().commands, vec!["docker"]);
    }

    #[test]
    fn parse_skill_frontmatter_with_triggers() {
        let content = "---\nname: test\ntriggers:\n  - foo\n  - bar\n---\nBody";
        let (meta_opt, _) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert_eq!(meta.triggers, vec!["foo", "bar"]);
    }

    #[test]
    fn parse_skill_frontmatter_without_category_backwards_compat() {
        let content = "---\nname: old-skill\ndescription: old\n---\nBody";
        let (meta_opt, _) = parse_skill_frontmatter(content);
        let meta = meta_opt.unwrap();
        assert!(meta.category.is_none());
        assert!(meta.category_desc.is_none());
        assert!(meta.conditions().fallback_for_toolsets.is_empty());
    }

    #[test]
    fn matches_platform_basic() {
        let meta = SkillMetadata {
            platforms: vec!["macos".into(), "linux".into()],
            ..Default::default()
        };
        assert!(meta.matches_platform("darwin"));
        assert!(meta.matches_platform("linux"));
        assert!(!meta.matches_platform("windows"));
    }

    #[test]
    fn matches_platform_empty_matches_all() {
        let meta = SkillMetadata::default();
        assert!(meta.matches_platform("darwin"));
        assert!(meta.matches_platform("windows"));
    }

    #[test]
    fn is_excluded_path_dirs() {
        assert!(is_excluded_path(Path::new(".git")));
        assert!(is_excluded_path(Path::new("node_modules")));
        assert!(!is_excluded_path(Path::new("my-skill")));
    }
}
