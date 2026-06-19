use async_trait::async_trait;
use memory_v2::{
    AddResult, MemoryError, MemoryFile, MemoryStore, RemoveResult, ReplaceResult,
};
use loom_llm::message::ToolCallContent;
use loom_llm::tool::{ToolSourceError, ToolSpec};
use serde_json::{json, Value};
use std::sync::Arc;
use tool_core::{Tool, ToolCallContext};

pub const TOOL_MEMORY: &str = "memory";

const MEMORY_DESCRIPTION: &str = "\
Save durable information to persistent memory that survives across sessions. \
Memory is injected into future turns, so keep it compact and focused on facts \
that will still matter later.\n\
\n\
WHEN TO SAVE (do this proactively, don't wait to be asked):\n\
- User corrects you or says 'remember this' / 'don't do that again'\n\
- User shares a preference, habit, or personal detail (name, role, timezone, coding style)\n\
- You discover something about the environment (OS, installed tools, project structure)\n\
- You learn a convention, API quirk, or workflow specific to this user's setup\n\
- You identify a stable fact that will be useful again in future sessions\n\
\n\
PRIORITY: User preferences and corrections > environment facts > procedural knowledge. \
The most valuable memory prevents the user from having to repeat themselves.\n\
\n\
Do NOT save task progress, session outcomes, completed-work logs, or temporary TODO \
state to memory.\n\
If you've discovered a new way to do something, solved a problem that could be \
necessary later, save it as a skill with the skill tool.\n\
\n\
TWO TARGETS:\n\
- 'user': who the user is -- name, role, preferences, communication style, pet peeves\n\
- 'memory': your notes -- environment facts, project conventions, tool quirks, lessons learned\n\
\n\
ACTIONS: add (new entry), replace (update existing -- old_text identifies it), \
remove (delete -- old_text identifies it).\n\
\n\
SKIP: trivial/obvious info, things easily re-discovered, raw data dumps, and temporary task state.";

pub struct MemoryTool {
    store: Arc<MemoryStore>,
}

impl MemoryTool {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<MemoryStore> {
        &self.store
    }

    fn parse_target(target: &str) -> Result<MemoryFile, Value> {
        match target.to_lowercase().as_str() {
            "user" => Ok(MemoryFile::User),
            "memory" => Ok(MemoryFile::Project),
            _ => Err(json!({
                "success": false,
                "error": format!("Unknown target '{}'. Use 'memory' or 'user'.", target)
            })),
        }
    }

    fn handle_add(&self, file: MemoryFile, content: &str) -> Value {
        match self.store.add_entry(file, content) {
            Ok(AddResult {
                success,
                message,
                entry_count,
                usage,
            }) => json!({
                "success": success,
                "message": message,
                "entries": entry_count,
                "usage": usage
            }),
            Err(e) => Self::fmt_error(&e),
        }
    }

    fn handle_replace(&self, file: MemoryFile, old_text: &str, new_content: &str) -> Value {
        match self.store.replace_entry(file, old_text, new_content) {
            Ok(ReplaceResult {
                success,
                message,
                matched_count,
                entry_count,
                usage,
            }) => json!({
                "success": success,
                "message": message,
                "matched": matched_count,
                "entries": entry_count,
                "usage": usage
            }),
            Err(e) => Self::fmt_error(&e),
        }
    }

    fn handle_remove(&self, file: MemoryFile, old_text: &str) -> Value {
        match self.store.remove_entry(file, old_text) {
            Ok(RemoveResult {
                success,
                message,
                entry_count,
                usage,
            }) => json!({
                "success": success,
                "message": message,
                "entries": entry_count,
                "usage": usage
            }),
            Err(e) => Self::fmt_error(&e),
        }
    }

    fn fmt_error(e: &MemoryError) -> Value {
        json!({
            "success": false,
            "error": e.to_string()
        })
    }

