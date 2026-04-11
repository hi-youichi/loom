//! MCP server management for CLI operations
//!
//! This module provides a higher-level interface for MCP server management
//! that integrates with the config crate and handles CLI-specific concerns.

use config::{
    create_mcp_config_if_missing, discover_mcp_config_path, load_mcp_config_file,
    McpConfigError, McpServerEntry,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Manager for MCP server operations
pub struct McpManager {
    config_path: std::path::PathBuf,
}

impl McpManager {
    /// Creates a new MCP manager, discovering config path with priority:
    /// project `.loom/mcp.json` > `~/.loom/mcp.json`.
    /// Creates the file if none exists.
    pub fn new() -> Result<Self, McpConfigError> {
        let config_path = discover_mcp_config_path(None, Some(std::env::current_dir()?.as_path()))
            .unwrap_or_else(|| {
                let p = config::home::loom_home().join("mcp.json");
                let _ = create_mcp_config_if_missing(&p);
                p
            });
        Ok(Self { config_path })
    }

    /// Lists all MCP servers
    pub fn list_servers(&self) -> Result<Vec<ServerInfo>, McpConfigError> {
        let config = load_mcp_config_file(&self.config_path)?;
        let servers: Vec<ServerInfo> = config
            .mcp_servers
            .into_iter()
            .map(|(name, entry)| ServerInfo {
                name,
                server_type: if entry.command.is_some() {
                    "stdio".to_string()
                } else {
                    "http".to_string()
                },
                disabled: entry.disabled,
                command: entry.command,
                url: entry.url,
            })
            .collect();
        Ok(servers)
    }

    /// Shows details of a specific MCP server
    pub fn show_server(&self, name: &str) -> Result<Option<ServerDetail>, McpConfigError> {
        let config = load_mcp_config_file(&self.config_path)?;
        if let Some(entry) = config.mcp_servers.get(name) {
            Ok(Some(ServerDetail {
                name: name.to_string(),
                entry: entry.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Adds a new MCP server
    pub fn add_server(&self, args: &AddMcpArgs) -> Result<(), McpConfigError> {
        let entry = self.build_server_entry(args)?;

        // Check if server already exists
        let config = load_mcp_config_file(&self.config_path)?;
        if config.mcp_servers.contains_key(&args.name) {
            return Err(McpConfigError::InvalidEntry {
                name: args.name.clone(),
                message: "Server already exists".to_string(),
            });
        }

        config::upsert_mcp_server(&self.config_path, &args.name, entry)?;
        Ok(())
    }

    /// Edits an existing MCP server
    pub fn edit_server(&self, name: &str, args: &EditMcpArgs) -> Result<(), McpConfigError> {
        let config = load_mcp_config_file(&self.config_path)?;

        let existing_entry = config
            .mcp_servers
            .get(name)
            .ok_or_else(|| McpConfigError::InvalidEntry {
                name: name.to_string(),
                message: "Server not found".to_string(),
            })?;

        let updated_entry = self.merge_server_entry(existing_entry, args)?;
        config::upsert_mcp_server(&self.config_path, name, updated_entry)?;
        Ok(())
    }

    /// Deletes an MCP server
    pub fn delete_server(&self, name: &str) -> Result<bool, McpConfigError> {
        config::remove_mcp_server(&self.config_path, name)
    }

    /// Enables a disabled MCP server
    pub fn enable_server(&self, name: &str) -> Result<(), McpConfigError> {
        let config = load_mcp_config_file(&self.config_path)?;

        let mut entry = config
            .mcp_servers
            .get(name)
            .ok_or_else(|| McpConfigError::InvalidEntry {
                name: name.to_string(),
                message: "Server not found".to_string(),
            })?
            .clone();

        entry.disabled = false;
        config::upsert_mcp_server(&self.config_path, name, entry)?;
        Ok(())
    }

    /// Disables an enabled MCP server
    pub fn disable_server(&self, name: &str) -> Result<(), McpConfigError> {
        let config = load_mcp_config_file(&self.config_path)?;

        let mut entry = config
            .mcp_servers
            .get(name)
            .ok_or_else(|| McpConfigError::InvalidEntry {
                name: name.to_string(),
                message: "Server not found".to_string(),
            })?
            .clone();

        entry.disabled = true;
        config::upsert_mcp_server(&self.config_path, name, entry)?;
        Ok(())
    }

    fn build_server_entry(&self, args: &AddMcpArgs) -> Result<McpServerEntry, McpConfigError> {
        let env = self.parse_env_vars(&args.env);

        if let Some(command) = &args.command {
            Ok(McpServerEntry {
                command: Some(command.clone()),
                args: args.args.clone(),
                url: None,
                env,
                disabled: args.disabled,
                headers: HashMap::new(),
            })
        } else if let Some(url) = &args.url {
            Ok(McpServerEntry {
                command: None,
                args: Vec::new(),
                url: Some(url.clone()),
                env,
                disabled: args.disabled,
                headers: HashMap::new(),
            })
        } else {
            Err(McpConfigError::InvalidEntry {
                name: args.name.clone(),
                message: "Either --command or --url must be specified".to_string(),
            })
        }
    }

    fn merge_server_entry(
        &self,
        existing: &McpServerEntry,
        args: &EditMcpArgs,
    ) -> Result<McpServerEntry, McpConfigError> {
        let env = if args.env.is_empty() {
            existing.env.clone()
        } else {
            self.parse_env_vars(&args.env)
        };

        let command = if args.command.is_some() {
            args.command.clone()
        } else {
            existing.command.clone()
        };

        let url = if args.url.is_some() {
            args.url.clone()
        } else {
            existing.url.clone()
        };
        let server_args = if !args.args.is_empty() {
            args.args.clone()
        } else {
            existing.args.clone()
        };
        let disabled = args.disabled.unwrap_or(existing.disabled);
        Ok(McpServerEntry {
            command,
            args: server_args,
            url,
            env,
            disabled,
            headers: existing.headers.clone(),
        })
    }

    fn parse_env_vars(&self, env_vars: &[String]) -> HashMap<String, String> {
        env_vars
            .iter()
            .filter_map(|s| {
                let mut parts = s.splitn(2, '=');
                let key = parts.next()?;
                let value = parts.next().unwrap_or_default();
                Some((key.to_string(), value.to_string()))
            })
            .collect()
    }
}

/// CLI arguments for adding an MCP server
#[derive(Debug, Clone)]
pub struct AddMcpArgs {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env: Vec<String>,
    pub disabled: bool,
}

/// CLI arguments for editing an MCP server
#[derive(Debug, Clone)]
pub struct EditMcpArgs {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env: Vec<String>,
    pub disabled: Option<bool>,
}

/// Information about an MCP server for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub server_type: String,
    pub disabled: bool,
    pub command: Option<String>,
    pub url: Option<String>,
}

/// Detailed information about an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetail {
    pub name: String,
    #[serde(flatten)]
    pub entry: McpServerEntry,
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new().expect("Failed to create MCP manager")
    }
}