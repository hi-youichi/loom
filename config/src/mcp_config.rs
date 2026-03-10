//! MCP server config: parse JSON (Cursor/Claude-compatible) and discover config file path.
//!
//! Used by loom to load `mcp.json` from project `.loom/mcp.json` or
//! `$XDG_CONFIG_HOME/loom/mcp.json`. No dependency on loom.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpConfigError {
    #[error("read mcp config: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse mcp config: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Root structure of mcp.json; key `mcpServers` for Cursor/Claude compatibility.
#[derive(Debug, Deserialize)]
pub struct McpConfigFile {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

/// One server entry in the JSON (command required; args/env/disabled optional).
#[derive(Debug, Deserialize)]
pub struct McpServerEntry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

/// Parsed definition for one MCP server, used by loom to spawn and register.
#[derive(Clone, Debug)]
pub struct McpServerDef {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Parses JSON content into a list of enabled MCP server definitions.
/// Skips entries with `disabled: true`. Order follows the map iteration order.
pub fn parse_mcp_config(content: &str) -> Result<Vec<McpServerDef>, McpConfigError> {
    let file: McpConfigFile = serde_json::from_str(content)?;
    let mut out = Vec::with_capacity(file.mcp_servers.len());
    for (name, entry) in file.mcp_servers {
        if entry.disabled {
            continue;
        }
        out.push(McpServerDef {
            name,
            command: entry.command,
            args: entry.args,
            env: entry.env,
        });
    }
    Ok(out)
}

/// Reads the file at `path` and parses it as MCP config.
pub fn load_mcp_config_from_path(path: &Path) -> Result<Vec<McpServerDef>, McpConfigError> {
    let content = std::fs::read_to_string(path)?;
    parse_mcp_config(&content)
}

/// Returns the path to the MCP config file to use, or `None` if none exists.
///
/// Order: if `override_path` is `Some` and that file exists, use it; else
/// `working_dir/.loom/mcp.json` if it exists; else `$XDG_CONFIG_HOME/loom/mcp.json` if it exists.
pub fn discover_mcp_config_path(
    override_path: Option<&Path>,
    working_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    if let Some(wd) = working_dir {
        let project = wd.join(".loom").join("mcp.json");
        if project.exists() {
            return Some(project);
        }
    }
    let base = cross_xdg::BaseDirs::new().ok()?;
    let global = base.config_home().join("loom").join("mcp.json");
    if global.exists() {
        return Some(global);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_servers() {
        let json = r#"{"mcpServers":{}}"#;
        let list = parse_mcp_config(json).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn parse_one_server() {
        let json = r#"{
            "mcpServers": {
                "fs": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
                }
            }
        }"#;
        let list = parse_mcp_config(json).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "fs");
        assert_eq!(list[0].command, "npx");
        assert_eq!(list[0].args, ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]);
        assert!(list[0].env.is_empty());
    }

    #[test]
    fn parse_server_with_env_and_disabled_filter() {
        let json = r#"{
            "mcpServers": {
                "enabled": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {"API_KEY": "secret"}
                },
                "off": {
                    "command": "echo",
                    "args": [],
                    "disabled": true
                }
            }
        }"#;
        let list = parse_mcp_config(json).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "enabled");
        assert_eq!(list[0].command, "node");
        assert_eq!(list[0].args, ["server.js"]);
        assert_eq!(list[0].env.get("API_KEY"), Some(&"secret".to_string()));
    }

    #[test]
    fn parse_missing_mcp_servers_defaults_empty() {
        let json = r#"{}"#;
        let list = parse_mcp_config(json).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let json = r#"{"mcpServers": {"x": "not an object"}}"#;
        let err = parse_mcp_config(json).unwrap_err();
        assert!(matches!(err, McpConfigError::Parse(_)));
    }

    #[test]
    fn parse_missing_command_returns_error() {
        let json = r#"{"mcpServers": {"x": {}}}"#;
        let err = parse_mcp_config(json).unwrap_err();
        assert!(matches!(err, McpConfigError::Parse(_)));
    }

    #[test]
    fn discover_uses_override_when_exists() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("custom.json");
        std::fs::write(&override_path, r#"{"mcpServers":{}}"#).unwrap();
        let working = dir.path().join("proj");
        std::fs::create_dir_all(working.join(".loom")).unwrap();
        std::fs::write(working.join(".loom").join("mcp.json"), "{}").unwrap();

        let got = discover_mcp_config_path(Some(&override_path), Some(working.as_path()));
        assert_eq!(got.as_deref(), Some(override_path.as_path()));
    }

    #[test]
    fn discover_skips_override_when_not_exists_then_project() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("nonexistent.json");
        let working = dir.path().join("proj");
        std::fs::create_dir_all(working.join(".loom")).unwrap();
        let project_mcp = working.join(".loom").join("mcp.json");
        std::fs::write(&project_mcp, "{}").unwrap();

        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", dir.path().join("xdg"));
        std::fs::create_dir_all(dir.path().join("xdg").join("loom")).unwrap();
        std::fs::write(dir.path().join("xdg").join("loom").join("mcp.json"), "{}").unwrap();

        let got = discover_mcp_config_path(Some(&override_path), Some(working.as_path()));
        if let Some(ref p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(got.as_deref(), Some(project_mcp.as_path()));
    }

    #[test]
    fn discover_returns_none_when_nothing_exists() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path().join("empty");
        std::fs::create_dir_all(&working).unwrap();

        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        // no loom/mcp.json under XDG_CONFIG_HOME

        let got = discover_mcp_config_path(None, Some(working.as_path()));
        if let Some(ref p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(got.is_none());
    }

    #[test]
    fn load_mcp_config_from_path_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        std::fs::write(
            &path,
            r#"{"mcpServers":{"a":{"command":"cmd","args":["x"]}}}"#,
        )
        .unwrap();

        let list = load_mcp_config_from_path(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "a");
        assert_eq!(list[0].command, "cmd");
        assert_eq!(list[0].args, ["x"]);
    }

    #[test]
    fn load_mcp_config_from_nonexistent_returns_io_error() {
        let path = Path::new("/nonexistent/loom/mcp.json");
        let err = load_mcp_config_from_path(path).unwrap_err();
        assert!(matches!(err, McpConfigError::Io(_)));
    }
}
