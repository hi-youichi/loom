use async_trait::async_trait;
use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, Tool};

pub use loom_types::tools::tool_name::TOOL_HELP;

const HELP_TEXT: &str = r#"# Loom Help

## What is Loom?

Loom is an AI agent framework that connects LLMs to tools and external services through a ReAct (Reason-Act) loop. It supports multiple agent strategies (ReAct, Tree-of-Thought, Graph-of-Thought, Helve) and provides a rich set of built-in tools for file operations, shell execution, web access, memory, and more.

## Skills

Skills are reusable instruction sets stored as markdown files. They extend the agent's behavior for specific tasks without modifying the agent itself.

- Skills are loaded on demand via the `skill` tool.
- Use `skill` with `name="list"` to see all available skills.
- Skills are discovered from `.loom/skills/` in the working folder or from built-in skill registries.
- Each skill is a `.md` file containing instructions the agent follows when the skill is loaded.

## MCP (Model Context Protocol)

MCP is a standardized protocol for discovering and calling external tools. Loom connects to MCP servers to extend its capabilities beyond built-in tools.

- Configure MCP servers in `.loom/mcp.json` (project-level) or `~/.loom/mcp.json` (user-level).
- Both stdio and HTTP transports are supported.
- MCP tools appear alongside built-in tools — the agent uses them automatically when available.
- Common MCP integrations include GitHub, GitLab, filesystem servers, and custom tool servers.

## Quick Reference

- `skill name="list"` — list available skills
- `skill name="<name>"` — load a specific skill
- MCP tools are auto-discovered from configured servers
"#;

pub struct HelpTool;

impl HelpTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HelpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HelpTool {
    fn name(&self) -> &str {
        TOOL_HELP
    }

    fn spec(&self) -> tool_core::ToolSpec {
        ToolSpec {
            name: TOOL_HELP.to_string(),
            description: Some(
                "Show help information about Loom, including what it is, how Skills work, and how MCP (Model Context Protocol) is used.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent::text(HELP_TEXT.to_string()))
    }
}
