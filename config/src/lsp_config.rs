//! LSP configuration: language server definitions and settings.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// LSP configuration error types.
#[derive(Debug, Error)]
pub enum LspConfigError {
    #[error("Failed to parse LSP config: {0}")]
    ParseError(#[from] toml::de::Error),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Language server not found: {0}")]
    ServerNotFound(String),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Language server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Language identifier (e.g., "rust", "typescript")
    pub language: String,
    
    /// Command to start the language server
    pub command: String,
    
    /// Command-line arguments
    #[serde(default)]
    pub args: Vec<String>,
    
    /// File patterns this server handles (e.g., ["*.rs"])
    pub file_patterns: Vec<String>,
    
    /// Initialization options sent to the server
    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,
    
    /// Root URI override (if None, uses workspace root)
    #[serde(default)]
    pub root_uri: Option<String>,
    
    /// Environment variables to set when starting the server
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    
    /// Timeout for server startup (in milliseconds)
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_ms: u64,
    
    /// Auto-install configuration
    #[serde(default)]
    pub auto_install: Option<AutoInstallConfig>,
}

fn default_startup_timeout() -> u64 {
    10000 // 10 seconds
}

/// Auto-install configuration for language servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoInstallConfig {
    /// Whether auto-install is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Install command (e.g., "rustup component add rust-analyzer")
    pub command: String,
    
    /// Verification command to check if already installed
    #[serde(default)]
    pub verify_command: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Full LSP configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    /// List of language servers
    pub servers: Vec<LspServerConfig>,
    
    /// Global settings
    #[serde(default)]
    pub settings: LspGlobalSettings,
}

/// Global LSP settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspGlobalSettings {
    /// Maximum number of concurrent language servers
    #[serde(default = "default_max_servers")]
    pub max_concurrent_servers: usize,
    
    /// Enable logging of LSP communication
    #[serde(default)]
    pub log_communication: bool,
    
    /// Auto-shutdown idle servers after this many seconds (0 = never)
    #[serde(default)]
    pub idle_timeout_seconds: u64,
}

fn default_max_servers() -> usize {
    10
}

impl Default for LspConfig {
    fn default() -> Self {
        LspConfig {
            servers: get_default_servers(),
            settings: LspGlobalSettings::default(),
        }
    }
}

/// Get default language server configurations.
pub fn get_default_servers() -> Vec<LspServerConfig> {
    vec![
        // Rust
        LspServerConfig {
            language: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
            file_patterns: vec!["*.rs".to_string()],
            initialization_options: Some(serde_json::json!({
                "checkOnSave": { "enable": true },
                "cargo": { "loadOutDirsFromCheck": true }
            })),
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 15000,
            auto_install: Some(AutoInstallConfig {
                enabled: true,
                command: "rustup component add rust-analyzer".to_string(),
                verify_command: Some("rust-analyzer --version".to_string()),
            }),
        },
        
        // TypeScript
        LspServerConfig {
            language: "typescript".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            file_patterns: vec!["*.ts".to_string(), "*.tsx".to_string()],
            initialization_options: None,
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 10000,
            auto_install: Some(AutoInstallConfig {
                enabled: true,
                command: "npm install -g typescript-language-server typescript".to_string(),
                verify_command: Some("typescript-language-server --version".to_string()),
            }),
        },
        
        // JavaScript
        LspServerConfig {
            language: "javascript".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            file_patterns: vec!["*.js".to_string(), "*.jsx".to_string()],
            initialization_options: None,
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 10000,
            auto_install: Some(AutoInstallConfig {
                enabled: true,
                command: "npm install -g typescript-language-server typescript".to_string(),
                verify_command: Some("typescript-language-server --version".to_string()),
            }),
        },
        
        // Python
        LspServerConfig {
            language: "python".to_string(),
            command: "pylsp".to_string(),
            args: vec![],
            file_patterns: vec!["*.py".to_string()],
            initialization_options: None,
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 10000,
            auto_install: Some(AutoInstallConfig {
                enabled: true,
                command: "pip install python-lsp-server".to_string(),
                verify_command: Some("pylsp --version".to_string()),
            }),
        },
        
        // Go
        LspServerConfig {
            language: "go".to_string(),
            command: "gopls".to_string(),
            args: vec!["serve".to_string()],
            file_patterns: vec!["*.go".to_string()],
            initialization_options: None,
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 10000,
            auto_install: Some(AutoInstallConfig {
                enabled: true,
                command: "go install golang.org/x/tools/gopls@latest".to_string(),
                verify_command: Some("gopls version".to_string()),
            }),
        },
    ]
}

/// Load LSP configuration from a file.
pub fn load_lsp_config(path: &PathBuf) -> Result<LspConfig, LspConfigError> {
    if !path.exists() {
        return Ok(LspConfig::default());
    }
    
    let content = std::fs::read_to_string(path)?;
    let config: LspConfig = toml::from_str(&content)?;
    
    Ok(config)
}

/// Discover LSP config file location.
pub fn discover_lsp_config_path() -> Option<PathBuf> {
    // Check XDG config home first
    if let Some(xdg_config) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg_config).join("loom/lsp.toml");
        if path.exists() {
            return Some(path);
        }
    }
    
    // Check home directory
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".config/loom/lsp.toml");
        if path.exists() {
            return Some(path);
        }
    }
    
    None
}

/// Load LSP config from default locations.
pub fn load_default_lsp_config() -> Result<LspConfig, LspConfigError> {
    if let Some(path) = discover_lsp_config_path() {
        load_lsp_config(&path)
    } else {
        Ok(LspConfig::default())
    }
}

/// Get default LSP server configurations.
pub fn get_default_lsp_servers() -> Vec<LspServerConfig> {
    get_default_servers()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = LspConfig::default();
        assert!(!config.servers.is_empty());
        assert!(config.servers.iter().any(|s| s.language == "rust"));
        assert!(config.servers.iter().any(|s| s.language == "typescript"));
    }
    
    #[test]
    fn test_serialize_config() {
        let config = LspConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("[[servers]]"));
        assert!(toml_str.contains("rust"));
    }
}
