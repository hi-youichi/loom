//! Shared test helpers for skill_manager tests.

use skill::storage::{Lifecycle, SkillContent, SkillStorageRegistry, Source};
use tempfile::TempDir;

// Re-export parent module items for submodules.
pub(crate) use super::*;

pub fn make_storage() -> (TempDir, Arc<SkillStorageRegistry>) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(SkillStorageRegistry::new(dir.path()));
    (dir, storage)
}

pub fn make_tool(storage: Arc<SkillStorageRegistry>) -> SkillManagerTool {
    SkillManagerTool::for_background_review(storage, None)
}

pub fn make_skill_md(name: &str, description: &str, body: &str) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n{}",
        name, description, body
    )
}

/// Save a skill directly into storage (bypassing the tool).
/// Reduces the `SkillContent { ... }` boilerplate repeated in ~15 tests.
pub fn save_skill(storage: &SkillStorageRegistry, name: &str, description: &str, body: &str) {
    let (_, _, _, parsed_body) = validate_frontmatter(&make_skill_md(name, description, body)).unwrap();
    storage
        .save(
            name,
            &SkillContent {
                name: name.into(),
                description: description.into(),
                triggers: vec![],
                lifecycle: Lifecycle::Active,
                source: Source::Auto,
                created_by: None,
                body: parsed_body,
                raw: make_skill_md(name, description, body),
            },
        )
        .unwrap();
}

pub fn json_response(result: ToolCallContent) -> Value {
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON '{}': {}", text, e))
}

mod create;
mod coverage;
mod delete;
mod edit_patch;
mod files;
mod frontmatter;
mod spec;
