//! Load `[env]` table from `$XDG_CONFIG_HOME/<app>/config.toml`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::LoadError;

fn xdg_config_path(app_name: &str) -> Result<Option<PathBuf>, LoadError> {
    let base = cross_xdg::BaseDirs::new().map_err(|e| LoadError::XdgPath(e.to_string()))?;
    let config_dir = base.config_home();
    let path = config_dir.join(app_name).join("config.toml");
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
    let path = match xdg_config_path(app_name)? {
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

    #[test]
    fn missing_config_returns_empty_map() {
        // Use an app name that almost certainly has no config file
        let map = load_env_map("config-crate-test-nonexistent-12345").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn load_env_map_reads_toml() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("testapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let config_path = app_dir.join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[env]
FOO = "from_toml"
BAR = "baz"
"#,
        )
        .unwrap();

        let prev = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_env_map("testapp");
        if let Some(p) = prev.as_ref() {
            env::set_var("XDG_CONFIG_HOME", p);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }

        let map = result.unwrap();
        assert_eq!(map.get("FOO"), Some(&"from_toml".to_string()));
        assert_eq!(map.get("BAR"), Some(&"baz".to_string()));
    }

    #[test]
    fn empty_env_section_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("emptyenv");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("config.toml"), "[env]\n").unwrap();

        let prev = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_env_map("emptyenv");
        if let Some(p) = prev.as_ref() {
            env::set_var("XDG_CONFIG_HOME", p);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn invalid_toml_returns_xdg_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("badapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("config.toml"), "not valid toml [[[\n").unwrap();

        let prev = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_env_map("badapp");
        if let Some(p) = prev.as_ref() {
            env::set_var("XDG_CONFIG_HOME", p);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(matches!(result, Err(crate::LoadError::XdgParse(_))));
    }

    #[test]
    fn config_without_env_section_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("noenv");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("config.toml"), "[other]\nkey = \"ignored\"\n").unwrap();

        let prev = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_env_map("noenv");
        if let Some(p) = prev.as_ref() {
            env::set_var("XDG_CONFIG_HOME", p);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }

        let map = result.unwrap();
        assert!(map.is_empty());
    }
}
