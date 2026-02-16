//! Configuration for building a ReAct run context (checkpointer, store, runnable_config, tool_source).
//!
//! Used by [`build_react_run_context`](super::build::build_react_run_context). CLI or other
//! callers build this from their own config (e.g. env, CLI args) and pass it to the builder.

use std::path::PathBuf;

/// Configuration for building ReAct run context. Holds persistence, tool-source, optional
/// system prompt and optional LLM (OpenAI) fields for default LLM construction.
///
/// Callers build this from run config or env; graphweave uses it to build
/// checkpointer, store, runnable_config and tool_source. When build_react_runner is used
/// with `llm: None`, config's LLM fields (if set) are used to construct the default LLM.
#[derive(Clone, Debug)]
pub struct ReactBuildConfig {
    /// SQLite database path. Defaults to "memory.db" when None at build time.
    pub db_path: Option<String>,
    /// Thread ID for short-term memory (checkpointer). When set, checkpointer is created.
    pub thread_id: Option<String>,
    /// User ID for long-term memory (store). When set, store is created.
    pub user_id: Option<String>,
    /// Optional system prompt. When None, [`REACT_SYSTEM_PROMPT`](crate::REACT_SYSTEM_PROMPT) is used in initial state.
    pub system_prompt: Option<String>,
    /// Exa API key. When set, Exa MCP is enabled; when None, Exa is off.
    pub exa_api_key: Option<String>,
    /// Twitter API key (twitterapi.io). When set, twitter_search tool is enabled.
    pub twitter_api_key: Option<String>,
    /// Exa MCP server URL.
    pub mcp_exa_url: String,
    /// Command for mcp-remote (stdio→HTTP bridge).
    pub mcp_remote_cmd: String,
    /// Args for mcp-remote, e.g. "-y mcp-remote".
    pub mcp_remote_args: String,
    /// When true, MCP subprocess (e.g. mcp-remote) stderr is inherited so debug logs are visible.
    /// When false, stderr is discarded for a quiet default UX.
    pub mcp_verbose: bool,
    /// OpenAI API key. Used when building default LLM (e.g. build_react_runner with llm: None).
    pub openai_api_key: Option<String>,
    /// OpenAI API base URL. When None, default API base is used.
    pub openai_base_url: Option<String>,
    /// Model name (e.g. gpt-4o-mini). Used when building default LLM with `llm: None`.
    pub model: Option<String>,
    /// Embedding API key for long-term memory vector search. When set with `user_id`, enables
    /// semantic memory (e.g. InMemoryVectorStore). When unset and no fallback, long-term memory is disabled.
    pub embedding_api_key: Option<String>,
    /// Embedding API base URL. When None, OpenAI default or `openai_base_url` may be used.
    pub embedding_base_url: Option<String>,
    /// Embedding model (e.g. text-embedding-3-small). When None, a default may be used.
    pub embedding_model: Option<String>,
    /// Working folder for file tools (list_dir, read_file, write_file, etc.). When set, file
    /// tools are registered and paths are restricted to this directory. Typically set per-request
    /// by the server from the request body.
    pub working_folder: Option<PathBuf>,
    /// When set, tools that require approval (e.g. delete_file for DestructiveOnly) will trigger
    /// an Interrupt before execution. On resume, set state.approval_result and config.resume_from_node_id.
    pub approval_policy: Option<crate::helve::ApprovalPolicy>,
    /// When true, GoT uses AGoT mode: complex nodes may be expanded into subgraphs at test time.
    /// Set via `GOT_ADAPTIVE` env or `helve.got_adaptive`. Default: false (plain GoT).
    pub got_adaptive: bool,
    /// When true (and AGoT is enabled), use LLM to classify node simple vs complex instead of heuristic.
    /// Set via `GOT_AGOT_LLM_COMPLEXITY` env. Default: false (use heuristic).
    pub got_agot_llm_complexity: bool,
}

impl ReactBuildConfig {
    /// Builds config from environment variables. No variable is required; unset vars yield `None`
    /// or documented defaults. Use after loading `.env` (e.g. `dotenv::dotenv().ok()`) if desired.
    ///
    /// Reads: `DB_PATH`, `THREAD_ID`, `USER_ID`, `REACT_SYSTEM_PROMPT`, `EXA_API_KEY`,
    /// `MCP_EXA_URL`, `MCP_REMOTE_CMD`, `MCP_REMOTE_ARGS`, `MCP_VERBOSE`/`VERBOSE`,
    /// `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`, `EMBEDDING_API_KEY` (or `BIGMODEL_API_KEY`),
    /// `EMBEDDING_API_BASE` (or `EMBEDDING_BASE_URL`), `EMBEDDING_MODEL`. Defaults: `mcp_exa_url` =
    /// `"https://mcp.exa.ai/mcp"`, `mcp_remote_cmd` = `"npx"`, `mcp_remote_args` = `"-y mcp-remote"`,
    /// `mcp_verbose` = `false`.
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
            model: std::env::var("OPENAI_MODEL").ok(),
            embedding_api_key: std::env::var("EMBEDDING_API_KEY")
                .or_else(|_| std::env::var("BIGMODEL_API_KEY"))
                .ok(),
            embedding_base_url: std::env::var("EMBEDDING_API_BASE")
                .or_else(|_| std::env::var("EMBEDDING_BASE_URL"))
                .ok(),
            embedding_model: std::env::var("EMBEDDING_MODEL").ok(),
            working_folder: std::env::var("WORKING_FOLDER").ok().map(PathBuf::from),
            approval_policy: None,
            got_adaptive: std::env::var("GOT_ADAPTIVE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            got_agot_llm_complexity: std::env::var("GOT_AGOT_LLM_COMPLEXITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
        }
    }
}
