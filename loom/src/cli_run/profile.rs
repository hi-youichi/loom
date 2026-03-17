//! Agent Profile: load and resolve YAML profile (role, model, MCP, etc.).
//!
//! Phase 3: extends + merge; project + user ~/.loom/agents; .md and front matter.
//! Built-in agent "dev" is loaded from crate `loom/agents/dev/` at compile time (instructions.md + config.yaml).

use crate::cli_run::RunOptions;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("read profile {0}: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("parse profile {0}: {1}")]
    Parse(PathBuf, serde_yaml::Error),
    #[error("profile not found: {0}")]
    NotFound(String),
}

/// Agent Profile (YAML or front matter). Phase 3: extends + merge.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AgentProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub role: Option<RoleConfig>,
    #[serde(default)]
    pub tools: Option<ToolsConfig>,
    #[serde(default)]
    pub model: Option<ModelConfig>,
    #[serde(default)]
    pub behavior: Option<BehaviorConfig>,
    #[serde(default)]
    pub environment: Option<EnvironmentConfig>,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub skills: Option<SkillsConfig>,
    /// Directory containing this agent's profile (set at load time, not from YAML).
    #[serde(skip)]
    pub source_dir: Option<PathBuf>,
}

/// Skills configuration within an agent profile.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillsConfig {
    /// Additional directories to scan for skills.
    #[serde(default)]
    pub dirs: Option<Vec<String>>,
    /// Whitelist: only these skills are available (empty = all).
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
    /// Blacklist: these skills are excluded.
    #[serde(default)]
    pub disabled: Option<Vec<String>>,
    /// Skills whose full content is injected into system prompt at startup.
    #[serde(default)]
    pub preload: Option<Vec<String>>,
}

/// Built-in dev agent: instructions embedded at compile time (loom/agents/dev/).
const DEV_AGENT_INSTRUCTIONS: &str = include_str!("../../agents/dev/instructions.md");
const DEV_AGENT_CONFIG_YAML: &str = include_str!("../../agents/dev/config.yaml");

/// Built-in agent-builder: meta-agent that creates new agent profiles (loom/agents/agent-builder/).
const AGENT_BUILDER_INSTRUCTIONS: &str =
    include_str!("../../agents/agent-builder/instructions.md");
const AGENT_BUILDER_CONFIG_YAML: &str =
    include_str!("../../agents/agent-builder/config.yaml");

/// Built-in explore agent: file search specialist for codebase navigation (loom/agents/explore/).
const EXPLORE_AGENT_INSTRUCTIONS: &str = include_str!("../../agents/explore/instructions.md");
const EXPLORE_AGENT_CONFIG_YAML: &str = include_str!("../../agents/explore/config.yaml");

/// Built-in orchestrator agent: task decomposition and multi-agent delegation (loom/agents/orchestrator/).
const ORCHESTRATOR_AGENT_INSTRUCTIONS: &str =
    include_str!("../../agents/orchestrator/instructions.md");
