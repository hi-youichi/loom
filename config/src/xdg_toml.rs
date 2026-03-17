//! Load `[env]` table and `[[providers]]` from `~/.loom/config.toml`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::LoadError;

/// Returns path to `config.toml` if it exists. Public for config load report.
pub fn config_path(_app_name: &str) -> Result<Option<PathBuf>, LoadError> {
    let path = crate::home::loom_home().join("config.toml");
    if path.exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// A named LLM provider definition from `[[providers]]` in `config.toml`.
///
/// Example:
/// ```toml
/// [[providers]]
/// name = "openai"
/// api_key = "sk-..."
/// base_url = "https://api.openai.com/v1"
/// model = "gpt-4o-mini"
///
/// [[providers]]
/// name = "local"
/// api_key = "none"
/// base_url = "http://localhost:11434/v1"
/// model = "llama3.2"
///
/// [[providers]]
/// name = "bigmodel"
/// api_key = "xxx.yyy"
/// base_url = "https://open.bigmodel.cn/api/paas/v4"
/// model = "glm-4-flash"
/// type = "bigmodel"
/// ```
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ProviderDef {
    /// Unique name used to reference this provider (e.g. in `[default].provider`).
    pub name: String,
    /// API key (mapped to `OPENAI_API_KEY`).
    pub api_key: Option<String>,
    /// Base URL of the API endpoint (mapped to `OPENAI_BASE_URL`).
    pub base_url: Option<String>,
    /// Default model name (mapped to `MODEL`).
    pub model: Option<String>,
    /// Provider implementation type: `"openai"` (default) or `"bigmodel"` (mapped to `LLM_PROVIDER`).
    #[serde(rename = "type")]
    pub provider_type: Option<String>,
}

impl ProviderDef {
    /// Returns env key→value pairs derived from this provider's settings.
    /// Keys: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL`, `LLM_PROVIDER` (when type is set).
    pub fn to_env_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(ref v) = self.api_key {
            map.insert("OPENAI_API_KEY".to_string(), v.clone());
        }
        if let Some(ref v) = self.base_url {
            map.insert("OPENAI_BASE_URL".to_string(), v.clone());
        }
        if let Some(ref v) = self.model {
            map.insert("MODEL".to_string(), v.clone());
        }
        if let Some(ref v) = self.provider_type {
            map.insert("LLM_PROVIDER".to_string(), v.clone());
        }
        map
    }
}

#[derive(serde::Deserialize, Default)]
struct DefaultSection {
    provider: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    default: DefaultSection,
    #[serde(default)]
    providers: Vec<ProviderDef>,
}

/// Parsed content of `config.toml`: env map, default provider name, and provider definitions.
pub struct FullConfig {
    pub env: HashMap<String, String>,
    pub default_provider: Option<String>,
    pub providers: Vec<ProviderDef>,
}

/// Returns env key-value pairs from `[env]` section. Missing file or empty section returns empty map.
#[cfg_attr(not(test), allow(dead_code))]
pub fn load_env_map(app_name: &str) -> Result<HashMap<String, String>, LoadError> {
    Ok(load_full_config(app_name)?.env)
}

/// Loads the full config: `[env]` table, `[default].provider`, and `[[providers]]` list.
/// Missing file returns empty defaults.
pub fn load_full_config(app_name: &str) -> Result<FullConfig, LoadError> {
    let path = match config_path(app_name)? {
        Some(p) => p,
        None => {
            return Ok(FullConfig {
                env: HashMap::new(),
                default_provider: None,
                providers: vec![],
            })
        }
    };
    let content = std::fs::read_to_string(&path).map_err(LoadError::XdgRead)?;
    let config: ConfigFile = toml::from_str(&content)?;
    Ok(FullConfig {
        env: config.env,
        default_provider: config.default.provider,
        providers: config.providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    struct LoomHomeGuard {
        prev: Option<String>,
    }

    impl LoomHomeGuard {
        fn set(value: &std::path::Path) -> Self {
            let prev = env::var("LOOM_HOME").ok();
            env::set_var("LOOM_HOME", value);
            Self { prev }
        }
    }

    impl Drop for LoomHomeGuard {
        fn drop(&mut self) {
            if let Some(p) = self.prev.as_ref() {
                env::set_var("LOOM_HOME", p);
            } else {
                env::remove_var("LOOM_HOME");
            }
        }
    }

    #[test]
    fn missing_config_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = LoomHomeGuard::set(dir.path());
        let map = load_env_map("loom").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn load_env_map_reads_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[env]
FOO = "from_toml"
BAR = "baz"
"#,
        )
        .unwrap();

        let _guard = LoomHomeGuard::set(dir.path());
        let result = load_env_map("loom");

        let map = result.unwrap();
        assert_eq!(map.get("FOO"), Some(&"from_toml".to_string()));
        assert_eq!(map.get("BAR"), Some(&"baz".to_string()));
    }

    #[test]
    fn empty_env_section_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[env]\n").unwrap();

        let _guard = LoomHomeGuard::set(dir.path());
        let result = load_env_map("loom");

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn invalid_toml_returns_xdg_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "not valid toml [[[\n").unwrap();

        let _guard = LoomHomeGuard::set(dir.path());
        let result = load_env_map("loom");

        assert!(matches!(result, Err(crate::LoadError::XdgParse(_))));
    }

    #[test]
    fn config_without_env_section_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[other]\nkey = \"ignored\"\n").unwrap();

        let _guard = LoomHomeGuard::set(dir.path());
        let result = load_env_map("loom");

        let map = result.unwrap();
        assert!(map.is_empty());
    }
}
