//! Configuration for building a ReAct run context.

use std::path::PathBuf;

/// ToT-specific runner config (max depth, candidates per step, etc.).
#[derive(Clone, Debug)]
pub struct TotRunnerConfig {
    pub max_depth: u32,
    pub candidates_per_step: u32,
    pub research_quality_addon: bool,
}

impl Default for TotRunnerConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            candidates_per_step: 3,
            research_quality_addon: false,
        }
    }
}

/// GoT-specific runner config (adaptive mode, AGoT LLM complexity).
#[derive(Clone, Debug)]
pub struct GotRunnerConfig {
    pub adaptive: bool,
    pub agot_llm_complexity: bool,
}

impl Default for GotRunnerConfig {
    fn default() -> Self {
        Self {
            adaptive: false,
            agot_llm_complexity: false,
        }
    }
}

/// Configuration for building ReAct run context.
#[derive(Clone, Debug)]
pub struct ReactBuildConfig {
    pub db_path: Option<String>,
    pub thread_id: Option<String>,
    pub user_id: Option<String>,
    pub system_prompt: Option<String>,
    pub exa_api_key: Option<String>,
    pub twitter_api_key: Option<String>,
    pub mcp_exa_url: String,
    pub mcp_remote_cmd: String,
    pub mcp_remote_args: String,
    pub mcp_verbose: bool,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub model: Option<String>,
    pub embedding_api_key: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_model: Option<String>,
    pub working_folder: Option<PathBuf>,
    pub approval_policy: Option<crate::helve::ApprovalPolicy>,
    pub compaction_config: Option<crate::compress::CompactionConfig>,
    pub tot_config: TotRunnerConfig,
    pub got_config: GotRunnerConfig,
}

impl ReactBuildConfig {
    /// Builds config from environment variables.
    pub fn from_env() -> Self {
        let mcp_verbose = std::env::var("MCP_VERBOSE")
            .or_else(|_| std::env::var("VERBOSE"))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);
        Self {
            db_path: std::env::var("DB_PATH").ok(),
            thread_id: std::env::var("THREAD_ID").ok(),
            user_id: std::env::var("USER_ID").ok(),
            system_prompt: std::env::var("REACT_SYSTEM_PROMPT").ok(),
            exa_api_key: std::env::var("EXA_API_KEY").ok(),
            twitter_api_key: std::env::var("TWITTER_API_KEY").ok(),
            mcp_exa_url: std::env::var("MCP_EXA_URL")
                .unwrap_or_else(|_| "https://mcp.exa.ai/mcp".to_string()),
            mcp_remote_cmd: std::env::var("MCP_REMOTE_CMD").unwrap_or_else(|_| "npx".to_string()),
            mcp_remote_args: std::env::var("MCP_REMOTE_ARGS")
                .unwrap_or_else(|_| "-y mcp-remote".to_string()),
            mcp_verbose,
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
            model: std::env::var("MODEL")
                .or_else(|_| std::env::var("OPENAI_MODEL"))
                .ok(),
            embedding_api_key: std::env::var("EMBEDDING_API_KEY")
                .or_else(|_| std::env::var("BIGMODEL_API_KEY"))
                .ok(),
            embedding_base_url: std::env::var("EMBEDDING_API_BASE")
                .or_else(|_| std::env::var("EMBEDDING_BASE_URL"))
                .ok(),
            embedding_model: std::env::var("EMBEDDING_MODEL").ok(),
            working_folder: std::env::var("WORKING_FOLDER").ok().map(PathBuf::from),
            approval_policy: None,
            compaction_config: None,
            tot_config: TotRunnerConfig {
                max_depth: std::env::var("TOT_MAX_DEPTH")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5),
                candidates_per_step: std::env::var("TOT_CANDIDATES_PER_STEP")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3),
                research_quality_addon: std::env::var("TOT_RESEARCH_QUALITY_ADDON")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
            },
            got_config: GotRunnerConfig {
                adaptive: std::env::var("GOT_ADAPTIVE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
                agot_llm_complexity: std::env::var("GOT_AGOT_LLM_COMPLEXITY")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
            },
        }
    }
}
