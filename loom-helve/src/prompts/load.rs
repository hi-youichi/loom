//! Load agent prompts from a directory of YAML files.
//!
//! **Canonical source**: Default prompt text lives in `loom/prompts/*.yaml`; they are
//! embedded at compile time and used when no `PROMPTS_DIR` or directory is present.
//! See [`load`], [`load_or_default`], [`default_from_embedded`], and [`LoadError`].

use std::path::Path;

use serde::Deserialize;

use super::{DupPromptsFile, GotPromptsFile, HelvePromptsFile, ReactPromptsFile, TotPromptsFile};

/// Embedded default YAML (canonical source: `loom/prompts/*.yaml`).
macro_rules! embed_prompt_yaml {
    ($name:literal) => {
        include_str!(concat!("../../../loom/prompts/", $name))
    };
}
const EMBED_REACT: &str = embed_prompt_yaml!("react.yaml");
const EMBED_TOT: &str = embed_prompt_yaml!("tot.yaml");
const EMBED_GOT: &str = embed_prompt_yaml!("got.yaml");
const EMBED_DUP: &str = embed_prompt_yaml!("dup.yaml");
const EMBED_HELVE: &str = embed_prompt_yaml!("helve.yaml");

/// Error when loading prompts from a directory (missing dir, invalid YAML).
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("prompts directory not found or not readable: {0}")]
    DirNotFound(String),
    #[error("failed to read prompts file {path}: {message}")]
    ReadFile { path: String, message: String },
    #[error("failed to parse YAML in {path}: {message}")]
    ParseYaml { path: String, message: String },
}

/// Names of YAML files under the prompts directory (one per pattern).
const REACT_FILE: &str = "react.yaml";
const TOT_FILE: &str = "tot.yaml";
const GOT_FILE: &str = "got.yaml";
const DUP_FILE: &str = "dup.yaml";
const HELVE_FILE: &str = "helve.yaml";

/// Default directory name when `PROMPTS_DIR` is not set.
#[allow(dead_code)]
const DEFAULT_PROMPTS_DIR: &str = "prompts";

/// Loads agent prompts from a directory of YAML files.
///
/// # Errors
///
/// Returns [`LoadError::DirNotFound`] if the directory doesn't exist.
pub fn load(dir: &Path) -> Result<AgentPrompts, LoadError> {
    if !dir.is_dir() {
        return Err(LoadError::DirNotFound(dir.display().to_string()));
    }

    Ok(AgentPrompts {
        react: load_file(dir, REACT_FILE).unwrap_or_default(),
        tot: load_file(dir, TOT_FILE).unwrap_or_default(),
        got: load_file(dir, GOT_FILE).unwrap_or_default(),
        dup: load_file(dir, DUP_FILE).unwrap_or_default(),
        helve: load_file(dir, HELVE_FILE).unwrap_or_default(),
    })
}

/// Loads from a directory if it exists, otherwise falls back to embedded defaults.
pub fn load_or_default(dir: &Path) -> AgentPrompts {
    load(dir).unwrap_or_else(|_| default_from_embedded())
}

/// Returns prompt materials from embedded defaults (canonical source).
pub fn default_from_embedded() -> AgentPrompts {
    AgentPrompts {
        react: parse_yaml(EMBED_REACT).unwrap_or_default(),
        tot: parse_yaml(EMBED_TOT).unwrap_or_default(),
        got: parse_yaml(EMBED_GOT).unwrap_or_default(),
        dup: parse_yaml(EMBED_DUP).unwrap_or_default(),
        helve: parse_yaml(EMBED_HELVE).unwrap_or_default(),
    }
}

fn load_file<T>(dir: &Path, filename: &str) -> Result<T, LoadError>
where
    T: for<'de> Deserialize<'de>,
{
    let path = dir.join(filename);
    let content = std::fs::read_to_string(&path).map_err(|e| LoadError::ReadFile {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    parse_yaml_with_context(&content, &path).map_err(|e| LoadError::ParseYaml {
        path: path.display().to_string(),
        message: e,
    })
}

fn parse_yaml<T>(yaml: &str) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    parse_yaml_with_context(yaml, Path::new("<embedded>")).ok()
}

fn parse_yaml_with_context<T>(yaml: &str, path: &Path) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_yaml::from_str(yaml).map_err(|e| format!("{}: {}", path.display(), e))
}

/// Loaded YAML prompt materials for all agent patterns.
///
/// Build via [`load`] or [`load_or_default`].
/// ReAct prompt materials are loaded here but assembled elsewhere.
#[derive(Clone, Debug, Default)]
pub struct AgentPrompts {
    pub react: ReactPromptsFile,
    pub tot: TotPromptsFile,
    pub got: GotPromptsFile,
    pub dup: DupPromptsFile,
    pub helve: HelvePromptsFile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_from_embedded_returns_react_system_prompt() {
        let prompts = default_from_embedded();
        assert!(!prompts.react.system_prompt.unwrap_or_default().is_empty());
    }

    #[test]
    fn load_nonexistent_directory_returns_err() {
        let result = load(Path::new("/nonexistent/prompts"));
        assert!(matches!(result, Err(LoadError::DirNotFound(_))));
    }

    #[test]
    fn load_or_default_fallback_to_embedded() {
        let prompts = load_or_default(Path::new("/nonexistent/prompts"));
        assert!(!prompts.react.system_prompt.unwrap_or_default().is_empty());
    }

    #[test]
    fn embedded_yaml_constants_are_not_empty() {
        assert!(!EMBED_REACT.is_empty());
        assert!(!EMBED_TOT.is_empty());
        assert!(!EMBED_GOT.is_empty());
        assert!(!EMBED_DUP.is_empty());
        assert!(!EMBED_HELVE.is_empty());
    }
}