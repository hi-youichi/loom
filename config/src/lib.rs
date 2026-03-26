//! Load configuration from `~/.loom/config.toml` and project `.env`, then apply to the process
//! environment with priority: **existing env > .env > providers > config.toml `[env]`**.

mod dotenv;
pub mod home;
mod mcp_config;
mod xdg_toml;

#[cfg(feature = "tracing-init")]
pub mod tracing_init;

pub use mcp_config::{
    discover_mcp_config_path, load_mcp_config_from_path, parse_mcp_config, McpConfigError,
    McpConfigFile, McpServerDef, McpServerEntry,
};
pub use xdg_toml::{load_full_config, FullConfig, ProviderDef};

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

/// Masks a secret value for logging: keeps first 2 and last 2 characters, middle becomes `***`.
pub fn mask_value(value: &str) -> String {
    const PREFIX_LEN: usize = 2;
    const SUFFIX_LEN: usize = 2;
    let n = value.len();
    if n <= PREFIX_LEN + SUFFIX_LEN {
        return "***".to_string();
    }
    let prefix = &value[..PREFIX_LEN];
    let suffix = &value[n - SUFFIX_LEN..];
    format!("{}***{}", prefix, suffix)
}

/// Returns true if the key looks like a secret (e.g. API key, token, password).
/// Used to decide whether to mask the value in config summary; non-secret keys show value as-is.
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_uppercase();
    k.ends_with("_KEY")
        || k.ends_with("KEY") && k.len() <= 4
        || k.contains("_KEY_")
        || k.contains("TOKEN")
        || k.contains("SECRET")
        || k.contains("PASSWORD")
        || k.contains("CREDENTIAL")
        || k.starts_with("AUTH_")
        || k.ends_with("_AUTH")
        || k.contains("_AUTH_")
}

/// Source of a config key that was applied or already set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    /// Already set in process environment (not overwritten).
    ExistingEnv,
    /// Loaded from project `.env`.
    Dotenv,
    /// Loaded from an active `[[providers]]` entry in `config.toml`.
    Provider,
    /// Loaded from XDG `config.toml` `[env]`.
    Xdg,
}

/// One entry in the config load report (key plain; value masked if secret, else plain for display).
#[derive(Clone, Debug)]
pub struct LoadedEntry {
    pub key: String,
    /// Value for display: masked as `***` when [`is_secret_key`] is true, otherwise the actual value.
    pub value_masked: String,
    pub source: ConfigSource,
}

/// Result of loading config: which keys were effective and from where (secret values masked in summary).
#[derive(Clone, Debug, Default)]
pub struct ConfigLoadReport {
    pub entries: Vec<LoadedEntry>,
    pub dotenv_path: Option<PathBuf>,
    pub xdg_path: Option<PathBuf>,
    /// Name of the active provider resolved from `[[providers]]`, if any.
    pub active_provider: Option<String>,
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
    /// One-line summary for logging: `config: KEY1=*** KEY2=value ...` (secret values masked, others plain).
    fn format_entry(key: &str, value: &str) -> String {
        let v = if value.contains(' ') || value.is_empty() {
            format!("\"{}\"", value.replace('\"', "\\\""))
        } else {
            value.to_string()
        };
        format!("{}={}", key, v)
    }

    pub fn keys_summary(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let line = self
            .entries
            .iter()
            .map(|e| Self::format_entry(&e.key, &e.value_masked))
            .collect::<Vec<_>>()
            .join(" ");
        Some(format!("config: {}", line))
    }

