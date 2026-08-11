//! Skill config variable system — extract and resolve config variables from skill frontmatter.
//!
//! Skills can declare config variables in their frontmatter:
//! ```yaml
//! metadata:
//!   config:
//!     - key: wiki.path
//!       description: Path to wiki directory
//!       default: "~/wiki"
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfigVarDecl {
    pub key: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillConfigBlock {
    #[serde(default)]
    pub config: Vec<ConfigVarDecl>,
}

pub fn extract_config_vars(frontmatter: &serde_yaml::Mapping) -> Vec<ConfigVarDecl> {
    let metadata = frontmatter
        .get(serde_yaml::Value::String("metadata".into()))
        .and_then(|v| v.as_mapping());

    let metadata = match metadata {
        Some(m) => m,
        None => return vec![],
    };

    let config = metadata
        .get(serde_yaml::Value::String("config".into()))
        .and_then(|v| v.as_sequence());

    match config {
        Some(seq) => seq
            .iter()
            .filter_map(|v| serde_yaml::from_value::<ConfigVarDecl>(v.clone()).ok())
            .collect(),
        None => vec![],
    }
}

pub fn resolve_config_values(
    decls: &[ConfigVarDecl],
    config: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut resolved = HashMap::new();
    for decl in decls {
        let value = config
            .get(&decl.key)
            .cloned()
            .or_else(|| decl.default.clone())
            .unwrap_or_default();
        resolved.insert(decl.key.clone(), expand_value(&value));
    }
    resolved
}

fn expand_value(value: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let expanded = if cfg!(windows) {
        value.replace("%USERPROFILE%", &home)
    } else {
        shellexpand::tilde(value).to_string()
    };
    expanded
}

pub fn inject_config_into_content(content: &str, resolved: &HashMap<String, String>) -> String {
    if resolved.is_empty() {
        return content.to_string();
    }

    let mut lines = vec!["[Skill Configuration]".to_string()];
    for (key, value) in resolved {
        lines.push(format!("- {}: {}", key, value));
    }
    lines.push(String::new());

    format!("{}\n{}", lines.join("\n"), content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapping(yaml: &str) -> serde_yaml::Mapping {
        let doc: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        doc.as_mapping().unwrap().clone()
    }

    #[test]
    fn extract_config_vars_empty_frontmatter() {
        let fm = serde_yaml::Mapping::new();
        let vars = extract_config_vars(&fm);
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_config_vars_no_metadata() {
        let fm = make_mapping("name: test\n");
        let vars = extract_config_vars(&fm);
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_config_vars_with_metadata_no_config() {
        let fm = make_mapping("metadata:\n  tags:\n    - test\n");
        let vars = extract_config_vars(&fm);
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_config_vars_with_config() {
        let fm = make_mapping(
            "metadata:\n  config:\n    - key: wiki.path\n      description: Wiki path\n      default: \"~/wiki\"\n",
        );
        let vars = extract_config_vars(&fm);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].key, "wiki.path");
        assert_eq!(vars[0].description.as_deref(), Some("Wiki path"));
        assert_eq!(vars[0].default.as_deref(), Some("~/wiki"));
    }

    #[test]
    fn extract_config_vars_multiple() {
        let fm = make_mapping(
            "metadata:\n  config:\n    - key: a\n      description: Var A\n    - key: b\n      description: Var B\n      default: \"x\"\n",
        );
        let vars = extract_config_vars(&fm);
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn extract_config_vars_with_prompt() {
        let fm = make_mapping(
            "metadata:\n  config:\n    - key: api.key\n      description: API key\n      prompt: Enter your API key\n",
        );
        let vars = extract_config_vars(&fm);
        assert_eq!(vars[0].prompt.as_deref(), Some("Enter your API key"));
    }

    #[test]
    fn resolve_config_values_from_config_map() {
        let decls = vec![ConfigVarDecl {
            key: "port".into(),
            description: Some("Server port".into()),
            default: Some("8080".into()),
            prompt: None,
        }];
        let mut config = HashMap::new();
        config.insert("port".into(), "3000".into());
        let resolved = resolve_config_values(&decls, &config);
        assert_eq!(resolved.get("port").unwrap(), "3000");
    }

    #[test]
    fn resolve_config_values_falls_back_to_default() {
        let decls = vec![ConfigVarDecl {
            key: "port".into(),
            description: Some("Server port".into()),
            default: Some("8080".into()),
            prompt: None,
        }];
        let config = HashMap::new();
        let resolved = resolve_config_values(&decls, &config);
        assert_eq!(resolved.get("port").unwrap(), "8080");
    }

    #[test]
    fn resolve_config_values_no_default_yields_empty() {
        let decls = vec![ConfigVarDecl {
            key: "missing".into(),
            description: Some("No default".into()),
            default: None,
            prompt: None,
        }];
        let config = HashMap::new();
        let resolved = resolve_config_values(&decls, &config);
        assert_eq!(resolved.get("missing").unwrap(), "");
    }

    #[test]
    fn inject_config_into_content_prepends_block() {
        let content = "Step 1: Do stuff";
        let mut config = HashMap::new();
        config.insert("key1".into(), "val1".into());
        let result = inject_config_into_content(content, &config);
        assert!(result.starts_with("[Skill Configuration]"));
        assert!(result.contains("- key1: val1"));
        assert!(result.contains("Step 1: Do stuff"));
    }

    #[test]
    fn inject_config_empty_returns_original() {
        let content = "Original content";
        let config = HashMap::new();
        let result = inject_config_into_content(content, &config);
        assert_eq!(result, "Original content");
    }
}
