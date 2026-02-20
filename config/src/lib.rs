//! Load configuration from XDG `config.toml` and project `.env`, then apply to the process
//! environment with priority: **existing env > .env > XDG**.
//!
//! See workspace `docs/xdg_toml_config.md` for the design.

mod dotenv;
mod xdg_toml;

use std::path::Path;
use thiserror::Error;

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
    let xdg_map = xdg_toml::load_env_map(app_name)?;
    let dotenv_map = dotenv::load_env_map(override_dir).map_err(LoadError::DotenvRead)?;

    // Collect all keys from both sources; for each, choose value: env > .env > XDG.
    let mut keys: std::collections::HashSet<String> = xdg_map.keys().cloned().collect();
    keys.extend(dotenv_map.keys().cloned());

    for key in keys {
        if std::env::var(&key).is_ok() {
            continue; // existing env wins
        }
        let value = dotenv_map
            .get(&key)
            .or_else(|| xdg_map.get(&key))
            .cloned();
        if let Some(v) = value {
            std::env::set_var(&key, v);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

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
