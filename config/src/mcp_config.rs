//! MCP server config: parse JSON (Cursor/Claude-compatible) and discover config file path.
//!
//! Used by loom to load `mcp.json` from project `.loom/mcp.json` or
//! `~/.loom/mcp.json`. No dependency on loom.

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
    #[error("mcp server entry {name}: {message}")]
    InvalidEntry { name: String, message: String },
}

/// Root structure of mcp.json; key `mcpServers` for Cursor/Claude compatibility.
#[derive(Debug, Deserialize)]
pub struct McpConfigFile {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

/// One server entry in the JSON. Cursor format: either `command` (stdio) or `url` (remote).
#[derive(Debug, Deserialize)]
pub struct McpServerEntry {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Parsed definition for one MCP server: stdio (spawn process) or HTTP (remote URL).
#[derive(Clone, Debug)]
pub enum McpServerDef {
    Stdio {
        name: String,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Http {
        name: String,
        url: String,
        headers: HashMap<String, String>,
    },
}

fn is_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Parses JSON content into a list of enabled MCP server definitions.
/// Skips entries with `disabled: true`. Order follows the map iteration order.
/// Cursor-compatible: each entry must have either `command` (stdio) or `url` (http(s)); url wins if both present.
pub fn parse_mcp_config(content: &str) -> Result<Vec<McpServerDef>, McpConfigError> {
    let file: McpConfigFile = serde_json::from_str(content)?;
    let mut out = Vec::with_capacity(file.mcp_servers.len());
    for (name, entry) in file.mcp_servers {
        if entry.disabled {
            continue;
        }
        let def = if let Some(ref url) = entry.url {
            if !is_http_url(url) {
                return Err(McpConfigError::InvalidEntry {
                    name: name.clone(),
                    message: "url must start with http:// or https://".to_string(),
                });
            }
            McpServerDef::Http {
                name: name.clone(),
                url: url.clone(),
                headers: entry.headers,
            }
        } else if let Some(ref cmd) = entry.command {
            if cmd.is_empty() {
                return Err(McpConfigError::InvalidEntry {
                    name: name.clone(),
                    message: "command must be non-empty when present".to_string(),
                });
            }
            McpServerDef::Stdio {
                name: name.clone(),
                command: cmd.clone(),
                args: entry.args,
                env: entry.env,
            }
        } else {
            return Err(McpConfigError::InvalidEntry {
                name: name.clone(),
                message: "each server must have either 'command' or 'url'".to_string(),
            });
        };
        out.push(def);
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
/// `working_dir/.loom/mcp.json` if it exists; else `~/.loom/mcp.json` if it exists.
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
    let global = crate::home::loom_home().join("mcp.json");
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
        match &list[0] {
            McpServerDef::Stdio { name, command, args, env } => {
                assert_eq!(name, "fs");
                assert_eq!(command, "npx");
                assert_eq!(args.as_slice(), ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]);
                assert!(env.is_empty());
            }
            McpServerDef::Http { .. } => panic!("expected Stdio"),
        }
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
        match &list[0] {
            McpServerDef::Stdio { name, command, args, env } => {
                assert_eq!(name, "enabled");
                assert_eq!(command, "node");
                assert_eq!(args.as_slice(), ["server.js"]);
                assert_eq!(env.get("API_KEY"), Some(&"secret".to_string()));
            }
            McpServerDef::Http { .. } => panic!("expected Stdio"),
        }
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
    fn parse_missing_command_and_url_returns_invalid_entry() {
        let json = r#"{"mcpServers": {"x": {}}}"#;
        let err = parse_mcp_config(json).unwrap_err();
        assert!(matches!(err, McpConfigError::InvalidEntry { .. }));
    }

    #[test]
    fn parse_url_only_http() {
        let json = r#"{
            "mcpServers": {
                "my-service": {
                    "url": "https://mcp.example.com/sse"
                }
            }
        }"#;
        let list = parse_mcp_config(json).unwrap();
        assert_eq!(list.len(), 1);
        match &list[0] {
            McpServerDef::Http { name, url, headers } => {
                assert_eq!(name, "my-service");
                assert_eq!(url, "https://mcp.example.com/sse");
                assert!(headers.is_empty());
            }
            McpServerDef::Stdio { .. } => panic!("expected Http"),
        }
    }

