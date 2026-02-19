//! Tools config block for run config summary.
//!
//! Implements [`ConfigSection`](super::ConfigSection). Used by CLI to build
//! the "Tools" line (e.g. tools=memory,exa exa_url=...).

use super::ConfigSection;

/// Tool sources summary: list of sources and optional Exa URL.
///
/// Built from `RunConfig` tool_source and mcp_exa_url. Implements [`ConfigSection`].
pub struct ToolConfigSummary {
    /// Tool source names, e.g. `["memory"]`, `["memory", "exa"]`.
    pub sources: Vec<String>,
    /// Exa MCP URL when Exa is enabled (optional display).
    pub exa_url: Option<String>,
}

impl ConfigSection for ToolConfigSummary {
    fn section_name(&self) -> &str {
        "Tools"
    }

    fn entries(&self) -> Vec<(&'static str, String)> {
        let tools = self.sources.join(",");
        let mut out = vec![("tools", tools)];
        if let Some(ref u) = self.exa_url {
            out.push(("exa_url", u.clone()));
        }
        out
    }
}
