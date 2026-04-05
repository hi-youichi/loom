//! Configuration for building a ReAct run context.

use std::path::PathBuf;
use std::sync::Arc;

use env_config::McpServerDef;

use crate::skill::SkillRegistry;

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
    /// When `EXA_API_KEY` is set, register the Exa `codesearch` tool only if this is true.
    /// Opt-in via env `LOOM_EXA_CODESEARCH` (`1`, `true`, or `yes`, case-insensitive). Default off.
    pub exa_codesearch_enabled: bool,
    pub twitter_api_key: Option<String>,
    pub mcp_exa_url: String,
    pub mcp_remote_cmd: String,
    pub mcp_remote_args: String,
    /// When set, loom will spawn the GitHub MCP server (mcp_github_cmd + mcp_github_args) and pass
    /// GITHUB_TOKEN so the agent can operate on issues (comment, close, labels, etc.).
    pub github_token: Option<String>,
    /// Command to run the GitHub MCP server (e.g. "npx"). Override with MCP_GITHUB_CMD.
    pub mcp_github_cmd: String,
    /// Args for the GitHub MCP server (e.g. ["-y", "@modelcontextprotocol/server-github"]). Override with MCP_GITHUB_ARGS (space-separated).
    pub mcp_github_args: Vec<String>,
    /// When set and http(s), use HTTP transport for GitHub MCP (e.g. https://api.githubcopilot.com/mcp/). Override with MCP_GITHUB_URL.
    pub mcp_github_url: Option<String>,
    pub mcp_verbose: bool,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub model: Option<String>,
    /// Explicit provider type override. When `Some("openai_compat")` or `Some("bigmodel")`, build layer uses [`crate::llm::ChatOpenAICompat`]; otherwise default is OpenAI.
    /// If unset, build layer may infer provider type from `MODEL` in `provider/model` format.
    pub llm_provider: Option<String>,
    /// Sampling temperature for chat completions. Set via `OPENAI_TEMPERATURE`.
    pub openai_temperature: Option<String>,
    pub embedding_api_key: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_model: Option<String>,
    pub working_folder: Option<PathBuf>,
    pub approval_policy: Option<crate::helve::ApprovalPolicy>,
    pub compaction_config: Option<crate::compress::CompactionConfig>,
    pub tot_config: TotRunnerConfig,
    pub got_config: GotRunnerConfig,
    /// MCP servers from mcp.json (discovered by CLI/ACP) or from ACP request.
    pub mcp_servers: Option<Vec<McpServerDef>>,
    /// Skill registry for the skill tool (built during helve config construction).
    pub skill_registry: Option<Arc<SkillRegistry>>,
    /// Maximum nesting depth for `invoke_agent` tool calls (default 3).
    pub max_sub_agent_depth: Option<u32>,
    /// When true, tools are not executed; call_tool returns a placeholder (CLI --dry).
    pub dry_run: bool,
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
            exa_codesearch_enabled: std::env::var("LOOM_EXA_CODESEARCH")
                .ok()
                .is_some_and(|s| {
                    matches!(s.as_str(), "1")
                        || s.eq_ignore_ascii_case("true")
                        || s.eq_ignore_ascii_case("yes")
                }),
            twitter_api_key: std::env::var("TWITTER_API_KEY").ok(),
            mcp_exa_url: std::env::var("MCP_EXA_URL")
                .unwrap_or_else(|_| "https://mcp.exa.ai/mcp".to_string()),
            mcp_remote_cmd: std::env::var("MCP_REMOTE_CMD").unwrap_or_else(|_| "npx".to_string()),
            mcp_remote_args: std::env::var("MCP_REMOTE_ARGS")
                .unwrap_or_else(|_| "-y mcp-remote".to_string()),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            mcp_github_cmd: std::env::var("MCP_GITHUB_CMD").unwrap_or_else(|_| "npx".to_string()),
            mcp_github_args: std::env::var("MCP_GITHUB_ARGS")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_else(|_| {
                    vec![
                        "-y".to_string(),
                        "@modelcontextprotocol/server-github".to_string(),
                    ]
                }),
            mcp_github_url: std::env::var("MCP_GITHUB_URL").ok(),
            mcp_verbose,
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
            model: std::env::var("MODEL")
                .or_else(|_| std::env::var("OPENAI_MODEL"))
                .ok(),
            llm_provider: std::env::var("LLM_PROVIDER").ok(),
            openai_temperature: std::env::var("OPENAI_TEMPERATURE").ok(),
            embedding_api_key: std::env::var("EMBEDDING_API_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
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
            mcp_servers: None,
            skill_registry: None,
            max_sub_agent_depth: std::env::var("MAX_SUB_AGENT_DEPTH")
                .ok()
                .and_then(|s| s.parse().ok()),
            dry_run: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReactBuildConfig;

    fn with_env(key: &str, value: Option<&str>, f: impl FnOnce()) {
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        if let Some(p) = prev {
            std::env::set_var(key, p);
        } else {
            std::env::remove_var(key);
        }
    }

    /// Run all from_env GitHub-related tests in one test to avoid env races when tests run in parallel.
    #[test]
    fn from_env_github_token_and_mcp_override() {
        // 1) With GITHUB_TOKEN set: github_token is_some and default cmd/args
        with_env("GITHUB_TOKEN", Some("test-token"), || {
            with_env("MCP_GITHUB_CMD", None, || {
                with_env("MCP_GITHUB_ARGS", None, || {
                    let config = ReactBuildConfig::from_env();
                    assert!(config.github_token.is_some());
                    assert_eq!(config.github_token.as_deref(), Some("test-token"));
                    assert_eq!(config.mcp_github_cmd, "npx");
                    assert!(config.mcp_github_args.contains(&"-y".to_string()));
                    assert!(config
                        .mcp_github_args
                        .iter()
                        .any(|a| a.contains("server-github")));
                });
            });
        });

        // 2) Without GITHUB_TOKEN: github_token is_none
        with_env("GITHUB_TOKEN", None, || {
            let config = ReactBuildConfig::from_env();
            assert!(config.github_token.is_none());
        });

        // 3) With overrides: MCP_GITHUB_CMD and MCP_GITHUB_ARGS
        with_env("GITHUB_TOKEN", Some("x"), || {
            with_env("MCP_GITHUB_CMD", Some("custom-cmd"), || {
                with_env("MCP_GITHUB_ARGS", Some("arg1 arg2"), || {
                    let config = ReactBuildConfig::from_env();
                    assert_eq!(config.mcp_github_cmd, "custom-cmd");
                    assert_eq!(config.mcp_github_args, &["arg1", "arg2"]);
                });
            });
        });

        // 4) MCP_GITHUB_URL: when set, mcp_github_url is Some
        with_env(
            "MCP_GITHUB_URL",
            Some("https://api.githubcopilot.com/mcp/"),
            || {
                let config = ReactBuildConfig::from_env();
                assert_eq!(
                    config.mcp_github_url.as_deref(),
                    Some("https://api.githubcopilot.com/mcp/")
                );
            },
        );
        with_env("MCP_GITHUB_URL", None, || {
            let config = ReactBuildConfig::from_env();
            assert!(config.mcp_github_url.is_none());
        });
    }
}
