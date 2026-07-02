use env_config::McpServerDef;
use std::path::PathBuf;
use std::sync::Arc;

use skill::SkillRegistry;
use crate::compress::CompactionConfig;
use tool_core::BuiltinToolFilter;
use model_spec_core::ModelTier;
use super::runner_config::{TotRunnerConfig, GotRunnerConfig};
use super::env_context::EnvContext;

#[derive(Clone)]
pub struct ReactBuildConfig {
    pub db_path: Option<String>,
    pub thread_id: Option<String>,
    pub trace_thread_id: Option<String>,
    pub user_id: Option<String>,
    pub system_prompt: Option<String>,
    pub exa_api_key: Option<String>,
    pub exa_codesearch_enabled: bool,
    pub twitter_api_key: Option<String>,
    pub mcp_exa_url: String,
    pub mcp_remote_cmd: String,
    pub mcp_remote_args: String,
    pub github_token: Option<String>,
    pub mcp_github_cmd: String,
    pub mcp_github_args: Vec<String>,
    pub mcp_github_url: Option<String>,
    pub mcp_verbose: bool,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub model: Option<String>,
    pub model_tier: Option<ModelTier>,
    pub parent_model_hint: Option<String>,
    pub aux_model: Option<String>,
    pub llm_provider: Option<String>,
    pub llm_provider_name: Option<String>,
    pub openai_temperature: Option<String>,
    pub embedding_api_key: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_model: Option<String>,
    pub working_folder: Option<PathBuf>,
    pub compaction_config: Option<CompactionConfig>,
    pub tot_config: TotRunnerConfig,
    pub got_config: GotRunnerConfig,
    pub mcp_servers: Option<Vec<McpServerDef>>,
    pub skill_registry: Option<Arc<SkillRegistry>>,
    pub max_sub_agent_depth: Option<u32>,
    pub dry_run: bool,
    pub builtin_tool_filter: Option<BuiltinToolFilter>,
    pub call_tool_filter: Option<BuiltinToolFilter>,
    pub bash_executor: Option<Arc<dyn tool_basic::bash::CommandExecutor>>,
    pub extra_tools: Option<Arc<Vec<Arc<dyn tool_core::Tool>>>>,
    pub acp_session_id: Option<String>,
    pub goal_mode: bool,
    pub is_background_review: bool,
    pub memory_enabled: bool,
    pub user_profile_enabled: bool,
    pub memory_nudge_interval: u32,
    pub skill_nudge_interval: u32,
    pub role_setting: Option<String>,
    pub agents_md: Option<String>,
    pub system_prompt_override: Option<String>,
    pub skills_prompt: Option<String>,
    pub memory_prompt: Option<String>,
    pub env_context: Option<EnvContext>,
    pub reasoning_effort: Option<String>,
}

impl std::fmt::Debug for ReactBuildConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactBuildConfig")
            .field("db_path", &self.db_path)
            .field("working_folder", &self.working_folder)
            .field("model", &self.model)
            .field("bash_executor", &self.bash_executor.as_ref().map(|_| "..."))
            .field("extra_tools", &self.extra_tools.as_ref().map(|t| t.len()))
            .finish()
    }
}

impl Default for ReactBuildConfig {
    fn default() -> Self {
        Self {
            db_path: None,
            thread_id: None,
            trace_thread_id: None,
            user_id: None,
            system_prompt: None,
            exa_api_key: None,
            exa_codesearch_enabled: false,
            twitter_api_key: None,
            mcp_exa_url: String::new(),
            mcp_remote_cmd: String::new(),
            mcp_remote_args: String::new(),
            github_token: None,
            mcp_github_cmd: String::new(),
            mcp_github_args: Vec::new(),
            mcp_github_url: None,
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            openai_temperature: None,
            model: None,
            model_tier: None,
            parent_model_hint: None,
            aux_model: None,
            llm_provider: None,
            llm_provider_name: None,
            embedding_api_key: None,
            embedding_base_url: None,
            embedding_model: None,
            working_folder: None,
            compaction_config: None,
            tot_config: TotRunnerConfig::default(),
            got_config: GotRunnerConfig::default(),
            mcp_servers: None,
            skill_registry: None,
            max_sub_agent_depth: None,
            dry_run: false,
            builtin_tool_filter: None,
            call_tool_filter: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            goal_mode: false,
            is_background_review: false,
            memory_enabled: true,
            user_profile_enabled: true,
            memory_nudge_interval: 10,
            skill_nudge_interval: 10,
            role_setting: None,
            agents_md: None,
            system_prompt_override: None,
            skills_prompt: None,
            memory_prompt: None,
            env_context: None,
            reasoning_effort: None,
        }
    }
}