const ORCHESTRATOR_AGENT_CONFIG_YAML: &str =
    include_str!("../../agents/orchestrator/config.yaml");

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RoleConfig {
    #[serde(default)]
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub builtin: Option<BuiltinToolsConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BuiltinToolsConfig {
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
    #[serde(default)]
    pub disabled: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub config: Option<PathBuf>,
    #[serde(default)]
    pub servers: Option<Vec<McpServerConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BehaviorConfig {
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub timeout: Option<u32>,
    /// Maximum nesting depth for `invoke_agent` calls (default 3).
    #[serde(default)]
    pub max_sub_agent_depth: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnvironmentConfig {
    #[serde(default)]
    pub working_folder: Option<PathBuf>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// Splits content into (YAML block, optional body). If content starts with "---\n" and has a second "---",
/// returns (yaml_slice, Some(body)); otherwise (full_content, None).
fn parse_front_matter(content: &str) -> (&str, Option<String>) {
    const DELIM: &str = "---";
    if !content.starts_with(DELIM) {
        return (content, None);
    }
    let rest = match content.get(DELIM.len()..) {
        Some(r) => r,
        None => return (content, None),
    };
    if !rest.starts_with('\n') {
        return (content, None);
    }
    let after_first = &rest[1..];
    let sep = match after_first.find(DELIM) {
        Some(i) => i,
        None => return (content, None),
    };
    let yaml_str = after_first[..sep].trim_start_matches('\n');
    let body = after_first[sep + DELIM.len()..].trim_start_matches('\n');
    (yaml_str, Some(body.to_string()))
}

/// Resolves base profile path for `extends: name`. Same directory as current path: name.yaml, name.yml, name/config.yaml, name/config.yml, name.md.
fn resolve_extends_path(parent: &Path, extends: &str) -> Option<PathBuf> {
    let with_ext = parent.join(extends);
    let candidates = [
        with_ext.with_extension("yaml"),
        with_ext.with_extension("yml"),
        with_ext.with_extension("md"),
        parent.join(extends).join("config.yaml"),
        parent.join(extends).join("config.yml"),
        parent.join(extends).join("config.md"),
    ];
    for p in &candidates {
        if p.exists() {
            return Some(p.clone());
        }
    }
    if with_ext.extension().is_some() && with_ext.exists() {
        return Some(with_ext);
    }
    None
}

/// Merges base and override. Override wins for simple values and arrays; objects merged recursively.
/// Special: `tools.builtin.disabled` is combined (base + override, deduped).
fn merge_profiles(mut base: AgentProfile, over: AgentProfile) -> AgentProfile {
    if !over.name.is_empty() {
        base.name = over.name;
    }
    if over.description.is_some() {
        base.description = over.description;
    }
    if over.version.is_some() {
        base.version = over.version;
    }
    if over.role.is_some() {
        base.role = over.role;
    }
    if over.model.is_some() {
        base.model = over.model;
    }
    if over.behavior.is_some() {
        base.behavior = over.behavior;
    }
    if over.environment.is_some() {
        base.environment = over.environment;
    }
    if over.tools.is_some() {
        base.tools = Some(merge_tools_config(
            base.tools.take().unwrap_or_default(),
            over.tools.unwrap_or_default(),
        ));
    }
    if over.skills.is_some() {
        base.skills = over.skills;
    }
    base.extends = None;
    base
}

fn merge_tools_config(base: ToolsConfig, over: ToolsConfig) -> ToolsConfig {
    let builtin = match (base.builtin, over.builtin) {
        (None, None) => None,
        (Some(b), None) => Some(b),
        (None, Some(o)) => Some(BuiltinToolsConfig {
            enabled: o.enabled,
            disabled: merge_disabled_lists(None, o.disabled),
        }),
        (Some(mut b), Some(o)) => {
            if o.enabled.is_some() {
                b.enabled = o.enabled;
            }
            let merged_disabled = merge_disabled_lists(b.disabled.take(), o.disabled);
            b.disabled = merged_disabled;
            Some(b)
        }
    };
    let mcp = over.mcp.or(base.mcp);
    ToolsConfig { builtin, mcp }
}

fn merge_disabled_lists(a: Option<Vec<String>>, b: Option<Vec<String>>) -> Option<Vec<String>> {
    let mut v: Vec<String> = a.unwrap_or_default();
    v.extend(b.unwrap_or_default());
    if v.is_empty() {
        None
    } else {
        v.sort_unstable();
        v.dedup();
        Some(v)
    }
}

/// Load a single profile from path. Supports pure YAML or front matter (---\nYAML\n---\nbody).
/// Resolves role.file relative to profile dir. If `extends` is set, loads base and merges.
pub fn load_agent_profile(path: &Path) -> Result<AgentProfile, ProfileError> {
    let content = std::fs::read_to_string(path).map_err(|e| ProfileError::Read(path.to_path_buf(), e))?;
    let (yaml_str, role_body) = parse_front_matter(&content);
    let mut profile: AgentProfile =
        serde_yaml::from_str(yaml_str).map_err(|e| ProfileError::Parse(path.to_path_buf(), e))?;
    if let Some(body) = role_body {
        profile.role = Some(RoleConfig {
            file: None,
            content: Some(body),
        });
    }

    let parent = path.parent().unwrap_or(Path::new("."));
    if let Some(ref mut role) = profile.role {
        if let Some(ref file) = role.file {
            let role_path = parent.join(file);
            let s = std::fs::read_to_string(&role_path)
                .map_err(|e| ProfileError::Read(role_path.clone(), e))?;
            role.content = Some(s.trim().to_string());
        }
    }

    if let Some(ref extends) = profile.extends {
        let base_path = resolve_extends_path(parent, extends)
            .ok_or_else(|| ProfileError::NotFound(extends.clone()))?;
        let base = load_agent_profile(&base_path)?;
        profile = merge_profiles(base, profile);
    }

    profile.source_dir = Some(parent.to_path_buf());

    Ok(profile)
}

/// Resolve named profile path: project .loom/agents first, then ~/.loom/agents. Supports .yaml, .yml, .md.
pub fn resolve_named_profile(name: &str) -> Option<PathBuf> {
    let project_agents = PathBuf::from(".loom/agents");
    let tries = [
        project_agents.join(name).join("config.yaml"),
        project_agents.join(name).join("config.yml"),
        project_agents.join(name).join("config.md"),
        project_agents.join(format!("{}.yaml", name)),
        project_agents.join(format!("{}.yml", name)),
        project_agents.join(format!("{}.md", name)),
    ];
    for p in &tries {
        if p.exists() {
            return Some(p.clone());
        }
    }
    let user_agents = env_config::home::loom_home().join("agents");
    let user_tries = [
        user_agents.join(name).join("config.yaml"),
        user_agents.join(name).join("config.yml"),
        user_agents.join(name).join("config.md"),
        user_agents.join(format!("{}.yaml", name)),
        user_agents.join(format!("{}.yml", name)),
        user_agents.join(format!("{}.md", name)),
    ];
    for p in &user_tries {
        if p.exists() {
            return Some(p.clone());
        }
    }
    None
}

/// Find default profile path: project .loom/agents/default, then cwd agent.yaml/agent.yml, then ~/.loom/agents/default.
pub fn find_default_profile() -> Option<PathBuf> {
    let project_agents = PathBuf::from(".loom/agents");
    let candidates = [
        project_agents.join("default").join("config.yaml"),
        project_agents.join("default").join("config.yml"),
        project_agents.join("default").join("config.md"),
        project_agents.join("default.yaml"),
        project_agents.join("default.yml"),
        project_agents.join("default.md"),
        PathBuf::from("agent.yaml"),
        PathBuf::from("agent.yml"),
    ];
    for p in &candidates {
        if p.exists() {
            return Some(p.clone());
        }
    }
    let user_agents = env_config::home::loom_home().join("agents");
    let user_candidates = [
        user_agents.join("default").join("config.yaml"),
        user_agents.join("default").join("config.yml"),
        user_agents.join("default").join("config.md"),
        user_agents.join("default.yaml"),
        user_agents.join("default.yml"),
        user_agents.join("default.md"),
    ];
    for p in &user_candidates {
        if p.exists() {
            return Some(p.clone());
        }
    }
    None
}

/// Load profile from RunOptions: --agent name → built-in agents (compile-time) or resolve_named_profile; else find_default_profile. On error returns None (fallback to no profile).
/// Returns the loaded profile together with its source (BuiltIn / Project / User / Default).
pub fn load_profile_from_options(opts: &RunOptions) -> Option<(AgentProfile, ProfileSource)> {
    if let Some(ref name) = opts.agent {
        if let Some(mut profile) = load_builtin_profile(name) {
            let project_dir = PathBuf::from(".loom/agents").join(name);
            if project_dir.is_dir() {
                profile.source_dir = Some(project_dir);
            }
            return Some((profile, ProfileSource::BuiltIn));
        }
        let path = resolve_named_profile(name)?;
        let source = classify_profile_path(&path);
        return load_agent_profile(&path).ok().map(|p| (p, source));
    }
    let path = find_default_profile()?;
    let source = classify_profile_path(&path);
    load_agent_profile(&path).ok().map(|p| (p, source))
}

/// Classify a profile path as Project or User based on its location.
fn classify_profile_path(path: &Path) -> ProfileSource {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let user_agents = env_config::home::loom_home().join("agents");
    if let Ok(user_abs) = user_agents.canonicalize() {
        if abs.starts_with(&user_abs) {
            return ProfileSource::User;
        }
    }
    ProfileSource::Project
}

/// Built-in agent names (compile-time embedded).
const BUILTIN_AGENT_NAMES: &[&str] = &["dev", "agent-builder", "explore", "orchestrator"];

/// Resolve an agent profile by name at runtime. Tries built-in agents first,
/// then project-level `.loom/agents/<name>/`, then user-level `~/.loom/agents/<name>/`.
///
/// This is the primary API for `InvokeAgentTool` to load a sub-agent profile
/// without depending on `RunOptions`.
pub fn resolve_profile(name: &str) -> Result<AgentProfile, ProfileError> {
    if let Some(mut profile) = load_builtin_profile(name) {
        let project_dir = PathBuf::from(".loom/agents").join(name);
        if project_dir.is_dir() {
            profile.source_dir = Some(project_dir);
        }
        return Ok(profile);
    }
    let path = resolve_named_profile(name)
        .ok_or_else(|| ProfileError::NotFound(name.to_string()))?;
    load_agent_profile(&path)
}

/// Where a profile was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSource {
    BuiltIn,
    Project,
    User,
}

impl std::fmt::Display for ProfileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileSource::BuiltIn => write!(f, "built-in"),
            ProfileSource::Project => write!(f, "project"),
            ProfileSource::User => write!(f, "user"),
        }
    }
}

