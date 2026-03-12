//! Load configuration from XDG `config.toml` and project `.env`, then apply to the process
//! environment with priority: **existing env > .env > XDG**.
//!
//! See workspace `docs/xdg_toml_config.md` for the design.

mod dotenv;
mod mcp_config;
mod xdg_toml;

pub use mcp_config::{
    discover_mcp_config_path, load_mcp_config_from_path, parse_mcp_config, McpConfigError,
    McpConfigFile, McpServerDef, McpServerEntry,
};

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Masks a key for logging: keeps first `prefix_len` and last `suffix_len` chars, middle becomes `***`.
pub fn mask_key(key: &str, prefix_len: usize, suffix_len: usize) -> String {
    let n = key.len();
    if n == 0 {
        return "***".to_string();
    }
    let p = prefix_len.min(n);
    let s = if n > p { suffix_len.min(n - p) } else { 0 };
    if p == 0 && s == 0 {
        return "***".to_string();
    }
    let prefix = &key[..p];
    let suffix = if s > 0 { &key[n - s..] } else { "" };
    format!("{}***{}", prefix, suffix)
}

/// Source of a config key that was applied or already set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    /// Already set in process environment (not overwritten).
    ExistingEnv,
    /// Loaded from project `.env`.
    Dotenv,
    /// Loaded from XDG `config.toml` `[env]`.
    Xdg,
}

/// One entry in the config load report (key masked, value not included).
#[derive(Clone, Debug)]
pub struct LoadedEntry {
    pub key_masked: String,
    pub source: ConfigSource,
}

/// Result of loading config: which keys were effective and from where (keys masked).
#[derive(Clone, Debug, Default)]
pub struct ConfigLoadReport {
    pub entries: Vec<LoadedEntry>,
    pub dotenv_path: Option<PathBuf>,
    pub xdg_path: Option<PathBuf>,
}

/// Paths of config files (for logging when load fails). Paths are as resolved (not necessarily canonical).
#[derive(Clone, Debug, Default)]
pub struct ConfigFilePaths {
    pub dotenv: Option<PathBuf>,
    pub xdg: Option<PathBuf>,
}

/// Returns paths that would be used for loading (without loading). Use when load fails to log what was tried.
pub fn config_file_paths(app_name: &str, override_dir: Option<&Path>) -> ConfigFilePaths {
    let dotenv = dotenv::env_file_path(override_dir);
    let xdg = xdg_toml::config_path(app_name).ok().flatten();
    ConfigFilePaths { dotenv, xdg }
}

impl ConfigLoadReport {
    /// Keys summary line only (for separate logging).
    pub fn keys_summary(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let source_str = |s: ConfigSource| match s {
            ConfigSource::ExistingEnv => "env",
            ConfigSource::Dotenv => ".env",
            ConfigSource::Xdg => "config.toml",
        };
        let line = self
            .entries
            .iter()
            .map(|e| format!("{} ({})", e.key_masked, source_str(e.source)))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("config keys: {}", line))
    }

    /// Human-readable summary for logging (keys masked). Prefer logging each path separately with `dotenv_path`/`xdg_path` and then `keys_summary()`.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        if let Some(p) = &self.dotenv_path {
            lines.push(format!("  .env: {}", p.display()));
        }
        if let Some(p) = &self.xdg_path {
            lines.push(format!("  config.toml: {}", p.display()));
        }
        if lines.is_empty() && self.entries.is_empty() {
            return "config: no .env or config.toml loaded".to_string();
        }
        let header = if lines.is_empty() {
            "config loaded (env only)".to_string()
        } else {
            format!("config loaded:\n{}", lines.join("\n"))
        };
        if let Some(keys) = self.keys_summary() {
            format!("{}\n  {}", header, keys)
        } else {
            header
        }
    }
}

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("xdg config path: {0}")]
    XdgPath(String),
    #[error("read xdg config: {0}")]
    XdgRead(std::io::Error),
    #[error("parse xdg toml: {0}")]
    XdgParse(#[from] toml::de::Error),
    #[error("read .env: {0}")]
    DotenvRead(std::io::Error),
}

/// Loads config from XDG `config.toml` and optional project `.env`, then sets environment
/// variables only for keys that are **not** already set (so existing env has highest priority).
///
/// Order of precedence when a key is missing in the process environment:
/// 1. Value from project `.env` (current directory or `override_dir` if given)
/// 2. Value from `$XDG_CONFIG_HOME/<app_name>/config.toml` `[env]` table
///
/// * `app_name`: e.g. `"loom"` — used for XDG path `~/.config/<app_name>/config.toml`.
/// * `override_dir`: if `Some`, look for `.env` in this directory instead of `std::env::current_dir()`.
pub fn load_and_apply(app_name: &str, override_dir: Option<&Path>) -> Result<(), LoadError> {
    let _ = load_and_apply_with_report(app_name, override_dir)?;
    Ok(())
}