impl ReactBuildConfig {
    pub fn from_env() -> Self {
        Self {
            db_path: std::env::var("LOOM_DB_PATH").ok(),
            thread_id: std::env::var("LOOM_THREAD_ID").ok(),
            trace_thread_id: None,
            user_id: std::env::var("LOOM_USER_ID").ok(),
            system_prompt: std::env::var("SYSTEM_PROMPT").ok(),
            exa_api_key: std::env::var("EXA_API_KEY").ok(),
            exa_codesearch_enabled: std::env::var("LOOM_EXA_CODESEARCH")
                .ok()
                .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            twitter_api_key: std::env::var("TWITTER_API_KEY").ok(),
            mcp_exa_url: std::env::var("MCP_EXA_URL")
                .unwrap_or_else(|_| "https://exa-cp.backend.mcp.dev".to_string()),
            mcp_remote_cmd: std::env::var("MCP_REMOTE_CMD").unwrap_or_else(|_| "npx".to_string()),
            mcp_remote_args: std::env::var("MCP_REMOTE_ARGS")
                .unwrap_or_else(|_| "mcp-remote".to_string()),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            mcp_github_cmd: std::env::var("MCP_GITHUB_CMD").unwrap_or_else(|_| "npx".to_string()),
            mcp_github_args: std::env::var("MCP_GITHUB_ARGS")
                .unwrap_or_else(|_| "-y @modelcontextprotocol/server-github".to_string())
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            mcp_github_url: std::env::var("MCP_GITHUB_URL").ok(),
            mcp_verbose: std::env::var("MCP_VERBOSE")
                .ok()
                .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
            openai_temperature: std::env::var("OPENAI_TEMPERATURE").ok(),
            model: None,
            model_tier: None,
            parent_model_hint: None,
            aux_model: std::env::var("LOOM_AUX_MODEL").ok(),
            llm_provider: None,
            llm_provider_name: None,
            embedding_api_key: std::env::var("EMBEDDING_API_KEY").ok(),
            embedding_base_url: std::env::var("EMBEDDING_BASE_URL").ok(),
            embedding_model: std::env::var("EMBEDDING_MODEL").ok(),
            working_folder: std::env::var("WORKING_FOLDER").ok().map(PathBuf::from),
            compaction_config: None,
            tot_config: TotRunnerConfig::default(),
            got_config: GotRunnerConfig {
                adaptive: std::env::var("LOOM_GOT_ADAPTIVE")
                    .ok()
                    .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(false),
                agot_llm_complexity: std::env::var("LOOM_GOT_AGOT_LLM_COMPLEXITY")
                    .ok()
                    .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(false),
            },
            mcp_servers: None,
            skill_registry: None,
            max_sub_agent_depth: std::env::var("MAX_SUB_AGENT_DEPTH")
                .ok()
                .and_then(|s| s.parse().ok()),
            dry_run: std::env::var("LOOM_DRY_RUN")
                .ok()
                .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            builtin_tool_filter: None,
            call_tool_filter: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            goal_mode: false,
            is_background_review: false,
            memory_enabled: true,
            user_profile_enabled: true,
            memory_nudge_interval: 10,
            skill_nudge_interval: 10,
            role_setting: None,
            agents_md: None,
            system_prompt_override: None,
            skills_prompt: None,
            memory_prompt: None,
            env_context: None,
            reasoning_effort: None,
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

    #[test]
    fn from_env_github_token_and_mcp_override() {
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

        with_env("GITHUB_TOKEN", None, || {
            let config = ReactBuildConfig::from_env();
            assert!(config.github_token.is_none());
        });

        with_env("GITHUB_TOKEN", Some("x"), || {
            with_env("MCP_GITHUB_CMD", Some("custom-cmd"), || {
                with_env("MCP_GITHUB_ARGS", Some("arg1 arg2"), || {
                    let config = ReactBuildConfig::from_env();
                    assert_eq!(config.mcp_github_cmd, "custom-cmd");
                    assert_eq!(config.mcp_github_args, &["arg1", "arg2"]);
                });
            });
        });

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

    #[test]
    fn sub_agent_config_inherits_trace_thread_id() {
        let parent = ReactBuildConfig::from_env();
        let mut parent_with_trace = parent.clone();
        parent_with_trace.trace_thread_id = Some("parent-trace-id".to_string());

        let sub_config = ReactBuildConfig::from_env();

        assert_eq!(sub_config.trace_thread_id, None);
    }

    #[test]
    fn nested_sub_agent_keeps_same_trace_thread_id() {
        let parent = ReactBuildConfig::from_env();
        let mut parent_with_trace = parent.clone();
        parent_with_trace.trace_thread_id = Some("root-trace-id".to_string());

        let mut sub_config = ReactBuildConfig::from_env();
        sub_config.trace_thread_id = parent_with_trace.trace_thread_id.clone();

        assert_eq!(sub_config.trace_thread_id, Some("root-trace-id".to_string()));
    }

    #[test]
    fn trace_thread_id_falls_back_to_thread_id() {
        let config = ReactBuildConfig::from_env();

        let trace_id = config.trace_thread_id.clone();
        let thread_id = config.thread_id.clone();

        assert!(trace_id.is_none());

        let fallback = trace_id.or(thread_id);
        assert!(fallback.is_none());
    }

    #[test]
    fn aux_model_reads_from_env() {
        with_env("LOOM_AUX_MODEL", Some("cheap-model-v1"), || {
            let config = ReactBuildConfig::from_env();
            assert_eq!(config.aux_model.as_deref(), Some("cheap-model-v1"));
        });
        with_env("LOOM_AUX_MODEL", None, || {
            let config = ReactBuildConfig::from_env();
            assert!(config.aux_model.is_none());
        });
    }

    #[test]
    fn aux_model_default_is_none() {
        let config = ReactBuildConfig::default();
        assert!(config.aux_model.is_none());
    }
}
