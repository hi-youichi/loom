//! Skill tool: load a skill by name from a skill registry or from .loom/skills directory.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::skill::SkillRegistry;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for loading a skill.
pub const TOOL_SKILL: &str = "skill";

const SKILLS_SUBDIR: &str = ".loom/skills";

/// Tool that loads skill content by name. Prefers registry-based loading when a
/// [`SkillRegistry`] is provided; otherwise falls back to directory scan under working folder.
pub struct SkillTool {
    registry: Option<Arc<SkillRegistry>>,
    working_folder: Option<Arc<std::path::PathBuf>>,
}

impl SkillTool {
    /// Creates a skill tool that loads from the given registry (discovery-based).
    pub fn new_with_registry(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry: Some(registry),
            working_folder: None,
        }
    }

    /// Creates a skill tool that loads from the working folder's `.loom/skills` directory (legacy).
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self {
            registry: None,
            working_folder: Some(working_folder),
        }
    }

    fn skills_dir(&self) -> Option<std::path::PathBuf> {
        self.working_folder
            .as_ref()
            .map(|wf| wf.join(SKILLS_SUBDIR))
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        TOOL_SKILL
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_SKILL.to_string(),
            description: Some(
                "Load a skill by name. Use when a task matches one of the available skills listed in your instructions. \
                 Pass name=\"list\" to see all available skills with descriptions."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name to load (from <available_skills>), or \"list\" to list all."
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing name".to_string()))?;

        if let Some(ref registry) = self.registry {
            if name == "list" {
                let lines: Vec<String> = registry
                    .list()
                    .iter()
                    .map(|e| {
                        let desc = if e.metadata.description.is_empty() {
                            "(no description)".to_string()
                        } else {
                            e.metadata.description.trim().to_string()
                        };
                        format!("- {}: {}", e.metadata.name, desc)
                    })
                    .collect();
                return Ok(ToolCallContent {
                    text: lines.join("\n"),
                });
            }
            let content = registry
                .load_skill(name)
                .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;
            return Ok(ToolCallContent {
                text: format!(
                    "<skill_content name=\"{}\">\n{}\n</skill_content>",
                    name, content
                ),
            });
        }

        let skills_dir = self
            .skills_dir()
            .ok_or_else(|| ToolSourceError::InvalidInput("no working folder".to_string()))?;
        if !skills_dir.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "skills directory not found: {}",
                skills_dir.display()
            )));
        }

        const EXTENSIONS: &[&str] = &["md", "txt", "markdown"];
        for ext in EXTENSIONS {
            let p = skills_dir.join(format!("{}.{}", name, ext));
            if p.is_file() {
                let content = std::fs::read_to_string(&p)
                    .map_err(|e| ToolSourceError::Transport(format!("read skill: {}", e)))?;
                return Ok(ToolCallContent {
                    text: format!(
                        "<skill_content name=\"{}\">\n{}\n</skill_content>",
                        name, content
                    ),
                });
            }
        }
        let path = skills_dir.join(name);

        if !path.exists() {
            let mut available = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for e in entries.flatten() {
                    if let Some(stem) = e.path().file_stem() {
                        available.push(stem.to_string_lossy().to_string());
                    }
                }
            }
            return Err(ToolSourceError::InvalidInput(format!(
                "skill '{}' not found. Available: {}",
                name,
                available.join(", ")
            )));
        }
        if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "skill '{}' is a directory, not a file",
                name
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("read skill: {}", e)))?;

        Ok(ToolCallContent {
            text: format!(
                "<skill_content name=\"{}\">\n{}\n</skill_content>",
                name, content
            ),
        })
    }
}
