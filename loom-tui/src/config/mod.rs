//! Configuration management for Loom TUI

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

/// Command line arguments for TUI mode
#[derive(Parser, Debug, Clone)]
pub struct Args {
    #[command(flatten)]
    pub cli_args: cli::Args,  // Reuse existing CLI arguments
    
    #[arg(long, default_value = "true")]
    pub enable_tui: bool,     // Enable TUI interface
    
    #[arg(long, default_value = "3")]
    pub tui_height: u16,      // Height of TUI section in lines
    
    #[arg(long, default_value = "true")]
    pub enable_status_bar: bool, // Enable status bar
    
    #[arg(long, default_value = "1000")]
    pub input_history_size: usize, // Size of input history
    
    #[arg(long, default_value = "false")]
    pub debug_tui: bool,       // Debug TUI rendering
    
    #[arg(long)]
    pub config_path: Option<PathBuf>, // Custom config path
    
    #[arg(long, default_value = "info")]
    pub log_level: String,    // Logging level
}

/// TUI-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiConfig {
    pub ui: UiConfig,
    pub layout: LayoutConfig,
    pub colors: ColorConfig,
    pub input: InputConfig,
    pub status: StatusConfig,
    
    #[serde(flatten)]
    pub agent_config: AgentConfig,  // Agent-specific configuration
}

/// UI-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub enable_tui: bool,
    pub tui_height: u16,
    pub enable_status_bar: bool,
    pub theme: String,
    pub animations: bool,
    pub show_borders: bool,
}

/// Layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub main_output_height_ratio: f32,
    pub tui_section_height: u16,
    pub status_bar_height: u16,
    pub padding: u16,
    pub margin: u16,
}

/// Color configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorConfig {
    pub background: String,
    pub primary: String,
    pub secondary: String,
    pub accent: String,
    pub text: String,
    pub input_text: String,
    pub status_text: String,
    pub border: String,
    pub success: String,
    pub warning: String,
    pub error: String,
}

/// Input configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub history_size: usize,
    pub completion_enabled: bool,
    pub syntax_highlighting: bool,
    pub auto_suggest: bool,
    pub multiline_support: bool,
}

/// Status configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    pub show_agent_status: bool,
    pub show_session_info: bool,
    pub show_system_metrics: bool,
    pub update_interval_ms: u64,
    pub compact_mode: bool,
}

/// Agent configuration (flattened from CLI args)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: String,
    pub provider: String,
    pub working_folder: Option<PathBuf>,
    pub interactive: bool,
    pub verbose: bool,
    pub json: bool,
    pub timestamp: bool,
}

/// Main application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub cli_args: Args,
    pub tui_config: TuiConfig,
    pub agent_config: cli::RunOptions,
}

impl Config {
    /// Load configuration from command line arguments and config file
    pub fn load(args: &Args) -> Result<Self> {
        // Load TUI-specific config
        let tui_config = Self::load_tui_config(args)?;
        
        // Convert CLI args to agent config
        let agent_config = Self::create_agent_config(args);
        
        Ok(Self {
            cli_args: args.clone(),
            tui_config,
            agent_config,
        })
    }
    
    /// Load TUI configuration from file or defaults
    fn load_tui_config(args: &Args) -> Result<TuiConfig> {
        let config_path = args.config_path
            .clone()
            .or_else(|| {
                let config_dir = dirs::config_dir()
                    .map(|dir| dir.join("loom-tui"))
                    .unwrap_or_else(|| PathBuf::from("."));
                Some(config_dir.join("loom-tui.toml"))
            });
        
        if let Some(path) = config_path {
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let config: TuiConfig = toml::from_str(&content)
                    .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;
                return Ok(config);
            }
        }
        
        // Return default configuration
        Ok(Self::default_tui_config(args))
    }
    
    /// Create default TUI configuration
    fn default_tui_config(args: &Args) -> TuiConfig {
        TuiConfig {
            ui: UiConfig {
                enable_tui: args.enable_tui,
                tui_height: args.tui_height,
                enable_status_bar: args.enable_status_bar,
                theme: "dark".to_string(),
                animations: true,
                show_borders: true,
            },
            layout: LayoutConfig {
                main_output_height_ratio: 0.85,
                tui_section_height: args.tui_height,
                status_bar_height: if args.enable_status_bar { 1 } else { 0 },
                padding: 1,
                margin: 0,
            },
            colors: ColorConfig {
                background: "#1a1a1a".to_string(),
                primary: "#3498db".to_string(),
                secondary: "#2ecc71".to_string(),
                accent: "#e74c3c".to_string(),
                text: "#ecf0f1".to_string(),
                input_text: "#ffffff".to_string(),
                status_text: "#bdc3c7".to_string(),
                border: "#34495e".to_string(),
                success: "#2ecc71".to_string(),
                warning: "#f39c12".to_string(),
                error: "#e74c3c".to_string(),
            },
            input: InputConfig {
                history_size: args.input_history_size,
                completion_enabled: true,
                syntax_highlighting: false,
                auto_suggest: true,
                multiline_support: false,
            },
            status: StatusConfig {
                show_agent_status: true,
                show_session_info: true,
                show_system_metrics: true,
                update_interval_ms: 1000,
                compact_mode: false,
            },
            agent_config: Self::create_agent_config(args),
        }
    }
    
    /// Create agent configuration from CLI args
    fn create_agent_config(args: &Args) -> cli::RunOptions {
        cli::RunOptions {
            message: loom::UserContent::Text("".to_string()), // Will be filled by user input
            working_folder: args.cli_args.working_folder.clone(),
            session_id: None,
            cancellation: None,
            thread_id: args.cli_args.session_id.clone(),
            agent: args.cli_args.agent.clone(),
            verbose: args.cli_args.verbose,
            got_adaptive: false, // Will be set based on command type
            display_max_len: 2000, // Default value
            output_json: args.cli_args.json,
            model: args.cli_args.model.clone(),
            mcp_config_path: args.cli_args.mcp_config.clone(),
            output_timestamp: args.cli_args.timestamp,
            dry_run: args.cli_args.dry,
            debug_llm: args.cli_args.debug_llm,
            provider: args.cli_args.provider.clone(),
            base_url: None,
            api_key: None,
            provider_type: None,
            any_stream_event_sender: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
            worktree: args.cli_args.worktree,
            goal_mode: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_config_loading_from_args() {
        let args = Args::parse_from(&[
            "loom-tui",
            "--enable-tui",
            "--tui-height", "5",
            "--enable-status-bar",
        ]);
        
        let config = Config::load(&args).unwrap();
        assert_eq!(config.tui_config.ui.tui_height, 5);
        assert_eq!(config.tui_config.ui.enable_status_bar, true);
    }
    
    #[test]
    fn test_default_config_fallback() {
        let args = Args::parse_from(&["loom-tui"]);
        let config = Config::load(&args).unwrap();
        assert_eq!(config.tui_config.ui.tui_height, 3);
        assert_eq!(config.tui_config.ui.enable_tui, true);
    }
}