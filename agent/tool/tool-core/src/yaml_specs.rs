use thiserror::Error;

use crate::ToolSpec;

macro_rules! embed_tool_yaml {
    ($($path:literal),+ $(,)?) => {
        &[ $( include_str!($path) ),+ ]
    };
}

const TOOL_YAML_FILES: &[&str] = embed_tool_yaml!(
    "../../../../loom/tools/bash.yaml",
    "../../../../loom/tools/batch.yaml",
    "../../../../loom/tools/powershell.yaml",
    "../../../../loom/tools/web_fetcher.yaml",
    "../../../../loom/tools/read.yaml",
    "../../../../loom/tools/write_file.yaml",
    "../../../../loom/tools/ls.yaml",
    "../../../../loom/tools/glob.yaml",
    "../../../../loom/tools/grep.yaml",
    "../../../../loom/tools/multiedit.yaml",
    "../../../../loom/tools/move_file.yaml",
    "../../../../loom/tools/apply_patch.yaml",
    "../../../../loom/tools/date.yaml",
    "../../../../loom/tools/delete_file.yaml",
    "../../../../loom/tools/create_dir.yaml",
    "../../../../loom/tools/remember.yaml",
    "../../../../loom/tools/recall.yaml",
    "../../../../loom/tools/search_memories.yaml",
    "../../../../loom/tools/list_memories.yaml",
    "../../../../loom/tools/get_recent_messages.yaml",
    "../../../../loom/tools/todo_write.yaml",
    "../../../../loom/tools/todo_read.yaml",
    "../../../../loom/tools/twitter_search.yaml",
    "../../../../loom/tools/websearch.yaml",
    "../../../../loom/tools/codesearch.yaml",
    "../../../../loom/tools/skill.yaml",
    "../../../../loom/tools/lsp.yaml",
    "../../../../loom/tools/invoke_agent.yaml",
    "../../../../loom/tools/list_agents.yaml",
    "../../../../loom/tools/help.yaml",
    "../../../../loom/tools/task_create.yaml",
    "../../../../loom/tools/task_show.yaml",
    "../../../../loom/tools/task_list.yaml",
    "../../../../loom/tools/task_update.yaml",
    "../../../../loom/tools/task_delete.yaml",
);

#[derive(Debug, Error)]
pub enum YamlSpecError {
    #[error("failed to parse tool YAML ({name}): {message}")]
    Parse { name: String, message: String },
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_tool_specs_returns_builtin_tools() {
        let specs = load_tool_specs().expect("tools/*.yaml must parse");
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"read"), "expected read in {:?}", names);
        assert!(
            names.contains(&"web_fetcher"),
            "expected web_fetcher in {:?}",
            names
        );
    }
}
