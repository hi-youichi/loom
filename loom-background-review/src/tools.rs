use super::curator::Curator;
use tool_experimental::MemoryTool;
use skill::validation::{validate_skill_create, validate_skill_path, Severity};
use skill::storage::{Lifecycle, SkillContent, SkillStorageRegistry as SkillRegistry, Source};
use skill::usage::SkillUsageStore;
use skill::provenance::{WriteOrigin, WriteOriginGuard};
use loom_llm::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const REVIEW_ALLOWED_TOOLS: &[&str] = &[
    "memory",
    "skills_list",
    "skill_view",
    "skill_create",
    "skill_edit",
    "skill_patch",
    "skill_delete",
    "skill_write_file",
    "skill_remove_file",
];

pub struct ReviewToolExecutor<'a> {
    pub memory_tool: &'a MemoryTool,
    pub skills: &'a SkillRegistry,
    pub curator: Option<&'a Curator>,
    pub skill_usage: Option<&'a SkillUsageStore>,
    pub actions: Vec<ReviewAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewAction {
    pub kind: String,
    pub target: String,
    pub summary: String,
    #[serde(default)]
    pub has_modification: bool,
}

impl<'a> ReviewToolExecutor<'a> {
    pub fn new(memory_tool: &'a MemoryTool, skills: &'a SkillRegistry) -> Self {
        Self {
            memory_tool,
            skills,
            curator: None,
            skill_usage: None,
            actions: Vec::new(),
        }
    }

    pub fn with_curator(mut self, curator: &'a Curator) -> Self {
        self.curator = Some(curator);
        self
    }

    pub fn with_skill_usage(mut self, skill_usage: &'a SkillUsageStore) -> Self {
        self.skill_usage = Some(skill_usage);
        self
    }

