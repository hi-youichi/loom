use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_FILES;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::common::truncate_for_preview;
use crate::runtime::WorkflowRuntime;

pub struct WorkflowFilesTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowFilesTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowFilesTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_FILES
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_FILES.to_string(),
            description: Some(
                "List the available Lua workflow definitions. Returns each workflow's \
                 name, size, and first non-empty line. Pass a returned name to \
                 `workflow_start`."
                    .to_string(),
            ),
            input_schema: json!({"type": "object", "properties": {}}),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        _args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        list_workflow_files(&self.runtime)
    }
}

fn list_workflow_files(
    runtime: &WorkflowRuntime,
) -> Result<ToolCallContent, ToolSourceError> {
    let root = runtime.workflows_dir();
    let mut workflows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|v| v.to_str()) != Some("lua") {
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            let first_line = source
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default();
            workflows.push(json!({
                "name": name,
                "size_bytes": source.len(),
                "first_line": truncate_for_preview(first_line, 200),
            }));
        }
    }
    workflows.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .cmp(&right.get("name").and_then(Value::as_str))
    });

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "workflows": workflows,
            "count": workflows.len(),
        }))
        .unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;
    use std::path::Path;

    fn runtime_with(folder: &Path) -> Arc<WorkflowRuntime> {
        let cfg = AgentConfig {
            working_folder: Some(folder.to_path_buf()),
            ..Default::default()
        };
        Arc::new(WorkflowRuntime::new(cfg))
    }

    #[test]
    fn files_lists_lua_definitions_with_first_line() {
        let tmp = tempfile::tempdir().unwrap();
        let wf_dir = tmp.path().join(".loom").join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(
            wf_dir.join("alpha.lua"),
            "-- alpha\nfunction main() end\n",
        )
        .unwrap();
        std::fs::write(
            wf_dir.join("beta.lua"),
            "\n\n-- beta\nfunction main() end\n",
        )
        .unwrap();
        let ignored = wf_dir.join("ignore.txt");
        std::fs::write(ignored, "nope").unwrap();

        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let list = v["workflows"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["name"], "alpha.lua");
        assert_eq!(list[0]["first_line"], "-- alpha");
        assert_eq!(list[1]["name"], "beta.lua");
        assert_eq!(list[1]["first_line"], "-- beta");
    }

    #[test]
    fn files_returns_empty_when_no_workflows_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 0);
        assert!(v["workflows"].is_array());
    }

    #[test]
    fn files_excludes_non_lua_files() {
        let tmp = tempfile::tempdir().unwrap();
        let wf_dir = tmp.path().join(".loom").join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(wf_dir.join("readme.md"), "# docs").unwrap();
        std::fs::write(wf_dir.join("greet.lua"), "-- greet\nfunction main() end").unwrap();

        let rt = runtime_with(tmp.path());
        let result = list_workflow_files(&rt).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let list = v["workflows"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "greet.lua");
        assert_eq!(list[0]["first_line"], "-- greet");
    }
}