    #[test]
    fn parse_url_with_headers() {
        let json = r#"{
            "mcpServers": {
                "my-service": {
                    "url": "https://mcp.example.com/sse",
                    "headers": {
                        "Authorization": "Bearer your-token-here"
                    }
                }
            }
        }"#;
        let list = parse_mcp_config(json).unwrap();
        assert_eq!(list.len(), 1);
        match &list[0] {
            McpServerDef::Http { name, url, headers } => {
                assert_eq!(name, "my-service");
                assert_eq!(url, "https://mcp.example.com/sse");
                assert_eq!(headers.get("Authorization").map(|s| s.as_str()), Some("Bearer your-token-here"));
            }
            McpServerDef::Stdio { .. } => panic!("expected Http"),
        }
    }

    #[test]
    fn parse_url_wins_when_both_command_and_url() {
        let json = r#"{
            "mcpServers": {
                "hybrid": {
                    "command": "npx",
                    "args": ["-y", "mcp-server"],
                    "url": "https://remote.example.com/mcp"
                }
            }
        }"#;
        let list = parse_mcp_config(json).unwrap();
        assert_eq!(list.len(), 1);
        match &list[0] {
            McpServerDef::Http { name, url, .. } => {
                assert_eq!(name, "hybrid");
                assert_eq!(url, "https://remote.example.com/mcp");
            }
            McpServerDef::Stdio { .. } => panic!("url should win, expected Http"),
        }
    }

    #[test]
    fn parse_invalid_url_returns_error() {
        let json = r#"{"mcpServers": {"x": {"url": "ftp://invalid.example.com"}}}"#;
        let err = parse_mcp_config(json).unwrap_err();
        assert!(matches!(err, McpConfigError::InvalidEntry { .. }));
    }

    #[test]
    fn parse_empty_command_returns_error() {
        let json = r#"{"mcpServers": {"x": {"command": ""}}}"#;
        let err = parse_mcp_config(json).unwrap_err();
        assert!(matches!(err, McpConfigError::InvalidEntry { .. }));
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

        let loom_home = dir.path().join("loom_home");
        std::fs::create_dir_all(&loom_home).unwrap();
        std::fs::write(loom_home.join("mcp.json"), "{}").unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", &loom_home);

        let got = discover_mcp_config_path(Some(&override_path), Some(working.as_path()));
        if let Some(ref p) = prev {
            std::env::set_var("LOOM_HOME", p);
        } else {
            std::env::remove_var("LOOM_HOME");
        }

        assert_eq!(got.as_deref(), Some(project_mcp.as_path()));
    }

    #[test]
    fn discover_returns_none_when_nothing_exists() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path().join("empty");
        std::fs::create_dir_all(&working).unwrap();

        let loom_home = dir.path().join("loom_home");
        std::fs::create_dir_all(&loom_home).unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", &loom_home);

        let got = discover_mcp_config_path(None, Some(working.as_path()));
        if let Some(ref p) = prev {
            std::env::set_var("LOOM_HOME", p);
        } else {
            std::env::remove_var("LOOM_HOME");
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
        match &list[0] {
            McpServerDef::Stdio { name, command, args, .. } => {
                assert_eq!(name, "a");
                assert_eq!(command, "cmd");
                assert_eq!(args.as_slice(), ["x"]);
            }
            McpServerDef::Http { .. } => panic!("expected Stdio"),
        }
    }

    #[test]
    fn load_mcp_config_from_nonexistent_returns_io_error() {
        let path = Path::new("/nonexistent/loom/mcp.json");
        let err = load_mcp_config_from_path(path).unwrap_err();
        assert!(matches!(err, McpConfigError::Io(_)));
    }
}
