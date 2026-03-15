//! Load `[env]` table from `~/.loom/config.toml`.

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

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    env: HashMap<String, String>,
}

/// Returns env key-value pairs from `[env]` section. Missing file or empty section returns empty map.
pub fn load_env_map(app_name: &str) -> Result<HashMap<String, String>, LoadError> {
    let path = match config_path(app_name)? {
        Some(p) => p,
        None => return Ok(HashMap::new()),
    };
    let content = std::fs::read_to_string(&path).map_err(LoadError::XdgRead)?;
    let config: ConfigFile = toml::from_str(&content)?;
    Ok(config.env)
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
