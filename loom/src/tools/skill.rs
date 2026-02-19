//! Skill tool: load a skill by name from a skills directory (e.g. .loom/skills).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for loading a skill.
pub const TOOL_SKILL: &str = "skill";

const SKILLS_SUBDIR: &str = ".loom/skills";

/// Tool that loads skill content by name from the working folder's skills directory.
pub struct SkillTool {
    working_folder: Arc<std::path::PathBuf>,
}

impl SkillTool {
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }

    fn skills_dir(&self) -> std::path::PathBuf {
        self.working_folder.join(SKILLS_SUBDIR)
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
                "Load a skill by name from the skills directory (.loom/skills). \
                 Use when a task matches a known skill; the tool returns the skill's instructions and content."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name (filename without extension, or path relative to skills dir)."
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

        let skills_dir = self.skills_dir();
        if !skills_dir.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "skills directory not found: {}",
                skills_dir.display()
            )));
        }

        // Allow "foo" or "subdir/foo"; try extensions .md, .txt, .markdown for both.
        const EXTENSIONS: &[&str] = &["md", "txt", "markdown"];
        for ext in EXTENSIONS {
            let p = skills_dir.join(format!("{}.{}", name, ext));
            if p.is_file() {
                let content = std::fs::read_to_string(&p).map_err(|e| {
                    ToolSourceError::Transport(format!("read skill: {}", e))
                })?;
                return Ok(ToolCallContent {
                    text: format!("<skill_content name=\"{}\">\n{}\n</skill_content>", name, content),
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
            text: format!("<skill_content name=\"{}\">\n{}\n</skill_content>", name, content),
        })
    }
}