    pub fn tool_spec() -> ToolSpec {
        ToolSpec {
            name: TOOL_MEMORY.to_string(),
            description: Some(MEMORY_DESCRIPTION.to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "replace", "remove"],
                        "description": "The action to perform."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["memory", "user"],
                        "description": "Which memory store: 'memory' for personal notes, 'user' for user profile."
                    },
                    "content": {
                        "type": "string",
                        "description": "The entry content. Required for 'add' and 'replace'."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Short unique substring identifying the entry to replace or remove."
                    }
                },
                "required": ["action", "target"]
            }),
            output_hint: None,
        }
    }

    pub fn dispatch(&self, args: &Value) -> Value {
        let action = args["action"].as_str().unwrap_or("");
        let target = args["target"].as_str().unwrap_or("memory");

        let file = match Self::parse_target(target) {
            Ok(f) => f,
            Err(err_resp) => return err_resp,
        };

        match action {
            "add" => {
                let content = args["content"].as_str().unwrap_or("");
                self.handle_add(file, content)
            }
            "replace" => {
                let old_text = args["old_text"].as_str().unwrap_or("");
                let new_content = args["content"].as_str().unwrap_or("");
                self.handle_replace(file, old_text, new_content)
            }
            "remove" => {
                let old_text = args["old_text"].as_str().unwrap_or("");
                self.handle_remove(file, old_text)
            }
            _ => json!({
                "success": false,
                "error": format!("Unknown action '{}'. Use add/replace/remove.", action)
            }),
        }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        TOOL_MEMORY
    }

    fn spec(&self) -> ToolSpec {
        Self::tool_spec()
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let result = self.dispatch(&args);
        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_v2::MemoryStore;

    fn test_store() -> Arc<MemoryStore> {
        Arc::new(MemoryStore::new(
            &tempfile::tempdir().unwrap().keep(),
        ))
    }

    #[test]
    fn dispatch_add() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        let result = tool.dispatch(&json!({
            "action": "add",
            "target": "user",
            "content": "prefers Rust"
        }));
        assert!(result["success"].as_bool().unwrap());
    }

    #[test]
    fn dispatch_replace() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        tool.dispatch(&json!({"action": "add", "target": "user", "content": "likes tea"}));
        let result = tool.dispatch(&json!({
            "action": "replace",
            "target": "user",
            "old_text": "tea",
            "content": "likes coffee"
        }));
        assert!(result["success"].as_bool().unwrap());
    }

    #[test]
    fn dispatch_remove() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        tool.dispatch(&json!({"action": "add", "target": "user", "content": "temp entry"}));
        let result = tool.dispatch(&json!({
            "action": "remove",
            "target": "user",
            "old_text": "temp"
        }));
        assert!(result["success"].as_bool().unwrap());
    }

    #[test]
    fn dispatch_unknown_action() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        let result = tool.dispatch(&json!({"action": "read", "target": "user"}));
        assert!(!result["success"].as_bool().unwrap());
        assert!(result["error"].as_str().unwrap().contains("Unknown action"));
    }

    #[test]
    fn dispatch_unknown_target() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        let result = tool.dispatch(&json!({"action": "add", "target": "invalid", "content": "x"}));
        assert!(!result["success"].as_bool().unwrap());
    }

    #[test]
    fn spec_matches_reference() {
        let spec = MemoryTool::tool_spec();
        assert_eq!(spec.name, "memory");
        let schema = &spec.input_schema;
        let action_enum = schema["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(action_enum.len(), 3);
        assert!(action_enum.iter().any(|v| v == "add"));
        assert!(action_enum.iter().any(|v| v == "replace"));
        assert!(action_enum.iter().any(|v| v == "remove"));
        let target_enum = schema["properties"]["target"]["enum"].as_array().unwrap();
        assert!(target_enum.iter().any(|v| v == "memory"));
        assert!(target_enum.iter().any(|v| v == "user"));
    }

    #[tokio::test]
    async fn tool_trait_call() {
        let store = test_store();
        let tool = MemoryTool::new(store);
        let result = tool
            .call(json!({"action": "add", "target": "memory", "content": "test"}), None)
            .await
            .unwrap();
        let text = match result {
            ToolCallContent::Text(t) => t,
            _ => panic!("expected text"),
        };
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert!(parsed["success"].as_bool().unwrap());
    }

    #[test]
    fn target_memory_maps_to_project() {
        assert_eq!(MemoryTool::parse_target("memory").unwrap(), MemoryFile::Project);
        assert_eq!(MemoryTool::parse_target("user").unwrap(), MemoryFile::User);
    }
}