/// Lightweight summary of a discovered agent profile.
#[derive(Debug, Clone)]
pub struct ProfileSummary {
    pub name: String,
    pub description: Option<String>,
    pub source: ProfileSource,
}

/// List all available agent profiles (built-in + project + user).
pub fn list_available_profiles() -> Vec<ProfileSummary> {
    let mut profiles = Vec::new();

    for &name in BUILTIN_AGENT_NAMES {
        if let Some(p) = load_builtin_profile(name) {
            profiles.push(ProfileSummary {
                name: p.name.clone(),
                description: p.description.clone(),
                source: ProfileSource::BuiltIn,
            });
        }
    }

    let scan_dirs: Vec<(PathBuf, ProfileSource)> = vec![
        (PathBuf::from(".loom/agents"), ProfileSource::Project),
        (env_config::home::loom_home().join("agents"), ProfileSource::User),
    ];

    for (dir, source) in &scan_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if profiles.iter().any(|p| p.name == name) {
                    continue;
                }
                let profile = if path.is_dir() {
                    let config = path.join("config.yaml");
                    if config.exists() {
                        load_agent_profile(&config).ok()
                    } else {
                        let config_yml = path.join("config.yml");
                        if config_yml.exists() {
                            load_agent_profile(&config_yml).ok()
                        } else {
                            None
                        }
                    }
                } else if path.extension().and_then(|e| e.to_str()) == Some("yaml")
                    || path.extension().and_then(|e| e.to_str()) == Some("yml")
                    || path.extension().and_then(|e| e.to_str()) == Some("md")
                {
                    load_agent_profile(&path).ok()
                } else {
                    None
                };
                if let Some(p) = profile {
                    profiles.push(ProfileSummary {
                        name: if p.name.is_empty() { name } else { p.name },
                        description: p.description,
                        source: source.clone(),
                    });
                }
            }
        }
    }

    profiles
}

