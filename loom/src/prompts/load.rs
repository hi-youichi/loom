//! Load agent prompts from a directory of YAML files and apply env overrides.
//!
//! **Canonical source**: Default prompt text lives in `loom/prompts/*.yaml`; they are
//! embedded at compile time and used when no `PROMPTS_DIR` or directory is present.
//! See [`load`], [`load_or_default`], [`default_from_embedded`], and [`LoadError`].

use std::path::Path;

use serde::Deserialize;

use super::{
    DupPromptsFile, GotPromptsFile, HelvePromptsFile, ReactPromptsFile, TotPromptsFile,
};

/// Embedded default YAML (canonical source: `loom/prompts/*.yaml`).
macro_rules! embed_prompt_yaml {
    ($name:literal) => {
        include_str!(concat!("../../prompts/", $name))
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
const DEFAULT_PROMPTS_DIR: &str = "prompts";

/// Returns the directory to load prompts from: `dir` if `Some`, else `PROMPTS_DIR` env, else `DEFAULT_PROMPTS_DIR`.
fn prompts_dir(dir: Option<&Path>) -> std::path::PathBuf {
    dir.map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::env::var("PROMPTS_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PROMPTS_DIR))
    })
}

/// Tries to read and parse a YAML file into `T`. Missing file or parse error returns `None` or error.
fn read_yaml_file<T>(dir: &Path, name: &str) -> Result<Option<T>, LoadError>
where
    T: for<'de> Deserialize<'de>,
{
    let path = dir.join(name);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(LoadError::ReadFile {
                path: path.display().to_string(),
                message: e.to_string(),
            });
        }
    };
    let value: T = serde_yaml::from_str(&content).map_err(|e| LoadError::ParseYaml {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(Some(value))
}

/// Applies env override for ReAct: `REACT_SYSTEM_PROMPT` overrides `system_prompt`.
fn apply_react_env(mut file: ReactPromptsFile) -> ReactPromptsFile {
    if let Ok(s) = std::env::var("REACT_SYSTEM_PROMPT") {
        file.system_prompt = Some(s);
    }
    file
}

/// Loads prompts from a directory: reads `react.yaml`, `tot.yaml`, `got.yaml`, `dup.yaml`, `helve.yaml`,
/// applies env overrides (e.g. `REACT_SYSTEM_PROMPT`), and returns an [`AgentPrompts`](super::resolve::AgentPrompts).
///
/// If `dir` is `None`, uses `PROMPTS_DIR` env or default `./prompts`. Missing files are ignored
/// (that pattern keeps code defaults). Only returns error when the directory is required but missing,
/// or when a present file fails to parse.
pub fn load(dir: Option<&Path>) -> Result<super::resolve::AgentPrompts, LoadError> {
    let base = prompts_dir(dir);
    if !base.exists() || !base.is_dir() {
        return Err(LoadError::DirNotFound(base.display().to_string()));
    }

    let react = read_yaml_file::<ReactPromptsFile>(&base, REACT_FILE)?
        .map(apply_react_env)
        .unwrap_or_default();
    let tot = read_yaml_file::<TotPromptsFile>(&base, TOT_FILE)?
        .unwrap_or_default();
    let got = read_yaml_file::<GotPromptsFile>(&base, GOT_FILE)?
        .unwrap_or_default();
    let dup = read_yaml_file::<DupPromptsFile>(&base, DUP_FILE)?
        .unwrap_or_default();
    let helve = read_yaml_file::<HelvePromptsFile>(&base, HELVE_FILE)?
        .unwrap_or_default();

    Ok(super::resolve::AgentPrompts {
        react,
        tot,
        got,
        dup,
        helve,
    })
}

/// Returns default prompts by parsing the embedded YAML in `loom/prompts/*.yaml`.
///
/// This is the single source of truth for default prompt text; no duplicate strings in Rust.
/// Used by [`load_or_default`] when no directory is present and by tests.
pub fn default_from_embedded() -> super::resolve::AgentPrompts {
    let react: ReactPromptsFile = serde_yaml::from_str(EMBED_REACT).unwrap_or_default();
    let react = apply_react_env(react);
    let tot: TotPromptsFile = serde_yaml::from_str(EMBED_TOT).unwrap_or_default();
    let got: GotPromptsFile = serde_yaml::from_str(EMBED_GOT).unwrap_or_default();
    let dup: DupPromptsFile = serde_yaml::from_str(EMBED_DUP).unwrap_or_default();
    let helve: HelvePromptsFile = serde_yaml::from_str(EMBED_HELVE).unwrap_or_default();
    super::resolve::AgentPrompts {
        react,
        tot,
        got,
        dup,
        helve,
    }
}

/// Loads prompts from `dir` if the directory exists; otherwise returns default from embedded YAML.
///
/// Default text comes from `loom/prompts/*.yaml` (embedded), not from Rust const.
pub fn load_or_default(dir: Option<&Path>) -> super::resolve::AgentPrompts {
    load(dir).unwrap_or_else(|_| default_from_embedded())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load with a non-existent directory returns DirNotFound (when dir is explicitly given).
    #[test]
    fn load_nonexistent_dir_returns_error() {
        let result = load(Some(Path::new("/nonexistent_prompts_dir_12345")));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LoadError::DirNotFound(_)));
    }

    /// load_or_default with non-existent dir returns default from embedded YAML (empty react → code const).
    #[test]
    fn load_or_default_nonexistent_returns_default_from_embedded() {
        let p = load_or_default(Some(Path::new("/nonexistent_prompts_dir_12345")));
        let s = p.react_system_prompt();
        // Embedded react.yaml is empty by design; effective default is code const.
        assert_eq!(s, crate::agent::react::REACT_SYSTEM_PROMPT);
    }

    /// Load from a directory containing only react.yaml with system_prompt overrides that value.
    #[test]
    fn load_from_dir_with_react_yaml() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        let react_yaml = "system_prompt: \"From file.\"\n";
        std::fs::write(dir.join("react.yaml"), react_yaml).unwrap();
        let p = load(Some(dir)).unwrap();
        assert_eq!(p.react_system_prompt(), "From file.");
    }

    #[test]
    fn load_invalid_yaml_returns_parse_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        std::fs::write(dir.join("react.yaml"), "system_prompt: [not closed").unwrap();
        let err = load(Some(dir)).unwrap_err();
        assert!(matches!(err, LoadError::ParseYaml { .. }));
    }

    #[test]
    fn load_uses_prompts_dir_env_when_dir_is_none() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        std::fs::write(dir.join("react.yaml"), "system_prompt: \"From env dir\"").unwrap();
        let old = std::env::var("PROMPTS_DIR").ok();
        std::env::set_var("PROMPTS_DIR", dir);
        let p = load(None).unwrap();
        assert_eq!(p.react_system_prompt(), "From env dir");
        if let Some(v) = old {
            std::env::set_var("PROMPTS_DIR", v);
        } else {
            std::env::remove_var("PROMPTS_DIR");
        }
    }

    #[test]
    fn load_missing_files_are_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let p = load(Some(temp.path())).unwrap();
        assert_eq!(p.react_system_prompt(), crate::agent::react::REACT_SYSTEM_PROMPT);
        assert!(!p.tot_expand_system_addon().is_empty());
    }

}
