//! Load `[env]` table and `[[providers]]` from `~/.anureo/config.toml`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::LoadError;

/// Returns path to `config.toml` if it exists. Public for config load report.
pub fn config_path(_app_name: &str) -> Result<Option<PathBuf>, LoadError> {
    let path = crate::home::anureo_home().join("config.toml");
    if path.exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// A model specification declared manually in `config.toml` via `[[providers.models]]`.
///
/// Used to supplement models.dev coverage for models that are missing or outdated.
///
/// ```toml
/// [[providers]]
/// name = "zhipuai-coding-plan"
///
/// [[providers.models]]
/// id = "glm-5.2"
/// context_limit = 1000000
/// output_limit = 131072
/// ```
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ProviderModelDef {
    /// Model ID (e.g. `"glm-5.2"`).
    pub id: String,
    /// Context (input) token limit.
    pub context_limit: u32,
    /// Output token limit.
    pub output_limit: u32,
    /// Model-supported reasoning effort subset.
    /// When declared, ACP effort options show only `auto` + these values;
    /// when absent, all 7 values are shown.
    /// Example: `reasoning_efforts = ["low", "medium", "high"]`
    #[serde(default)]
    pub reasoning_efforts: Option<Vec<String>>,
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
///
/// [[providers]]
/// name = "zhipuai-coding-plan"
/// base_url = "https://open.bigmodel.cn/api/coding/paas/v4"
///
/// [[providers.models]]
/// id = "glm-5.2"
/// context_limit = 1000000
/// output_limit = 131072
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
    /// Provider implementation type: `"openai"` (default), `"openai_compat"`, or `"bigmodel"`.
    /// Informational only — the client type is inferred from `base_url` (864ee2d9);
    /// setting this no longer exports an `LLM_PROVIDER` env var.
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
    /// Manually declared model specs to supplement models.dev.
    #[serde(default)]
    pub models: Vec<ProviderModelDef>,
}

impl ProviderDef {
    /// Returns env key→value pairs derived from this provider's settings.
    /// Keys: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL`,
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
        if let Some(t) = self.temperature {
            if t.is_finite() {
                map.insert("OPENAI_TEMPERATURE".to_string(), format!("{t}"));
            }
        }
        map
    }
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct LlmAuditConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// Log rotation setting for a logging module.
#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct LogsModuleConfig {
    /// Whether to enable logging for this module. Default: `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Rotation strategy: `"none"` (default), `"daily"`, `"hourly"`.
    #[serde(default)]
    pub rotate: Option<String>,
    /// Custom log directory for this module (overrides global `dir`).
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Log level filter (tracing `EnvFilter` syntax, e.g. `"info"`, `"debug"`, `"anureo=debug"`).
    ///
    /// Priority: `--log-level` > `RUST_LOG` (shell) > this > verbosity flags (`-v`…) > `off`.
    #[serde(default)]
    pub level: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(feature = "tracing-init")]
impl LogsModuleConfig {
    /// Returns the resolved rotation strategy.
    pub fn rotate(&self) -> crate::tracing_init::LogRotate {
        self.rotate
            .as_deref()
            .and_then(crate::tracing_init::LogRotate::parse)
            .unwrap_or(crate::tracing_init::LogRotate::None)
    }
}

/// `[logging]` section in config.toml.
///
/// Unified logging configuration shared by CLI and ACP.
/// LLM audit logging has its own sub-section `[logging.llm]`.
///
/// ```toml
/// [logging]
/// level = "info"
/// path = "~/.anureo/anureo.log"
/// rotate = "daily"
/// ```
#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct LoggingSection {
    /// Log level filter (tracing `EnvFilter` syntax, e.g. `"info"`, `"debug"`, `"anureo=debug"`).
    ///
    /// Priority: `--log-level` > `RUST_LOG` (shell) > this > verbosity flags (`-v`…) > `off`.
    #[serde(default)]
    pub level: Option<String>,
    /// Log file path. Default: `~/.anureo/anureo.log`.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Rotation strategy: `"none"` (default), `"daily"`, `"hourly"`.
    #[serde(default)]
    pub rotate: Option<String>,
    /// LLM audit logging settings.
    #[serde(default)]
    pub llm: LogsModuleConfig,
}

#[cfg(feature = "tracing-init")]
impl LoggingSection {
    /// Returns the resolved rotation strategy.
    pub fn rotate(&self) -> crate::tracing_init::LogRotate {
        self.rotate
            .as_deref()
            .and_then(crate::tracing_init::LogRotate::parse)
            .unwrap_or(crate::tracing_init::LogRotate::None)
    }
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct LlmSection {
    #[serde(default)]
    pub audit: LlmAuditConfig,
}

impl LlmSection {
    pub fn audit_path(&self) -> PathBuf {
        self.audit
            .path
            .clone()
            .unwrap_or_else(crate::home::llm_logs_dir)
    }
}

/// Defaults applied to `anureo session list` when the user does not provide the
/// corresponding CLI flag. Loaded from the `[session]` table in `config.toml`.
///
/// Example:
/// ```toml
/// [session]
/// default_limit = 100
/// default_format = "%h  %r  %t  (%c)"
/// ```
#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct SessionSection {
    /// Default value for `--limit` when the user does not specify it.
    /// Falls back to the CLI's hardcoded default (50) when unset.
    #[serde(default)]
    pub default_limit: Option<usize>,

