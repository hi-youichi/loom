//! YAML-backed tool specs: load tool definitions from a folder of YAML files at runtime.
//!
//! Each tool has one file under `loom/tools/*.yaml`, embedded at compile time via
//! `include_str!` and parsed when building the tool source. Specs from YAML override the Rust
//! tool specs for `list_tools()`; execution still dispatches to the registered Rust `Tool`
//! implementations. Add a new line to `TOOL_YAML_FILES` when adding a tool YAML.

use std::collections::HashMap;

use async_trait::async_trait;
use thiserror::Error;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec};

macro_rules! embed_tool_yaml {
    ($($path:literal),+ $(,)?) => {
        &[ $( include_str!($path) ),+ ]
    };
}

const TOOL_YAML_FILES: &[&str] = embed_tool_yaml!(
    "../../../loom/tools/bash.yaml",
    "../../../loom/tools/batch.yaml",
    "../../../loom/tools/powershell.yaml",
    "../../../loom/tools/web_fetcher.yaml",
    "../../../loom/tools/read.yaml",
    "../../../loom/tools/write_file.yaml",
    "../../../loom/tools/ls.yaml",
    "../../../loom/tools/glob.yaml",
    "../../../loom/tools/grep.yaml",
    "../../../loom/tools/multiedit.yaml",
    "../../../loom/tools/move_file.yaml",
    "../../../loom/tools/apply_patch.yaml",
    "../../../loom/tools/date.yaml",
    "../../../loom/tools/delete_file.yaml",
    "../../../loom/tools/create_dir.yaml",
    "../../../loom/tools/remember.yaml",
    "../../../loom/tools/recall.yaml",
    "../../../loom/tools/search_memories.yaml",
    "../../../loom/tools/list_memories.yaml",
    "../../../loom/tools/get_recent_messages.yaml",
    "../../../loom/tools/todo_write.yaml",
    "../../../loom/tools/todo_read.yaml",
    "../../../loom/tools/twitter_search.yaml",
    "../../../loom/tools/websearch.yaml",
    "../../../loom/tools/codesearch.yaml",
    "../../../loom/tools/skill.yaml",
    "../../../loom/tools/lsp.yaml",
    "../../../loom/tools/invoke_agent.yaml",
    "../../../loom/tools/list_agents.yaml",
    "../../../loom/tools/help.yaml",
    "../../../loom/tools/task_create.yaml",
    "../../../loom/tools/task_show.yaml",
    "../../../loom/tools/task_list.yaml",
    "../../../loom/tools/task_update.yaml",
    "../../../loom/tools/task_delete.yaml",
);

#[derive(Debug, Error)]
pub enum YamlSpecError {
    #[error("failed to parse tool YAML ({name}): {message}")]
    Parse { name: String, message: String },
    #[error("failed to list tools from inner source: {0}")]
    ListTools(String),
}

/// Loads tool specs from the embedded YAML files (one spec per file).
///
/// Called when building the default tool source. Returns a list of specs that can be used to
/// override the registry specs for `list_tools()`. Tools not present in YAML keep their
/// Rust `spec()` when merging.
pub fn load_tool_specs() -> Result<Vec<ToolSpec>, YamlSpecError> {
    let mut specs = Vec::with_capacity(TOOL_YAML_FILES.len());
    for (i, yaml_str) in TOOL_YAML_FILES.iter().enumerate() {
        let spec: ToolSpec = serde_yaml::from_str(yaml_str).map_err(|e| YamlSpecError::Parse {
            name: format!("file_{}", i),
            message: e.to_string(),
        })?;
        specs.push(spec);
    }
    Ok(specs)
}

/// Wraps an inner `ToolSource` and overrides `list_tools()` with merged specs from YAML.
///
/// For each tool name returned by the inner source, if a spec exists in the loaded YAML it is
/// used; otherwise the inner spec is kept. `call_tool`, `call_tool_with_context`, and
/// `set_call_context` are delegated to the inner source.
pub struct YamlSpecToolSource {
    inner: Box<dyn ToolSource>,
    specs: Vec<ToolSpec>,
}

impl YamlSpecToolSource {
    /// Builds a wrapper that uses YAML specs for listing and delegates calls to `inner`.
    ///
    /// Loads the embedded YAML files, gets the list from `inner`, then merges: for each tool in
    /// `inner`'s list, use the YAML spec if present, else keep the inner spec. Returns an
    /// error if any YAML fails to parse.
    pub async fn wrap(inner: Box<dyn ToolSource>) -> Result<Self, YamlSpecError> {
        let registered = inner
            .list_tools()
            .await
            .map_err(|e| YamlSpecError::ListTools(e.to_string()))?;
        let yaml_specs = load_tool_specs()?;
        let yaml_map: HashMap<String, ToolSpec> = yaml_specs
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect();
        let specs: Vec<ToolSpec> = registered
            .into_iter()
            .map(|r| yaml_map.get(&r.name).cloned().unwrap_or(r))
            .collect();
        Ok(Self { inner, specs })
    }
}

#[async_trait]
impl ToolSource for YamlSpecToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        Ok(self.specs.clone())
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.inner.call_tool(name, arguments, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_tool_specs_returns_builtin_tools() {
        let specs = load_tool_specs().expect("tools/*.yaml must parse");
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        #[cfg(not(windows))]
        assert!(names.contains(&"bash"), "expected bash in {:?}", names);
        #[cfg(windows)]
        assert!(
            names.contains(&"powershell"),
            "expected powershell in {:?}",
            names
        );
        assert!(
            names.contains(&"web_fetcher"),
            "expected web_fetcher in {:?}",
            names
        );
        assert!(names.contains(&"read"), "expected read in {:?}", names);
    }
}
