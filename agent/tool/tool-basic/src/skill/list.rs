//! Skill list tool — discovers and lists available skills with metadata.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use skill::SkillEntry;
use tool_core::Tool;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};

use super::SkillContext;

pub use loom_types::tools::tool_name::TOOL_SKILL_LIST;

pub struct SkillListTool {
    ctx: Arc<SkillContext>,
}

impl SkillListTool {
    pub(crate) fn new(ctx: Arc<SkillContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        TOOL_SKILL_LIST
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_SKILL_LIST.to_string(),
            description: Some(
                "List all available skills with names, descriptions, and categories. \
                 Use this to discover which skills match your task before loading one with skill_view."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter (e.g. \"coding\", \"debugging\")."
                    }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let category_filter = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(ref registry) = self.ctx.registry {
            let entries: Vec<&SkillEntry> = registry
                .list()
                .iter()
                .filter(|e| {
                    category_filter
                        .as_ref()
                        .is_none_or(|cf| e.metadata.category.as_deref() == Some(cf.as_str()))
                })
                .collect();

            let categories: Vec<String> = entries
                .iter()
                .filter_map(|e| e.metadata.category.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let mut categories_sorted = categories;
            categories_sorted.sort();

            let skills_json: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut obj = json!({
                        "name": e.metadata.name,
                        "description": e.metadata.description,
                    });
                    if let Some(ref cat) = e.metadata.category {
                        obj["category"] = json!(cat);
                    }
                    obj
                })
                .collect();

            let result = json!({
                "success": true,
                "skills": skills_json,
                "categories": categories_sorted,
                "count": entries.len(),
                "hint": "Use skill_view(name) to see full content"
            });

            return Ok(ToolCallContent::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            ));
        }

        let skills_dir = self
            .ctx
            .skills_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".loom/skills"));
        if !skills_dir.is_dir() {
            return Ok(ToolCallContent::text(
                json!({
                    "success": true,
                    "skills": [],
                    "categories": [],
                    "count": 0,
                    "hint": "No skills directory found"
                })
                .to_string(),
            ));
        }

        let scanned = skill::discovery::scan_skills_dir_recursive(
            &skills_dir,
            skill::SkillSource::Project,
        );

        let mut skills = Vec::new();
        let mut categories_set: std::collections::HashSet<String> = std::collections::HashSet::new();

        for entry in &scanned {
            if let Some(ref cf) = category_filter {
                if entry.metadata.category.as_deref() != Some(cf.as_str()) {
                    continue;
                }
            }
            let mut obj = json!({
                "name": entry.metadata.name,
                "description": entry.metadata.description,
            });
            if let Some(ref cat) = entry.metadata.category {
                obj["category"] = json!(cat);
                categories_set.insert(cat.clone());
            }
            skills.push(obj);
        }

        let result = json!({
            "success": true,
            "skills": skills,
            "categories": categories_set.into_iter().collect::<Vec<_>>(),
            "count": skills.len(),
            "hint": "Use skill_view(name) to see full content"
        });

        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
        ))
    }
}