    /// Human-readable summary for logging (keys plain, values masked). Prefer logging each path separately with `dotenv_path`/`xdg_path` and then `keys_summary()`.
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
    #[error("config path: {0}")]
    XdgPath(String),
    #[error("read config: {0}")]
    XdgRead(std::io::Error),
    #[error("parse config toml: {0}")]
    XdgParse(#[from] toml::de::Error),
    #[error("read .env: {0}")]
    DotenvRead(std::io::Error),
}

/// Loads config from `~/.loom/config.toml` and optional project `.env`, then sets environment
/// variables only for keys that are **not** already set (so existing env has highest priority).
///
/// Order of precedence when a key is missing in the process environment:
/// 1. Value from project `.env` (current directory or `override_dir` if given)
/// 2. Value from `~/.loom/config.toml` `[env]` table
///
/// * `app_name`: e.g. `"loom"` — kept for API compatibility (config path is now always `~/.loom/config.toml`).
/// * `override_dir`: if `Some`, look for `.env` in this directory instead of `std::env::current_dir()`.
pub fn load_and_apply(app_name: &str, override_dir: Option<&Path>) -> Result<(), LoadError> {
    let _ = load_and_apply_with_report(app_name, override_dir)?;
    Ok(())
}

/// Like [`load_and_apply`] but returns a report of which keys were applied and from where (keys plain, values masked in report).
///
/// Priority (highest to lowest):
/// 1. Existing process environment  
/// 2. Project `.env`
/// 3. Active `[[providers]]` entry (selected via `[default].provider` or `LOOM_PROVIDER` env var)
/// 4. `[env]` table in `config.toml`
pub fn load_and_apply_with_report(
    app_name: &str,
    override_dir: Option<&Path>,
) -> Result<ConfigLoadReport, LoadError> {
    let full_config = xdg_toml::load_full_config(app_name)?;
    let xdg_map = full_config.env;
    let dotenv_map = dotenv::load_env_map(override_dir).map_err(LoadError::DotenvRead)?;

    // Resolve active provider name (priority: process env > .env > [env] > [default].provider).
    let active_provider_name = std::env::var("LOOM_PROVIDER")
        .ok()
        .or_else(|| dotenv_map.get("LOOM_PROVIDER").cloned())
        .or_else(|| xdg_map.get("LOOM_PROVIDER").cloned())
        .or(full_config.default_provider);
    let provider_map: std::collections::HashMap<String, String> = active_provider_name
        .as_deref()
        .and_then(|name| {
            full_config
                .providers
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
        })
        .map(|p| p.to_env_map())
        .unwrap_or_default();

    let dotenv_path = dotenv::env_file_path(override_dir);
    let xdg_path = xdg_toml::config_path(app_name)?;

    let mut keys: std::collections::HashSet<String> = xdg_map.keys().cloned().collect();
    keys.extend(provider_map.keys().cloned());
    keys.extend(dotenv_map.keys().cloned());

    let mut entries = Vec::with_capacity(keys.len());

    for key in keys {
        let source = if std::env::var(&key).is_ok() {
            ConfigSource::ExistingEnv
        } else if dotenv_map.contains_key(&key) {
            ConfigSource::Dotenv
        } else if provider_map.contains_key(&key) {
            ConfigSource::Provider
        } else {
            ConfigSource::Xdg
        };

        let value = if source == ConfigSource::ExistingEnv {
            std::env::var(&key).ok()
        } else {
            dotenv_map
                .get(&key)
                .or_else(|| provider_map.get(&key))
                .or_else(|| xdg_map.get(&key))
                .cloned()
        };

        if source != ConfigSource::ExistingEnv {
            if let Some(ref v) = value {
                std::env::set_var(&key, v);
            }
        }

        let value_display = value.as_deref().map_or("***".to_string(), |v| {
            if is_secret_key(&key) {
                mask_value(v)
            } else {
                v.to_string()
            }
        });

        entries.push(LoadedEntry {
            key,
            value_masked: value_display,
            source,
        });
    }

    Ok(ConfigLoadReport {
        entries,
        dotenv_path,
        xdg_path,
        active_provider: if provider_map.is_empty() {
            None
        } else {
            active_provider_name
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    static CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn mask_key_keeps_prefix_suffix() {
        assert_eq!(mask_key("OPENAI_API_KEY", 3, 3), "OPE***KEY");
        assert_eq!(mask_key("GITLAB_TOKEN", 3, 3), "GIT***KEN");
        assert_eq!(mask_key("X", 3, 3), "X***");
        assert_eq!(mask_key("", 3, 3), "***");
    }

    #[test]
    fn mask_value_keeps_first_and_last_two_chars() {
        assert_eq!(mask_value("secret"), "se***et");
        assert_eq!(mask_value("abcde"), "ab***de");
        assert_eq!(mask_value("sk-1234567890abcdef"), "sk***ef");
        assert_eq!(mask_value(""), "***");
        assert_eq!(mask_value("a"), "***");
        assert_eq!(mask_value("ab"), "***");
        assert_eq!(mask_value("abcd"), "***");
    }

    #[test]
    fn is_secret_key_masks_credentials_only() {
        assert!(is_secret_key("OPENAI_API_KEY"));
        assert!(is_secret_key("GITLAB_TOKEN"));
        assert!(is_secret_key("MY_SECRET"));
        assert!(is_secret_key("PASSWORD"));
        assert!(is_secret_key("AUTH_TOKEN"));
        assert!(!is_secret_key("RUST_LOG"));
        assert!(!is_secret_key("OPENAI_MODEL"));
        assert!(!is_secret_key("PATH"));
    }

    #[test]
    fn keys_summary_secret_masked_non_secret_plain_one_line() {
        let report = ConfigLoadReport {
            entries: vec![
                LoadedEntry {
                    key: "OPENAI_API_KEY".to_string(),
                    value_masked: "***".to_string(),
                    source: ConfigSource::Dotenv,
                },
                LoadedEntry {
                    key: "RUST_LOG".to_string(),
                    value_masked: "info".to_string(),
                    source: ConfigSource::Xdg,
                },
            ],
            dotenv_path: None,
            xdg_path: None,
            active_provider: None,
        };
        let s = report.keys_summary().unwrap();
        assert_eq!(s, "config: OPENAI_API_KEY=*** RUST_LOG=info");
    }

    fn restore_var(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn existing_env_wins() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        env::set_var("CONFIG_TEST_EXISTING", "from_env");
        let _ = load_and_apply("loom", None);
        assert_eq!(env::var("CONFIG_TEST_EXISTING").as_deref(), Ok("from_env"));
        env::remove_var("CONFIG_TEST_EXISTING");
    }

    #[test]
    fn load_and_apply_no_config_ok() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let empty_home = tempfile::tempdir().unwrap();
        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", empty_home.path());
        let r = load_and_apply("loom", None::<&std::path::Path>);
        restore_var("LOOM_HOME", prev_loom);
        assert!(r.is_ok());
    }

    #[test]
    fn dotenv_overrides_config_toml() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            "[env]\nCONFIG_TEST_PRIORITY = \"from_config\"\n",
        )
        .unwrap();

        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dotenv_dir.path().join(".env"),
            "CONFIG_TEST_PRIORITY=from_dotenv\n",
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("CONFIG_TEST_PRIORITY");

        let _ = load_and_apply("loom", Some(dotenv_dir.path()));
        let val = env::var("CONFIG_TEST_PRIORITY").unwrap();
        env::remove_var("CONFIG_TEST_PRIORITY");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(val, "from_dotenv");
    }

    #[test]
    fn config_toml_applied_when_no_dotenv() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            "[env]\nCONFIG_TEST_LOOM_ONLY = \"from_config\"\n",
        )
        .unwrap();

