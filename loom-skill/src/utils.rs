//! Utility functions for skill processing.
//!
//! Provides frontmatter parsing, YAML utilities, and platform matching.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Parse YAML frontmatter from markdown content.
///
/// Returns `(frontmatter, body)` where frontmatter is a YAML mapping,
/// and body is the content after the closing `---`.
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

/// Split content into (YAML block, body). If content starts with "---\n" and has a second "---",
/// returns (yaml_slice, body); otherwise (full_content, "").
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
    let yaml_str = after_first[..sep].trim_start_matches('\n');
    let body = after_first[sep + DELIM.len()..].trim_start_matches('\n');
    (yaml_str, body)
}

/// Parses skill file content. Returns (Some(metadata), body) if front matter has name+description;
/// otherwise (None, full_content) for legacy single-file skills.
pub fn parse_skill_frontmatter(content: &str) -> (Option<SkillMetadata>, String) {
    let (yaml_str, body) = split_frontmatter(content);
    if body.is_empty() {
        return (None, content.to_string());
    }
    let metadata: SkillMetadata = match serde_yaml::from_str::<SkillMetadata>(yaml_str) {
        Ok(m) if !m.name.is_empty() => m,
        _ => return (None, content.to_string()),
    };
    (Some(metadata), body.to_string())
}

/// Skill metadata parsed from SKILL.md front matter.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillMetadata {
    pub name: String,
    /// Semantic version of the skill.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Optional platform requirements.
    #[serde(default, rename = "platforms")]
    pub platforms: Vec<String>,
    /// Optional tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl SkillMetadata {
    /// Check if the skill is compatible with the current platform.
    pub fn matches_platform(&self, current_platform: &str) -> bool {
        if self.platforms.is_empty() {
            return true;
        }
        self.platforms.iter().any(|p| {
            let p = p.to_lowercase();
            match current_platform {
                "darwin" => p == "macos" || p == "darwin",
                other => p == other,
            }
        })
    }
}

/// Platform identifiers for the 'platforms' frontmatter field.
pub const PLATFORM_MAP: &[(&str, &str)] = &[
    ("macos", "darwin"),
    ("linux", "linux"),
    ("windows", "win32"),
];

/// Check if a path contains excluded directories.
pub fn is_excluded_path(path: &Path) -> bool {
    const EXCLUDED_DIRS: &[&str] = &[
        ".git", ".github", ".hub", ".archive", ".venv", "venv", "node_modules",
        "site-packages", "__pycache__", ".tox", ".nox", ".pytest_cache",
        ".mypy_cache", ".ruff_cache",
    ];

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            if let Some(s) = name.to_str() {
                if EXCLUDED_DIRS.contains(&s) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_splits() {
        let s = "---\nname: foo\ndescription: bar\n---\n# Body";
        let (yaml, body) = parse_frontmatter(s);
        assert!(yaml.get("name").is_some());
        assert_eq!(body.trim(), "# Body");
    }

    #[test]
    fn parse_frontmatter_no_frontmatter() {
        let s = "# Just markdown";
        let (yaml, body) = parse_frontmatter(s);
        assert!(yaml.is_empty());
        assert_eq!(body, s);
    }

    #[test]
    fn parse_frontmatter_no_closing_delimiter() {
        let s = "---\nname: foo\nno closing";
        let (yaml, body) = parse_frontmatter(s);
        // Without closing delimiter, returns empty mapping (not valid frontmatter)
        assert!(yaml.is_empty());
        assert_eq!(body, s);
    }

    #[test]
    fn split_frontmatter_basic() {
        let s = "---\nname: foo\n---\n# Body";
        let (yaml, body) = split_frontmatter(s);
        assert!(yaml.contains("name"));
        assert_eq!(body.trim(), "# Body");
    }

    #[test]
    fn split_frontmatter_no_frontmatter() {
        let s = "# Just text";
        let (yaml, body) = split_frontmatter(s);
        assert_eq!(yaml, s);
        assert_eq!(body, "");
    }

    #[test]
    fn parse_skill_frontmatter_with_metadata() {
        let s = "---\nname: code-review\ndescription: Review code.\n---\n# Instructions";
        let (meta, body) = parse_skill_frontmatter(s);
        let meta = meta.unwrap();
        assert_eq!(meta.name, "code-review");
        assert_eq!(meta.description, "Review code.");
        assert!(body.contains("Instructions"));
    }

    #[test]
    fn parse_skill_frontmatter_no_metadata() {
        let s = "# Just markdown";
        let (meta, body) = parse_skill_frontmatter(s);
        assert!(meta.is_none());
        assert_eq!(body, s);
    }

    #[test]
    fn parse_skill_frontmatter_empty_name() {
        let s = "---\nname: \"\"\ndescription: test\n---\nbody";
        let (meta, body) = parse_skill_frontmatter(s);
        assert!(meta.is_none());
        assert_eq!(body, s);
    }

    #[test]
    fn skill_metadata_platforms() {
        let meta = SkillMetadata {
            name: "test".to_string(),
            version: None,
            description: "test".to_string(),
            platforms: vec!["macos".to_string(), "linux".to_string()],
            tags: vec![],
        };
        assert!(meta.matches_platform("darwin"));
        assert!(meta.matches_platform("linux"));
        assert!(!meta.matches_platform("win32"));
    }

    #[test]
    fn skill_metadata_empty_platforms_matches_all() {
        let meta = SkillMetadata {
            name: "test".to_string(),
            version: None,
            description: "test".to_string(),
            platforms: vec![],
            tags: vec![],
        };
        assert!(meta.matches_platform("darwin"));
        assert!(meta.matches_platform("linux"));
        assert!(meta.matches_platform("win32"));
    }

    #[test]
    fn is_excluded_path_git() {
        assert!(is_excluded_path(Path::new("/some/path/.git")));
        assert!(is_excluded_path(Path::new(".git")));
        assert!(is_excluded_path(Path::new("/some/.github/-actions")));
    }

    #[test]
    fn is_excluded_path_normal() {
        assert!(!is_excluded_path(Path::new("/some/project/src")));
        assert!(!is_excluded_path(Path::new("skills")));
    }
}