/// Try to load a built-in agent by name. Returns None if not a built-in.
fn load_builtin_profile(name: &str) -> Option<AgentProfile> {
    let (config_yaml, instructions) = match name {
        "dev" => (DEV_AGENT_CONFIG_YAML, DEV_AGENT_INSTRUCTIONS),
        "agent-builder" => (AGENT_BUILDER_CONFIG_YAML, AGENT_BUILDER_INSTRUCTIONS),
        "explore" => (EXPLORE_AGENT_CONFIG_YAML, EXPLORE_AGENT_INSTRUCTIONS),
        "orchestrator" => (ORCHESTRATOR_AGENT_CONFIG_YAML, ORCHESTRATOR_AGENT_INSTRUCTIONS),
        _ => return None,
    };
    let mut profile: AgentProfile = serde_yaml::from_str(config_yaml).ok()?;
    profile
        .role
        .get_or_insert_with(RoleConfig::default)
        .content
        .replace(instructions.trim().to_string());
    Some(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_dev_agent_loaded_with_embedded_instructions() {
        let opts = RunOptions {
            message: String::new(),
            working_folder: None,
            session_id: None,
            thread_id: None,
            role_file: None,
            agent: Some("dev".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
        };
        let (profile, source) = load_profile_from_options(&opts).expect("built-in dev profile");
        assert_eq!(profile.name, "dev");
        assert_eq!(source, ProfileSource::BuiltIn);
        let role = profile.role.as_ref().unwrap();
        assert!(role.content.as_ref().unwrap().contains("Editing constraints"));
        assert!(role.content.as_ref().unwrap().contains("agent"));
    }

    #[test]
    fn builtin_agent_builder_loaded_with_embedded_instructions() {
        let opts = RunOptions {
            message: String::new(),
            working_folder: None,
            session_id: None,
            thread_id: None,
            role_file: None,
            agent: Some("agent-builder".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
        };
        let (profile, source) = load_profile_from_options(&opts).expect("built-in agent-builder profile");
        assert_eq!(profile.name, "agent-builder");
        assert_eq!(source, ProfileSource::BuiltIn);
        let role = profile.role.as_ref().unwrap();
        let content = role.content.as_ref().unwrap();
        assert!(content.contains("Loom Agent Builder"));
        assert!(content.contains("AgentProfile Schema Reference"));
    }

    #[test]
    fn load_builtin_profile_returns_none_for_unknown() {
        assert!(load_builtin_profile("nonexistent-agent").is_none());
    }

    #[test]
    fn deserialize_minimal_profile() {
        let yaml = r#"
name: test-agent
description: "A test"
"#;
        let p: AgentProfile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.name, "test-agent");
        assert_eq!(p.description.as_deref(), Some("A test"));
        assert!(p.role.is_none());
        assert!(p.model.is_none());
    }

    #[test]
    fn deserialize_with_role_content() {
        let yaml = r#"
name: foo
role:
  content: "You are helpful."
"#;
        let p: AgentProfile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.role.as_ref().unwrap().content.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn deserialize_invalid_yaml_returns_parse_error() {
        let yaml = "name: [ unclosed";
        let res: Result<AgentProfile, _> = serde_yaml::from_str(yaml);
        assert!(res.is_err());
    }

    #[test]
    fn parse_front_matter_splits_yaml_and_body() {
        let content = r#"---
name: debugger
description: Debug specialist
---
You are an expert debugger.
Focus on root cause.
"#;
        let (yaml_str, body) = parse_front_matter(content);
        assert!(yaml_str.contains("name: debugger"));
        assert!(yaml_str.contains("description: Debug specialist"));
        let b = body.expect("body present");
        assert!(b.starts_with("You are an expert"));
        assert!(b.contains("root cause"));
    }

    #[test]
    fn parse_front_matter_no_delimiter_returns_full_content() {
        let content = "name: foo\nrole:\n  content: hi";
        let (yaml_str, body) = parse_front_matter(content);
        assert_eq!(yaml_str, content);
        assert!(body.is_none());
    }

    #[test]
    fn load_profile_from_front_matter_like_content() {
        let content = r#"---
name: fm-agent
description: From front matter
---
You are a helpful assistant.
"#;
        let (yaml_str, role_body) = parse_front_matter(content);
        let profile: AgentProfile = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(profile.name, "fm-agent");
        assert_eq!(profile.description.as_deref(), Some("From front matter"));
        assert!(role_body.is_some());
        let role = role_body.unwrap();
        assert!(role.trim().starts_with("You are a helpful"));
    }

    #[test]
    fn merge_profiles_override_wins_simple_and_merged_disabled() {
        let base = AgentProfile {
            name: "base".to_string(),
            description: Some("Base".to_string()),
            tools: Some(ToolsConfig {
                builtin: Some(BuiltinToolsConfig {
                    enabled: Some(vec!["bash".to_string(), "read".to_string(), "websearch".to_string()]),
                    disabled: Some(vec!["web_fetcher".to_string()]),
                }),
                mcp: Some(McpConfig {
                    config: Some(PathBuf::from("./mcp.json")),
                    servers: None,
                }),
            }),
            ..Default::default()
        };
        let over = AgentProfile {
            name: "override".to_string(),
            description: Some("Over".to_string()),
            tools: Some(ToolsConfig {
                builtin: Some(BuiltinToolsConfig {
                    enabled: Some(vec!["bash".to_string(), "read".to_string()]),
                    disabled: Some(vec!["websearch".to_string()]),
                }),
                mcp: None,
            }),
            ..Default::default()
        };
        let merged = merge_profiles(base, over);
        assert_eq!(merged.name, "override");
        assert_eq!(merged.description.as_deref(), Some("Over"));
        let builtin = merged.tools.as_ref().unwrap().builtin.as_ref().unwrap();
        assert_eq!(builtin.enabled, Some(vec!["bash".to_string(), "read".to_string()]));
        let disabled: Vec<_> = builtin.disabled.as_ref().unwrap().iter().map(String::as_str).collect();
        assert!(disabled.contains(&"web_fetcher"));
        assert!(disabled.contains(&"websearch"));
        assert_eq!(disabled.len(), 2);
        assert_eq!(merged.tools.as_ref().unwrap().mcp.as_ref().unwrap().config.as_ref().unwrap(), &PathBuf::from("./mcp.json"));
    }

    #[test]
    fn merge_profiles_base_kept_when_override_empty() {
        let base = AgentProfile {
            name: "base".to_string(),
            model: Some(ModelConfig {
                name: Some("gpt-4".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let over = AgentProfile {
            name: "child".to_string(),
            ..Default::default()
        };
        let merged = merge_profiles(base, over);
        assert_eq!(merged.name, "child");
        assert_eq!(merged.model.as_ref().unwrap().name.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn load_agent_profile_with_extends_merges_from_base_file() {
        let dir = tempfile::tempdir().unwrap();
        let base_yaml = r#"
name: base
model:
  name: gpt-4
tools:
  mcp:
    config: ./mcp.json
"#;
        let child_yaml = r#"
extends: base
name: child
description: Child profile
tools:
  builtin:
    disabled: [websearch]
"#;
        std::fs::write(dir.path().join("base.yaml"), base_yaml).unwrap();
        std::fs::write(dir.path().join("child.yaml"), child_yaml).unwrap();
        let loaded = load_agent_profile(&dir.path().join("child.yaml")).unwrap();
        assert_eq!(loaded.name, "child");
        assert_eq!(loaded.description.as_deref(), Some("Child profile"));
        assert_eq!(loaded.model.as_ref().unwrap().name.as_deref(), Some("gpt-4"));
        assert!(loaded.tools.as_ref().unwrap().mcp.as_ref().unwrap().config.is_some());
        let disabled = loaded.tools.as_ref().unwrap().builtin.as_ref().unwrap().disabled.as_ref().unwrap();
        assert_eq!(disabled, &["websearch".to_string()]);
        assert!(loaded.extends.is_none());
    }

    #[test]
    fn resolve_profile_returns_builtin_dev() {
        let profile = resolve_profile("dev").expect("built-in dev profile");
        assert_eq!(profile.name, "dev");
        assert!(profile.role.as_ref().unwrap().content.is_some());
    }

    #[test]
    fn resolve_profile_returns_builtin_agent_builder() {
        let profile = resolve_profile("agent-builder").expect("built-in agent-builder profile");
        assert_eq!(profile.name, "agent-builder");
    }

    #[test]
    fn resolve_profile_returns_not_found_for_unknown() {
        let result = resolve_profile("nonexistent-agent-xyz");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProfileError::NotFound(_)));
    }

    #[test]
    fn list_available_profiles_includes_builtins() {
        let profiles = list_available_profiles();
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"dev"), "missing dev in {:?}", names);
        assert!(
            names.contains(&"agent-builder"),
            "missing agent-builder in {:?}",
            names
        );
        for p in &profiles {
            if p.name == "dev" || p.name == "agent-builder" {
                assert_eq!(p.source, ProfileSource::BuiltIn);
            }
        }
    }

    #[test]
    fn builtin_explore_agent_loaded() {
        let profile = load_builtin_profile("explore").unwrap();
        assert_eq!(profile.name, "explore");
        assert!(profile.role.as_ref().unwrap().content.is_some());
    }

    #[test]
    fn resolve_profile_explore() {
        let profile = resolve_profile("explore").expect("built-in explore profile");
        assert_eq!(profile.name, "explore");
    }

    #[test]
    fn load_profile_from_options_no_agent_no_default() {
        let dir = tempfile::tempdir().unwrap();
        let prev_dir = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(dir.path());
        let prev_loom = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let opts = RunOptions {
            message: String::new(),
            working_folder: None,
            session_id: None,
            thread_id: None,
            role_file: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
        };
        let result = load_profile_from_options(&opts);

        if let Some(d) = prev_dir {
            let _ = std::env::set_current_dir(d);
        }
        match prev_loom {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        assert!(result.is_none());
    }

    #[test]
    fn load_profile_from_options_unknown_agent() {
        let prev_loom = std::env::var("LOOM_HOME").ok();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());

        let opts = RunOptions {
            message: String::new(),
            working_folder: None,
            session_id: None,
            thread_id: None,
            role_file: None,
            agent: Some("nonexistent-agent-xyz".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
        };
        let result = load_profile_from_options(&opts);
        match prev_loom {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
        assert!(result.is_none());
    }

    #[test]
    fn merge_profiles_tools_none_none() {
        let base = AgentProfile::default();
        let over = AgentProfile::default();
        let merged = merge_profiles(base, over);
        assert!(merged.tools.is_none());
    }

    #[test]
    fn merge_profiles_override_skills() {
        let base = AgentProfile {
            skills: Some(SkillsConfig {
                dirs: Some(vec!["/base".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let over = AgentProfile {
            name: "over".to_string(),
            skills: Some(SkillsConfig {
                dirs: Some(vec!["/over".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_profiles(base, over);
        let dirs = merged.skills.unwrap().dirs.unwrap();
        assert_eq!(dirs, vec!["/over".to_string()]);
    }

    #[test]
    fn merge_tools_config_base_only_builtin() {
        let base = ToolsConfig {
            builtin: Some(BuiltinToolsConfig {
                enabled: Some(vec!["bash".to_string()]),
                disabled: None,
            }),
            mcp: None,
        };
        let over = ToolsConfig { builtin: None, mcp: None };
        let merged = merge_tools_config(base, over);
        assert_eq!(merged.builtin.unwrap().enabled, Some(vec!["bash".to_string()]));
    }

    #[test]
    fn merge_tools_config_over_only_builtin() {
        let base = ToolsConfig { builtin: None, mcp: None };
        let over = ToolsConfig {
            builtin: Some(BuiltinToolsConfig {
                enabled: Some(vec!["read".to_string()]),
                disabled: Some(vec!["bash".to_string()]),
            }),
            mcp: None,
        };
        let merged = merge_tools_config(base, over);
        let b = merged.builtin.unwrap();
        assert_eq!(b.enabled, Some(vec!["read".to_string()]));
        assert_eq!(b.disabled, Some(vec!["bash".to_string()]));
    }

    #[test]
    fn merge_disabled_lists_both_none() {
        assert!(merge_disabled_lists(None, None).is_none());
    }

    #[test]
    fn merge_disabled_lists_deduplicates() {
        let result = merge_disabled_lists(
            Some(vec!["a".to_string(), "b".to_string()]),
            Some(vec!["b".to_string(), "c".to_string()]),
        ).unwrap();
        assert_eq!(result, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn resolve_extends_path_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_extends_path(dir.path(), "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn resolve_extends_path_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base.yaml"), "name: base").unwrap();
        let result = resolve_extends_path(dir.path(), "base");
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("base.yaml"));
    }

    #[test]
    fn resolve_extends_path_yml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base.yml"), "name: base").unwrap();
        let result = resolve_extends_path(dir.path(), "base");
        assert!(result.is_some());
    }

    #[test]
    fn resolve_extends_path_subdir_config() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("parent");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("config.yaml"), "name: parent").unwrap();
        let result = resolve_extends_path(dir.path(), "parent");
        assert!(result.is_some());
    }

    #[test]
    fn load_agent_profile_with_role_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("role.md"), "You are a test agent.").unwrap();
        std::fs::write(
            dir.path().join("config.yaml"),
            "name: test\nrole:\n  file: role.md\n",
        )
        .unwrap();
        let profile = load_agent_profile(&dir.path().join("config.yaml")).unwrap();
        assert_eq!(profile.name, "test");
        assert_eq!(
            profile.role.as_ref().unwrap().content.as_deref(),
            Some("You are a test agent.")
        );
    }

    #[test]
    fn load_agent_profile_read_error() {
        let result = load_agent_profile(Path::new("/nonexistent/config.yaml"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProfileError::Read(_, _)));
    }

    #[test]
    fn load_agent_profile_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.yaml"), "name: [ unclosed").unwrap();
        let result = load_agent_profile(&dir.path().join("bad.yaml"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProfileError::Parse(_, _)));
    }

    #[test]
    fn profile_error_display() {
        let e = ProfileError::NotFound("test".to_string());
        assert!(e.to_string().contains("test"));
        assert!(e.to_string().contains("not found"));
    }

    #[test]
    fn profile_source_debug() {
        let s = format!("{:?}", ProfileSource::BuiltIn);
        assert!(s.contains("BuiltIn"));
    }

    #[test]
    fn profile_summary_debug() {
        let s = ProfileSummary {
            name: "test".to_string(),
            description: Some("desc".to_string()),
            source: ProfileSource::Project,
        };
        let d = format!("{:?}", s);
        assert!(d.contains("test"));
    }

    #[test]
    fn deserialize_full_profile() {
        let yaml = r#"
name: full
description: "Full profile"
version: "1.0"
role:
  content: "You are helpful"
model:
  name: gpt-4
  temperature: 0.7
  max_tokens: 4096
behavior:
  approval_policy: auto
  max_iterations: 10
  timeout: 300
  max_sub_agent_depth: 3
environment:
  working_folder: /tmp
  thread_id: t1
  user_id: u1
skills:
  dirs:
    - /extra/skills
  enabled:
    - code-review
  disabled:
    - debug
  preload:
    - code-review
tools:
  builtin:
    enabled: [bash, read]
    disabled: [websearch]
  mcp:
    config: ./mcp.json
"#;
        let p: AgentProfile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.name, "full");
        assert_eq!(p.version.as_deref(), Some("1.0"));
        assert!(p.model.as_ref().unwrap().temperature.is_some());
        assert!(p.behavior.as_ref().unwrap().max_sub_agent_depth.is_some());
        assert!(p.environment.as_ref().unwrap().working_folder.is_some());
        assert!(p.skills.as_ref().unwrap().preload.is_some());
    }

    #[test]
    fn resolve_named_profile_user_level() {
        let loom_home = tempfile::tempdir().unwrap();
        let agents_dir = loom_home.path().join("agents");
        let agent_dir = agents_dir.join("custom-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("config.yaml"),
            "name: custom-agent\ndescription: Custom\n",
        )
        .unwrap();

        let prev_loom = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());

        let prev_dir = std::env::current_dir().ok();
        let empty = tempfile::tempdir().unwrap();
        let _ = std::env::set_current_dir(empty.path());

        let result = resolve_named_profile("custom-agent");

        if let Some(d) = prev_dir {
            let _ = std::env::set_current_dir(d);
        }
        match prev_loom {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }

        assert!(result.is_some());
    }

    #[test]
    fn default_true_returns_true() {
        assert!(default_true());
    }
}