        let empty_dir = tempfile::tempdir().unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("CONFIG_TEST_LOOM_ONLY");

        let _ = load_and_apply("loom", Some(empty_dir.path()));
        let val = env::var("CONFIG_TEST_LOOM_ONLY").unwrap();
        env::remove_var("CONFIG_TEST_LOOM_ONLY");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(val, "from_config");
    }

    #[test]
    fn dotenv_only_when_no_xdg() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dotenv_dir.path().join(".env"),
            "CONFIG_TEST_DOTENV_ONLY=from_dotenv_only\n",
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("CONFIG_TEST_DOTENV_ONLY");
        let _ = load_and_apply("loom", Some(dotenv_dir.path()));
        let val = env::var("CONFIG_TEST_DOTENV_ONLY").unwrap();
        env::remove_var("CONFIG_TEST_DOTENV_ONLY");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(val, "from_dotenv_only");
    }

    #[test]
    fn invalid_config_toml_fails_with_parse_error() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(loom_home.path().join("config.toml"), "invalid [[[\n").unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());

        let result = load_and_apply("loom", None::<&std::path::Path>);
        restore_var("LOOM_HOME", prev_loom);

        assert!(matches!(result, Err(LoadError::XdgParse(_))));
    }

    #[test]
    fn config_file_paths_returns_both_paths() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(loom_home.path().join("config.toml"), "[env]\n").unwrap();
        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(dotenv_dir.path().join(".env"), "K=V\n").unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        let paths = config_file_paths("loom", Some(dotenv_dir.path()));
        restore_var("LOOM_HOME", prev_loom);

        assert!(paths.xdg.is_some());
        assert!(paths.dotenv.is_some());
    }

    #[test]
    fn config_file_paths_returns_none_when_missing() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let empty = tempfile::tempdir().unwrap();
        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", empty.path());
        let paths = config_file_paths("loom", Some(empty.path()));
        restore_var("LOOM_HOME", prev_loom);

        assert!(paths.xdg.is_none());
        assert!(paths.dotenv.is_none());
    }

    #[test]
    fn load_and_apply_with_report_returns_entries() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            "[env]\nCONFIG_TEST_REPORT_LOG = \"report_val\"\n",
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("CONFIG_TEST_REPORT_LOG");

        let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();
        env::remove_var("CONFIG_TEST_REPORT_LOG");
        restore_var("LOOM_HOME", prev_loom);

        assert!(!report.entries.is_empty());
        let entry = report.entries.iter().find(|e| e.key == "CONFIG_TEST_REPORT_LOG");
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.source, ConfigSource::Xdg);
        assert_eq!(entry.value_masked, "report_val");
    }

    #[test]
    fn load_and_apply_with_report_masks_secret_values() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            "[env]\nCONFIG_TEST_API_KEY = \"super_secret\"\n",
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("CONFIG_TEST_API_KEY");

        let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();
        env::remove_var("CONFIG_TEST_API_KEY");
        restore_var("LOOM_HOME", prev_loom);

        let entry = report.entries.iter().find(|e| e.key == "CONFIG_TEST_API_KEY").unwrap();
        assert_eq!(entry.value_masked, "su***et");
    }

    #[test]
    fn report_summary_no_config_loaded() {
        let report = ConfigLoadReport::default();
        let s = report.summary();
        assert!(s.contains("no .env or config.toml loaded"));
    }

    #[test]
    fn report_summary_with_dotenv_and_xdg_paths() {
        let report = ConfigLoadReport {
            entries: vec![LoadedEntry {
                key: "FOO".to_string(),
                value_masked: "bar".to_string(),
                source: ConfigSource::Dotenv,
            }],
            dotenv_path: Some(PathBuf::from("/tmp/.env")),
            xdg_path: Some(PathBuf::from("/tmp/config.toml")),
            active_provider: None,
        };
        let s = report.summary();
        assert!(s.contains(".env: /tmp/.env"));
        assert!(s.contains("config.toml: /tmp/config.toml"));
        assert!(s.contains("FOO=bar"));
    }

    #[test]
    fn report_summary_env_only_no_file_paths() {
        let report = ConfigLoadReport {
            entries: vec![LoadedEntry {
                key: "EXISTING".to_string(),
                value_masked: "val".to_string(),
                source: ConfigSource::ExistingEnv,
            }],
            dotenv_path: None,
            xdg_path: None,
            active_provider: None,
        };
        let s = report.summary();
        assert!(s.contains("config loaded (env only)"));
        assert!(s.contains("EXISTING=val"));
    }

    #[test]
    fn keys_summary_returns_none_when_empty() {
        let report = ConfigLoadReport::default();
        assert!(report.keys_summary().is_none());
    }

    #[test]
    fn format_entry_quotes_empty_and_spaces() {
        assert_eq!(ConfigLoadReport::format_entry("K", ""), "K=\"\"");
        assert_eq!(ConfigLoadReport::format_entry("K", "has space"), "K=\"has space\"");
        assert_eq!(ConfigLoadReport::format_entry("K", "nospace"), "K=nospace");
    }

    #[test]
    fn is_secret_key_edge_cases() {
        assert!(is_secret_key("KEY"));
        assert!(is_secret_key("MY_CREDENTIAL_ID"));
        assert!(is_secret_key("APP_AUTH_HEADER"));
        assert!(!is_secret_key("KEYBOARD"));
        assert!(!is_secret_key("MODEL_NAME"));
    }

    #[test]
    fn mask_key_with_zero_prefix_suffix() {
        assert_eq!(mask_key("ABCDEF", 0, 0), "***");
    }

    #[test]
    fn load_error_display() {
        let e = LoadError::XdgPath("test".to_string());
        assert!(e.to_string().contains("test"));
        let e = LoadError::DotenvRead(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(e.to_string().contains("gone"));
    }

    #[test]
    fn config_source_derives() {
        assert_eq!(ConfigSource::ExistingEnv, ConfigSource::ExistingEnv);
        assert_ne!(ConfigSource::Dotenv, ConfigSource::Xdg);
        assert_ne!(ConfigSource::Provider, ConfigSource::Xdg);
        let s = format!("{:?}", ConfigSource::Dotenv);
        assert!(s.contains("Dotenv"));
        let s = format!("{:?}", ConfigSource::Provider);
        assert!(s.contains("Provider"));
    }

    #[test]
    fn provider_settings_applied_when_default_provider_set() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[default]
provider = "my-llm"

[[providers]]
name = "my-llm"
api_key = "sk-test-provider"
base_url = "https://example.com/v1"
model = "test-model"
"#,
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("OPENAI_BASE_URL");
        env::remove_var("MODEL");
        env::remove_var("LOOM_PROVIDER");

        let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

        let api_key = env::var("OPENAI_API_KEY").unwrap_or_default();
        let base_url = env::var("OPENAI_BASE_URL").unwrap_or_default();
        let model = env::var("MODEL").unwrap_or_default();

        env::remove_var("OPENAI_API_KEY");
        env::remove_var("OPENAI_BASE_URL");
        env::remove_var("MODEL");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(api_key, "sk-test-provider");
        assert_eq!(base_url, "https://example.com/v1");
        assert_eq!(model, "test-model");

        let api_key_entry = report.entries.iter().find(|e| e.key == "OPENAI_API_KEY");
        assert!(api_key_entry.is_some());
        assert_eq!(api_key_entry.unwrap().source, ConfigSource::Provider);
    }

    #[test]
    fn provider_type_sets_llm_provider_env() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[default]