/// Like `load_and_apply` but returns a report of which keys were applied and from where (keys masked).
pub fn load_and_apply_with_report(
    app_name: &str,
    override_dir: Option<&Path>,
) -> Result<ConfigLoadReport, LoadError> {
    let xdg_map = xdg_toml::load_env_map(app_name)?;
    let dotenv_map = dotenv::load_env_map(override_dir).map_err(LoadError::DotenvRead)?;

    let dotenv_path = dotenv::env_file_path(override_dir);
    let xdg_path = xdg_toml::config_path(app_name)?;

    let mut keys: std::collections::HashSet<String> = xdg_map.keys().cloned().collect();
    keys.extend(dotenv_map.keys().cloned());

    let mut entries = Vec::with_capacity(keys.len());

    for key in keys {
        let source = if std::env::var(&key).is_ok() {
            ConfigSource::ExistingEnv
        } else if dotenv_map.contains_key(&key) {
            ConfigSource::Dotenv
        } else {
            ConfigSource::Xdg
        };

        let value = if source == ConfigSource::ExistingEnv {
            None
        } else {
            dotenv_map.get(&key).or_else(|| xdg_map.get(&key)).cloned()
        };

        if let Some(v) = value {
            std::env::set_var(&key, v);
        }

        entries.push(LoadedEntry {
            key_masked: mask_key(&key, 3, 3),
            source,
        });
    }

    Ok(ConfigLoadReport {
        entries,
        dotenv_path,
        xdg_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn mask_key_keeps_prefix_suffix() {
        assert_eq!(mask_key("OPENAI_API_KEY", 3, 3), "OPE***KEY");
        assert_eq!(mask_key("GITLAB_TOKEN", 3, 3), "GIT***KEN");
        assert_eq!(mask_key("X", 3, 3), "X***");
        assert_eq!(mask_key("", 3, 3), "***");
    }

    fn restore_var(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn existing_env_wins() {
        env::set_var("CONFIG_TEST_EXISTING", "from_env");
        let _ = load_and_apply("loom", None);
        assert_eq!(env::var("CONFIG_TEST_EXISTING").as_deref(), Ok("from_env"));
        env::remove_var("CONFIG_TEST_EXISTING");
    }

    #[test]
    fn load_and_apply_no_config_ok() {
        let r = load_and_apply("config-crate-nonexistent-app-xyz", None::<&std::path::Path>);
        assert!(r.is_ok());
    }

    #[test]
    fn dotenv_overrides_xdg() {
        let xdg_dir = tempfile::tempdir().unwrap();
        let app_dir = xdg_dir.path().join("loom");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(
            app_dir.join("config.toml"),
            "[env]\nCONFIG_TEST_PRIORITY = \"from_xdg\"\n",
        )
        .unwrap();

        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dotenv_dir.path().join(".env"),
            "CONFIG_TEST_PRIORITY=from_dotenv\n",
        )
        .unwrap();

        let prev_xdg = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", xdg_dir.path());
        env::remove_var("CONFIG_TEST_PRIORITY");

        let _ = load_and_apply("loom", Some(dotenv_dir.path()));
        let val = env::var("CONFIG_TEST_PRIORITY").unwrap();
        env::remove_var("CONFIG_TEST_PRIORITY");
        restore_var("XDG_CONFIG_HOME", prev_xdg);

        assert_eq!(val, "from_dotenv");
    }

    #[test]
    fn xdg_applied_when_no_dotenv() {
        let xdg_dir = tempfile::tempdir().unwrap();
        let app_dir = xdg_dir.path().join("loom");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(
            app_dir.join("config.toml"),
            "[env]\nCONFIG_TEST_XDG_ONLY = \"from_xdg\"\n",
        )
        .unwrap();

        let empty_dir = tempfile::tempdir().unwrap();

        let prev_xdg = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", xdg_dir.path());
        env::remove_var("CONFIG_TEST_XDG_ONLY");

        let _ = load_and_apply("loom", Some(empty_dir.path()));
        let val = env::var("CONFIG_TEST_XDG_ONLY").unwrap();
        env::remove_var("CONFIG_TEST_XDG_ONLY");
        restore_var("XDG_CONFIG_HOME", prev_xdg);

        assert_eq!(val, "from_xdg");
    }

    #[test]
    fn dotenv_only_when_no_xdg() {
        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dotenv_dir.path().join(".env"),
            "CONFIG_TEST_DOTENV_ONLY=from_dotenv_only\n",
        )
        .unwrap();

        env::remove_var("CONFIG_TEST_DOTENV_ONLY");
        let _ = load_and_apply("config-crate-nonexistent-app-xyz", Some(dotenv_dir.path()));
        let val = env::var("CONFIG_TEST_DOTENV_ONLY").unwrap();
        env::remove_var("CONFIG_TEST_DOTENV_ONLY");

        assert_eq!(val, "from_dotenv_only");
    }

    #[test]
    fn invalid_xdg_toml_fails_with_xdg_parse_error() {
        let xdg_dir = tempfile::tempdir().unwrap();
        let app_dir = xdg_dir.path().join("loom");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("config.toml"), "invalid [[[\n").unwrap();

        let prev_xdg = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", xdg_dir.path());

        let result = load_and_apply("loom", None::<&std::path::Path>);
        restore_var("XDG_CONFIG_HOME", prev_xdg);

        assert!(matches!(result, Err(LoadError::XdgParse(_))));
    }
}