    pub fn execute(&mut self, tool_name: &str, args: &Value) -> Value {
        if !REVIEW_ALLOWED_TOOLS.contains(&tool_name) {
            return json!({
                "success": false,
                "error": format!(
                    "Background review denied non-whitelisted tool: {}. Only memory/skill tools are allowed.",
                    tool_name
                )
            });
        }
        let _guard = WriteOriginGuard::new(WriteOrigin::BackgroundReview);
        match tool_name {
            "memory" => {
                let result = self.memory_tool.dispatch(args);
                if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                    let action = args["action"].as_str().unwrap_or("");
                    let target = args["target"].as_str().unwrap_or("memory");
                    self.actions.push(ReviewAction {
                        kind: "memory".to_string(),
                        target: target.to_string(),
                        summary: format!("Memory {} {}", action, target),
                        has_modification: true,
                    });
                }
                result
            }
            "skills_list" => self.skills_list(args),
            "skill_view" => self.skill_view(args),
            "skill_create" => self.skill_create(args),
            "skill_edit" => self.skill_edit(args),
            "skill_patch" => self.skill_patch(args),
            "skill_delete" => self.skill_delete(args),
            "skill_write_file" => self.skill_write_file(args),
            "skill_remove_file" => self.skill_remove_file(args),
            _ => json!({"success": false, "error": format!("Unknown tool: {}", tool_name)}),
        }
    }

    fn skills_list(&self, _args: &Value) -> Value {
        match self.skills.list() {
            Ok(skills) => {
                let list: Vec<Value> = skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "description": s.description,
                            "lifecycle": format!("{:?}", s.lifecycle).to_lowercase(),
                            "source": format!("{:?}", s.source).to_lowercase(),
                            "triggers": s.triggers,
                        })
                    })
                    .collect();
                json!({"success": true, "skills": list})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_view(&self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        match self.skills.load(name) {
            Ok(skill) => {
                if let Some(curator) = self.curator {
                    let _ = curator.touch_skill(name);
                }
                if let Some(su) = self.skill_usage {
                    su.bump_view(name);
                }
                json!({
                    "success": true,
                    "skill": {
                        "name": skill.name,
                        "description": skill.description,
                        "triggers": skill.triggers,
                        "lifecycle": format!("{:?}", skill.lifecycle).to_lowercase(),
                        "source": format!("{:?}", skill.source).to_lowercase(),
                        "body": skill.body,
                    }
                })
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_create(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        let description = args["description"].as_str().unwrap_or("");
        let triggers: Vec<String> = args["triggers"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let body = args["body"].as_str().unwrap_or("");

        let skill = SkillContent {
            name: name.to_string(),
            description: description.to_string(),
            triggers,
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            body: body.to_string(),
            raw: String::new(),
        };

        let validation = validate_skill_create(&skill);
        if !validation.valid {
            let errors: Vec<String> = validation.warnings.iter()
                .filter(|w| w.severity == Severity::Critical)
                .map(|w| w.message.clone())
                .collect();
            return json!({"success": false, "error": format!("Validation failed: {}", errors.join("; "))});
        }

        match self.skills.save(name, &skill) {
            Ok(()) => {
                if let Some(su) = self.skill_usage {
                    su.mark_agent_created(name);
                }
                self.actions.push(ReviewAction {
                    kind: "skill".to_string(),
                    target: name.to_string(),
                    summary: format!("Skill '{}' created", name),
                    has_modification: true,
                });
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_edit(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");

        if content.trim().is_empty() {
            return json!({"success": false, "error": "content cannot be empty — refusing to clear skill body"});
        }

        match self.skills.load(name) {
            Ok(mut skill) => {
                let original_body = skill.body.clone();
                skill.body = content.to_string();

                let validation = validate_skill_create(&skill);
                if !validation.valid {
                    let errors: Vec<String> = validation.warnings.iter()
                        .filter(|w| w.severity == Severity::Critical)
                        .map(|w| w.message.clone())
                        .collect();
                    return json!({"success": false, "error": format!("Validation failed: {}", errors.join("; "))});
                }

                match self.skills.save(name, &skill) {
                    Ok(()) => {
                        if let Some(su) = self.skill_usage {
                            su.bump_patch(name);
                        }
                        self.actions.push(ReviewAction {
                            kind: "skill".to_string(),
                            target: name.to_string(),
                            summary: format!("Skill '{}' updated ({} -> {} chars)", name, original_body.len(), content.len()),
                            has_modification: true,
                        });
                        json!({"success": true})
                    }
                    Err(e) => json!({"success": false, "error": e.to_string()}),
                }
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_patch(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");

        match self.skills.patch(name, old_string, new_string) {
            Ok(()) => {
                if let Ok(skill) = self.skills.load(name) {
                    let validation = validate_skill_create(&skill);
                    if !validation.valid {
                        let _ = self.skills.patch(name, new_string, old_string);
                        let errors: Vec<String> = validation.warnings.iter()
                            .filter(|w| w.severity == Severity::Critical)
                            .map(|w| w.message.clone())
                            .collect();
                        return json!({"success": false, "error": format!("Validation failed, patch reverted: {}", errors.join("; "))});
                    }
                }

                self.actions.push(ReviewAction {
                    kind: "skill".to_string(),
                    target: name.to_string(),
                    summary: format!("Skill '{}' patched", name),
                    has_modification: true,
                });
                if let Some(su) = self.skill_usage {
                    su.bump_patch(name);
                }
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_delete(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        if let Some(su) = self.skill_usage {
            if !su.is_agent_created(name) {
                return json!({
                    "success": false,
                    "error": format!(
                        "Refusing to delete '{}': not agent-created. Only skills autonomously created by background review can be deleted.",
                        name
                    )
                });
            }
        }
        match self.skills.delete(name) {
            Ok(()) => {
                self.actions.push(ReviewAction {
                    kind: "skill".to_string(),
                    target: name.to_string(),
                    summary: format!("Skill '{}' removed", name),
                    has_modification: true,
                });
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_write_file(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");

        let path_validation = validate_skill_path(path);
        if !path_validation.valid {
            let errors: Vec<String> = path_validation.warnings.iter()
                .filter(|w| w.severity == Severity::Critical)
                .map(|w| w.message.clone())
                .collect();
            return json!({"success": false, "error": format!("Path validation: {}", errors.join("; "))});
        }

        match self.skills.write_file(name, path, content) {
            Ok(()) => {
                self.actions.push(ReviewAction {
                    kind: "skill_file".to_string(),
                    target: name.to_string(),
                    summary: format!("File added to skill '{}': {}", name, path),
                    has_modification: true,
                });
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_remove_file(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or("");

        match self.skills.remove_file(name, path) {
            Ok(()) => {
                self.actions.push(ReviewAction {
                    kind: "skill_file".to_string(),
                    target: name.to_string(),
                    summary: format!("File removed from skill '{}': {}", name, path),
                    has_modification: true,
                });
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}

pub fn review_tool_specs() -> Vec<ToolSpec> {
    vec![
        MemoryTool::tool_spec(),
        ToolSpec {
            name: "skills_list".into(),
            description: Some("List all skills in the skill library.".into()),
            input_schema: json!({"type": "object", "properties": {}}),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_view".into(),
            description: Some("View details of a specific skill including its body.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"}
                },
                "required": ["name"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_create".into(),
            description: Some("Create a new skill.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name (class-level, kebab-case)"},
                    "description": {"type": "string", "description": "Short description"},
                    "triggers": {"type": "array", "items": {"type": "string"}, "description": "Trigger keywords"},
                    "body": {"type": "string", "description": "Skill body (markdown)"}
                },
                "required": ["name", "description", "body"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_edit".into(),
            description: Some("Replace the entire body of a skill.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"},
                    "content": {"type": "string", "description": "New full body content"}
                },
                "required": ["name", "content"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_patch".into(),
            description: Some("Apply a precise find-and-replace patch to a skill's SKILL.md.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"},
                    "old_string": {"type": "string", "description": "Exact text to find"},
                    "new_string": {"type": "string", "description": "Replacement text"}
                },
                "required": ["name", "old_string", "new_string"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_delete".into(),
            description: Some("Delete a skill entirely.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"}
                },
                "required": ["name"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_write_file".into(),
            description: Some("Add or overwrite a support file under a skill (e.g. references/, templates/, scripts/).".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"},
                    "path": {"type": "string", "description": "Relative path within skill dir (e.g. references/topic.md)"},
                    "content": {"type": "string", "description": "File content"}
                },
                "required": ["name", "path", "content"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "skill_remove_file".into(),
            description: Some("Remove a support file from a skill.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name"},
                    "path": {"type": "string", "description": "Relative path within skill dir"}
                },
                "required": ["name", "path"]
            }),
            output_hint: None,
        },
    ]
}
