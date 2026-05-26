use super::curator::Curator;
use super::memory::{MemoryFile, MemoryStore};
use super::security::{validate_skill_create, validate_skill_path, Severity};
use super::skill_registry::{Lifecycle, SkillContent, SkillRegistry, Source};
use crate::tool_source::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_MEMORY_FILE_SIZE: usize = 64 * 1024;
const REPLACE_SHRINK_RATIO: f64 = 0.3;

pub struct ReviewToolExecutor<'a> {
    pub memory: &'a MemoryStore,
    pub skills: &'a SkillRegistry,
    pub curator: Option<&'a Curator>,
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
    pub fn new(memory: &'a MemoryStore, skills: &'a SkillRegistry) -> Self {
        Self {
            memory,
            skills,
            curator: None,
            actions: Vec::new(),
        }
    }

    pub fn with_curator(mut self, curator: &'a Curator) -> Self {
        self.curator = Some(curator);
        self
    }

    pub fn execute(&mut self, tool_name: &str, args: &Value) -> Value {
        match tool_name {
            "memory_get" => self.memory_get(args),
            "memory_set" => self.memory_set(args),
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

    /// Parse a memory file identifier string into a [`MemoryFile`].
    ///
    /// Accepts case-insensitive names with or without `.md` extension.
    fn parse_memory_file(file_str: &str) -> Result<MemoryFile, Value> {
        match file_str.to_lowercase().as_str() {
            "user" | "user.md" => Ok(MemoryFile::User),
            "project" | "project.md" => Ok(MemoryFile::Project),
            "facts" | "facts.md" => Ok(MemoryFile::Facts),
            _ => Err(json!({"success": false, "error": format!("Unknown memory file: {}", file_str)})),
        }
    }

    fn memory_get(&self, args: &Value) -> Value {
        let file_str = args["file"].as_str().unwrap_or("user");
        let file = match Self::parse_memory_file(file_str) {
            Ok(f) => f,
            Err(e) => return e,
        };
        match self.memory.load(file) {
            Ok(content) => json!({"success": true, "content": content}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn memory_set(&mut self, args: &Value) -> Value {
        let file_str = args["file"].as_str().unwrap_or("user");
        let action = args["action"].as_str().unwrap_or("append");
        let content = args["content"].as_str().unwrap_or("");

        let file = match Self::parse_memory_file(file_str) {
            Ok(f) => f,
            Err(e) => return e,
        };

        if action == "append" {
            if let Ok(existing) = self.memory.load(file) {
                if existing.contains(content) {
                    return json!({"success": true, "warning": "Content already exists in memory, skipping duplicate"});
                }
                if existing.len() + content.len() > MAX_MEMORY_FILE_SIZE {
                    return json!({"success": false, "error": format!(
                        "Memory file would exceed {} bytes (current: {}, new: {})",
                        MAX_MEMORY_FILE_SIZE, existing.len(), content.len()
                    )});
                }
            }
        }

        if action == "replace" {
            if let Ok(existing) = self.memory.load(file) {
                if !existing.is_empty() && !content.is_empty() {
                    let ratio = content.len() as f64 / existing.len() as f64;
                    if ratio < REPLACE_SHRINK_RATIO {
                        return json!({"success": false, "error": format!(
                            "Replace would shrink file too much ({} -> {} chars, ratio {:.0}%). Use skill_patch for targeted edits.",
                            existing.len(), content.len(), ratio * 100.0
                        )});
                    }
                }
                if content.len() > MAX_MEMORY_FILE_SIZE {
                    return json!({"success": false, "error": format!(
                        "New content exceeds {} bytes", MAX_MEMORY_FILE_SIZE
                    )});
                }
            }
        }

        let result = match action {
            "append" => self.memory.append(file, content),
            "replace" => self.memory.replace(file, content),
            _ => return json!({"success": false, "error": format!("Unknown action: {}", action)}),
        };

        match result {
            Ok(()) => {
                self.actions.push(ReviewAction {
                    kind: "memory".to_string(),
                    target: file_str.to_string(),
                    summary: format!("Memory {} {}", action, file_str),
                    has_modification: true,
                });
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
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
                json!({"success": true})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }

    fn skill_delete(&mut self, args: &Value) -> Value {
        let name = args["name"].as_str().unwrap_or("");
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
        ToolSpec {
            name: "memory_get".into(),
            description: Some("Read a memory file (USER, PROJECT, or FACTS).".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": {"type": "string", "enum": ["USER", "PROJECT", "FACTS"], "description": "Memory file to read"}
                },
                "required": ["file"]
            }),
            output_hint: None,
        },
        ToolSpec {
            name: "memory_set".into(),
            description: Some("Write to a memory file. Action can be 'append' or 'replace'.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": {"type": "string", "enum": ["USER", "PROJECT", "FACTS"], "description": "Memory file to write"},
                    "action": {"type": "string", "enum": ["append", "replace"], "description": "'append' to add content, 'replace' to overwrite"},
                    "content": {"type": "string", "description": "Content to write"}
                },
                "required": ["file", "action", "content"]
            }),
            output_hint: None,
        },
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