provider = "bigmodel"

[[providers]]
name = "bigmodel"
api_key = "bm-key"
base_url = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-4-flash"
type = "bigmodel"
"#,
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("LLM_PROVIDER");
        env::remove_var("MODEL");
        env::remove_var("LOOM_PROVIDER");

        let _ = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

        let llm_provider = env::var("LLM_PROVIDER").unwrap_or_default();
        let model = env::var("MODEL").unwrap_or_default();

        env::remove_var("OPENAI_API_KEY");
        env::remove_var("LLM_PROVIDER");
        env::remove_var("MODEL");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(llm_provider, "bigmodel");
        assert_eq!(model, "glm-4-flash");
    }

    #[test]
    fn dotenv_overrides_provider_settings() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[default]
provider = "base"

[[providers]]
name = "base"
model = "model-from-provider"
"#,
        )
        .unwrap();

        let dotenv_dir = tempfile::tempdir().unwrap();
        std::fs::write(dotenv_dir.path().join(".env"), "MODEL=model-from-dotenv\n").unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("MODEL");
        env::remove_var("LOOM_PROVIDER");

        let _ = load_and_apply_with_report("loom", Some(dotenv_dir.path())).unwrap();
        let model = env::var("MODEL").unwrap_or_default();

        env::remove_var("MODEL");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(model, "model-from-dotenv");
    }

    #[test]
    fn process_env_wins_over_provider() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[default]
