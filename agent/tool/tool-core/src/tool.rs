use async_trait::async_trait;
use serde_json::Value;

use crate::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

/// A reference file bundled alongside a builtin skill.
///
/// Mirrors the role of `references/*.md` for filesystem skills: when the
/// registry loads a builtin skill, these files are listed under
/// `## Additional resources` and the agent can `read` them on demand.
pub type EmbeddedRef = (String, String);

/// A skill bundled inside a Tool crate, embedded in the binary via `include_str!`.
///
/// Tools that have detailed usage guidance beyond what fits in `ToolSpec.description`
/// can return one of these from [`Tool::builtin_skill`]. The Loom skill registry
/// injects them on startup; the agent loads them on demand via the `skill` tool.
#[derive(Debug, Clone)]
pub struct BuiltinSkill {
    pub name: String,
    pub description: String,
    /// Full SKILL.md content (including YAML frontmatter).
    pub content: String,
    pub triggers: Vec<String>,
    /// Tool names this skill depends on. The skill is hidden by
    /// `apply_toolset_filters` when none of these tools are registered.
    pub requires_tools: Vec<String>,
    /// Optional reference files bundled with the skill. Each entry is
    /// `(name, content)` - the name is the relative path shown to the agent
    /// (e.g. `"references/architecture-header.md"`), and the content is the
    /// full file body (already loaded via `include_str!` at compile time).
    /// The registry exposes them under `## Additional resources` so the
    /// agent can `read` them on demand, the same way filesystem skills
    /// expose `references/*.md`. Empty by default.
    pub references: Vec<EmbeddedRef>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn spec(&self) -> ToolSpec;

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;

    /// Optional: a reference skill bundled with this tool.
    ///
    /// Return `Some(_)` to make the skill discoverable by the Loom agent.
    /// The skill's content is embedded in the binary — no filesystem
    /// dependency at runtime.
    fn builtin_skill(&self) -> Option<BuiltinSkill> {
        None
    }
}