    /// Default value for `--format` (placeholder template) when the user
    /// does not specify it. Empty/missing disables the template default.
    #[serde(default)]
    pub default_format: Option<String>,
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
    #[serde(default)]
    llm: Option<LlmSection>,
    #[serde(default)]
    logging: Option<LoggingSection>,
    #[serde(default)]
    session: Option<SessionSection>,
}

/// Parsed content of `config.toml`: env map, default provider name, and provider definitions.
#[derive(Default)]
pub struct FullConfig {
    pub env: HashMap<String, String>,
    pub default_provider: Option<String>,
    pub providers: Vec<ProviderDef>,
    pub llm: LlmSection,
    pub logging: LoggingSection,
    /// `[session]` section with defaults for `anureo session list`.
    /// `None` fields mean "no override", falling back to CLI hardcoded defaults.
    pub session: SessionSection,
}

impl FullConfig {
    /// Convenience constructor for config-load failure fallback.
    /// Returns an empty config with session defaults unset.
    pub fn default_session() -> Self {
        Self::default()
    }
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
        None => return Ok(FullConfig::default()),
    };
    let content = std::fs::read_to_string(&path).map_err(LoadError::XdgRead)?;
    let config: ConfigFile = toml::from_str(&content)?;
    Ok(FullConfig {
        env: config.env,
        default_provider: config.default.provider,
        providers: config.providers,
        llm: config.llm.unwrap_or_default(),
        logging: config.logging.unwrap_or_default(),
        session: config.session.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AnureoHomeGuard {
        prev: Option<std::path::PathBuf>,
    }

    impl AnureoHomeGuard {
        fn set(value: &std::path::Path) -> Self {
            let prev = crate::home::override_path();
            crate::home::set_override(Some(value.to_path_buf()));
            Self { prev }
        }
    }

    impl Drop for AnureoHomeGuard {
        fn drop(&mut self) {
            if let Some(p) = self.prev.as_ref() {
                crate::home::set_override(Some(p.to_path_buf()));
            } else {
                crate::home::set_override(None);
            }
        }
    }

    #[test]
    fn missing_config_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = AnureoHomeGuard::set(dir.path());
        let map = load_env_map("anureo").unwrap();
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

        let _guard = AnureoHomeGuard::set(dir.path());
        let result = load_env_map("anureo");

        let map = result.unwrap();
        assert_eq!(map.get("FOO"), Some(&"from_toml".to_string()));
        assert_eq!(map.get("BAR"), Some(&"baz".to_string()));
    }

    #[test]
    fn empty_env_section_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[env]\n").unwrap();

        let _guard = AnureoHomeGuard::set(dir.path());
        let result = load_env_map("anureo");

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn invalid_toml_returns_xdg_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "not valid toml [[[\n").unwrap();

        let _guard = AnureoHomeGuard::set(dir.path());
        let result = load_env_map("anureo");

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

        let _guard = AnureoHomeGuard::set(dir.path());
        let result = load_env_map("anureo");

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
            models: vec![],
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
            models: vec![],
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
            models: vec![],
        };
        assert!(!p.to_env_map().contains_key("OPENAI_TEMPERATURE"));
    }

    static XDG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn load_full_config_parses_provider_temperature() {
        let _lock = XDG_TEST_LOCK.lock().unwrap();
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
        let _guard = AnureoHomeGuard::set(dir.path());
        let full = load_full_config("anureo").unwrap();
        assert_eq!(full.providers[0].temperature, Some(0.5));
    }

    #[test]
    fn load_full_config_parses_provider_models() {
        let _lock = XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
[[providers]]
name = "zhipuai-coding-plan"

[[providers.models]]
id = "glm-5.2"
context_limit = 1000000
output_limit = 131072

[[providers.models]]
id = "glm-4.6"
context_limit = 204800
output_limit = 131072
"#,
        )
        .unwrap();
        let _guard = AnureoHomeGuard::set(dir.path());
        let full = load_full_config("anureo").unwrap();
        assert_eq!(full.providers.len(), 1);
        assert_eq!(full.providers[0].models.len(), 2);
        assert_eq!(full.providers[0].models[0].id, "glm-5.2");
        assert_eq!(full.providers[0].models[0].context_limit, 1_000_000);
        assert_eq!(full.providers[0].models[0].output_limit, 131_072);
        assert_eq!(full.providers[0].models[1].id, "glm-4.6");
        assert_eq!(full.providers[0].models[1].context_limit, 204_800);
    }

    #[test]
    fn load_full_config_parses_session_defaults() {
        let _lock = XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
[session]
default_limit = 200
default_format = "%h  %r  %t  (%c)"
"#,
        )
        .unwrap();
        let _guard = AnureoHomeGuard::set(dir.path());
        let full = load_full_config("anureo").unwrap();
        assert_eq!(full.session.default_limit, Some(200));
        assert_eq!(
            full.session.default_format.as_deref(),
            Some("%h  %r  %t  (%c)")
        );
    }

    #[test]
    fn load_full_config_missing_session_returns_defaults() {
        let _lock = XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // No [session] section — should fall back to empty SessionSection.
        std::fs::write(dir.path().join("config.toml"), "[env]\nFOO = \"x\"\n").unwrap();
        let _guard = AnureoHomeGuard::set(dir.path());
        let full = load_full_config("anureo").unwrap();
        assert_eq!(full.session.default_limit, None);
        assert_eq!(full.session.default_format, None);
    }
}