provider = "base"

[[providers]]
name = "base"
model = "model-from-provider"
"#,
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::set_var("MODEL", "model-from-env");
        env::remove_var("LOOM_PROVIDER");

        let _ = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();
        let model = env::var("MODEL").unwrap_or_default();

        env::remove_var("MODEL");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(model, "model-from-env");
    }

    #[test]
    fn unknown_provider_name_applies_nothing() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[default]
provider = "nonexistent"

[[providers]]
name = "other"
model = "other-model"
"#,
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::remove_var("MODEL");
        env::remove_var("LOOM_PROVIDER");

        let _ = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();
        let model = env::var("MODEL").ok();

        restore_var("LOOM_HOME", prev_loom);

        assert!(model.is_none());
    }

    #[test]
    fn loom_provider_env_selects_provider() {
        let _g = CONFIG_ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        std::fs::write(
            loom_home.path().join("config.toml"),
            r#"
[[providers]]
name = "fast"
model = "fast-model"

[[providers]]
name = "slow"
model = "slow-model"
"#,
        )
        .unwrap();

        let prev_loom = env::var("LOOM_HOME").ok();
        env::set_var("LOOM_HOME", loom_home.path());
        env::set_var("LOOM_PROVIDER", "slow");
        env::remove_var("MODEL");

        let _ = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();
        let model = env::var("MODEL").unwrap_or_default();

        env::remove_var("MODEL");
        env::remove_var("LOOM_PROVIDER");
        restore_var("LOOM_HOME", prev_loom);

        assert_eq!(model, "slow-model");
    }
}
