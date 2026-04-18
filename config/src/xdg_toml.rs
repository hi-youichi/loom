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
/// tool_choice = "none"
/// temperature = 0.7
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
    /// Provider implementation type: `"openai"` (default), `"openai_compat"`, or `"bigmodel"` (alias; mapped to `LLM_PROVIDER`).
    #[serde(rename = "type")]
    pub provider_type: Option<String>,
    /// Sampling temperature (mapped to `OPENAI_TEMPERATURE` as a decimal string).
    #[serde(default)]
    pub temperature: Option<f64>,
    /// When `true`, fetch model list from `{base_url}/models` instead of models.dev.
    #[serde(default)]
    pub fetch_models: Option<bool>,
    /// Cache TTL for provider API models (in seconds). Default: 300 seconds (5 minutes).
    #[serde(default)]
    pub cache_ttl: Option<u64>,
    /// When `true`, enable tier resolution for this provider. Default: `true`.
    #[serde(default)]
    pub enable_tier_resolution: Option<bool>,
}

impl ProviderDef {
    /// Returns env key→value pairs derived from this provider's settings.
    /// Keys: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL`, `LLM_PROVIDER` (when type is set),
    /// `OPENAI_TEMPERATURE` (when `temperature` is set and finite).
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
        if let Some(t) = self.temperature {
            if t.is_finite() {
                map.insert("OPENAI_TEMPERATURE".to_string(), format!("{t}"));
            }
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
        std::fs::write(
            dir.path().join("config.toml"),
            "[other]\nkey = \"ignored\"\n",
        )
        .unwrap();

        let _guard = LoomHomeGuard::set(dir.path());
        let result = load_env_map("loom");

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn provider_to_env_map_ignores_whitespace_only_tool_choice() {
        let p = ProviderDef {
            name: "openai".into(),
            api_key: None,
            base_url: None,
            model: None,
            provider_type: None,
            temperature: None,
            fetch_models: None,
            cache_ttl: None,
            enable_tier_resolution: None,
        };
        assert!(!p.to_env_map().contains_key("OPENAI_TOOL_CHOICE"));
    }

    #[test]
    fn provider_to_env_map_includes_temperature_when_set() {
        let p = ProviderDef {
            name: "openai".into(),
            api_key: None,
            base_url: None,
            model: None,
            provider_type: None,
            temperature: Some(0.25),
            fetch_models: None,
            cache_ttl: None,
            enable_tier_resolution: None,
        };
        let m = p.to_env_map();
        assert_eq!(
            m.get("OPENAI_TEMPERATURE").map(String::as_str),
            Some("0.25")
        );
    }

    #[test]
    fn provider_to_env_map_omits_non_finite_temperature() {
        let p = ProviderDef {
            name: "openai".into(),
            api_key: None,
            base_url: None,
            model: None,
            provider_type: None,
            temperature: Some(f64::NAN),
            fetch_models: None,
            cache_ttl: None,
            enable_tier_resolution: None,
        };
        assert!(!p.to_env_map().contains_key("OPENAI_TEMPERATURE"));
    }

    #[test]
    fn load_full_config_parses_provider_temperature() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
[[providers]]
name = "p1"
temperature = 0.5
"#,
        )
        .unwrap();
        let _guard = LoomHomeGuard::set(dir.path());
        let full = load_full_config("loom").unwrap();
        assert_eq!(full.providers[0].temperature, Some(0.5));
    }
}
