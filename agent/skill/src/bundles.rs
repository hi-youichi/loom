//! Skill bundles — load multiple skills at once via YAML definitions.
//!
//! Bundles are stored in `~/.anureo/skill-bundles/*.yaml`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub skills: Vec<String>,
    #[serde(default)]
    pub instruction: Option<String>,
}

pub struct BundleRegistry {
    bundles: HashMap<String, SkillBundle>,
    bundles_dir: PathBuf,
}

impl BundleRegistry {
    pub fn new(bundles_dir: &Path) -> Self {
        let mut reg = Self {
            bundles: HashMap::new(),
            bundles_dir: bundles_dir.to_path_buf(),
        };
        reg.reload();
        reg
    }

    pub fn reload(&mut self) {
        self.bundles.clear();
        if !self.bundles_dir.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&self.bundles_dir) {
            for e in entries.flatten() {
                let path = e.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "yaml" && ext != "yml" {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(bundle) = serde_yaml::from_str::<SkillBundle>(&content) {
                        let slug = slugify(&bundle.name);
                        self.bundles.insert(slug, bundle);
                    }
                }
            }
        }
    }

    pub fn get(&self, slug: &str) -> Option<&SkillBundle> {
        self.bundles.get(&slug.to_lowercase())
    }

    pub fn list(&self) -> Vec<&SkillBundle> {
        let mut bundles: Vec<&SkillBundle> = self.bundles.values().collect();
        bundles.sort_by(|a, b| a.name.cmp(&b.name));
        bundles
    }

    pub fn resolve_command(&self, cmd: &str) -> Option<&SkillBundle> {
        let slug = cmd.trim_start_matches('/').to_lowercase();
        self.get(&slug)
    }
}

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .replace([' ', '_'], "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Backend Dev"), "backend-dev");
        assert_eq!(slugify("test_driven"), "test-driven");
        assert_eq!(slugify("My Skill!@#"), "my-skill");
    }
}
