use loom_types::config::BuiltinToolFilter;
use std::collections::HashSet;

/// Tool whitelist for background review agents.
///
/// **Alignment with Hermes** (`agent/background_review.py:459-465`): the review
/// fork installs a thread-local whitelist derived from `get_tool_definitions(
/// enabled_toolsets=["memory", "skills"])`. Hermes's Python tool name is
/// `skills_list` (plural) — see `hermes-agent/tools/skills_tool.py:1449` —
/// but Loom's unified Rust implementation renamed it to `skill_list`
/// (singular) to match the rest of the `skill_*` family (`skill_view`,
/// `skill_manage`). See `loom_types::tools::tool_name::TOOL_SKILL_LIST`. The
/// other three names are identical between Python and Rust.
pub const REVIEW_ALLOWED_TOOLS: &[&str] = &[
    "memory",
    "skill_list",
    "skill_view",
    "skill_manage",
];

#[derive(Clone)]
pub struct ReviewToolGate {
    allowed: HashSet<String>,
}

impl Default for ReviewToolGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewToolGate {
    pub fn new() -> Self {
        Self {
            allowed: REVIEW_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn with_allowed<I, S>(allowed: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: allowed.into_iter().map(Into::into).collect(),
        }
    }

    pub fn is_allowed(&self, name: &str) -> bool {
        self.allowed.contains(name)
    }

    pub fn allowed(&self) -> &HashSet<String> {
        &self.allowed
    }

    pub fn as_builtin_filter(&self) -> BuiltinToolFilter {
        BuiltinToolFilter {
            enabled: Some(self.allowed.iter().cloned().collect()),
            disabled: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gate_allows_only_review_tools() {
        let gate = ReviewToolGate::new();
        assert!(gate.is_allowed("memory"));
        assert!(gate.is_allowed("skill_list"));
        assert!(gate.is_allowed("skill_view"));
        assert!(gate.is_allowed("skill_manage"));
    }

    #[test]
    fn default_gate_denies_legacy_split_skill_tools() {
        // Legacy split tool names must NOT be whitelisted — the unified
        // `skill_manage` replaces them (alignment with Hermes).
        let gate = ReviewToolGate::new();
        assert!(!gate.is_allowed("skill_create"));
        assert!(!gate.is_allowed("skill_edit"));
        assert!(!gate.is_allowed("skill_patch"));
        assert!(!gate.is_allowed("skill_delete"));
        assert!(!gate.is_allowed("skill_write_file"));
        assert!(!gate.is_allowed("skill_remove_file"));
    }

    #[test]
    fn default_gate_denies_dangerous_tools() {
        let gate = ReviewToolGate::new();
        assert!(!gate.is_allowed("bash"));
        assert!(!gate.is_allowed("write_file"));
        assert!(!gate.is_allowed("read"));
        assert!(!gate.is_allowed("delete_file"));
        assert!(!gate.is_allowed("edit"));
        assert!(!gate.is_allowed("websearch"));
        assert!(!gate.is_allowed(""));
    }

    #[test]
    fn as_builtin_filter_produces_enabled_whitelist() {
        let gate = ReviewToolGate::new();
        let filter = gate.as_builtin_filter();
        let enabled = filter.enabled.expect("enabled should be set");
        assert!(enabled.contains(&"memory".to_string()));
        assert!(enabled.contains(&"skill_manage".to_string()));
        assert!(!enabled.contains(&"skill_create".to_string()));
        assert!(!enabled.contains(&"bash".to_string()));
    }

    #[test]
    fn as_builtin_filter_passes_builtin_filter_is_allowed() {
        let gate = ReviewToolGate::new();
        let filter = gate.as_builtin_filter();
        assert!(filter.is_allowed("memory"));
        assert!(filter.is_allowed("skill_manage"));
        assert!(!filter.is_allowed("skill_create"));
        assert!(!filter.is_allowed("bash"));
        assert!(!filter.is_allowed("write_file"));
    }

    #[test]
    fn with_allowed_supports_custom_whitelist() {
        let gate = ReviewToolGate::with_allowed(vec!["memory", "skill_list"]);
        assert!(gate.is_allowed("memory"));
        assert!(gate.is_allowed("skill_list"));
        assert!(!gate.is_allowed("skill_create"));
        assert!(!gate.is_allowed("bash"));
    }

    #[test]
    fn allowed_returns_reference_to_set() {
        let gate = ReviewToolGate::new();
        let allowed = gate.allowed();
        assert!(allowed.contains("memory"));
        assert!(allowed.contains("skill_view"));
    }
}
