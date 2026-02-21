//! Integration tests for SkillTool: load by name, extensions, subdir, missing dir/name.

mod init_logging;

use loom::tool_source::register_file_tools;
use loom::tool_source::{ToolSource, ToolSourceError};
use loom::tools::{AggregateToolSource, TOOL_SKILL};
use serde_json::json;

fn aggregate_with_file_tools(dir: &tempfile::TempDir) -> AggregateToolSource {
    let agg = AggregateToolSource::new();
    register_file_tools(&agg, dir.path()).unwrap();
    agg
}

#[tokio::test]
async fn skill_load_by_name_with_md_extension() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".loom").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("foo.md"), "# Foo skill\nDo something.").unwrap();

    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_SKILL, json!({ "name": "foo" }))
        .await
        .unwrap();
    assert!(result.text.contains("<skill_content"));
    assert!(result.text.contains("name=\"foo\""));
    assert!(result.text.contains("# Foo skill"));
    assert!(result.text.contains("Do something."));
}

#[tokio::test]
async fn skill_load_by_name_with_txt_extension() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".loom").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("bar.txt"), "Bar instructions").unwrap();

    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_SKILL, json!({ "name": "bar" }))
        .await
        .unwrap();
    assert!(result.text.contains("Bar instructions"));
}

#[tokio::test]
async fn skill_subdir_with_extension() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".loom").join("skills");
    std::fs::create_dir_all(skills_dir.join("nested")).unwrap();
    std::fs::write(
        skills_dir.join("nested").join("deep.md"),
        "Nested skill content",
    )
    .unwrap();

    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_SKILL, json!({ "name": "nested/deep" }))
        .await
        .unwrap();
    assert!(result.text.contains("Nested skill content"));
}

#[tokio::test]
async fn skill_missing_name_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".loom").join("skills")).unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg.call_tool(TOOL_SKILL, json!({})).await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn skill_no_skills_dir_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg.call_tool(TOOL_SKILL, json!({ "name": "any" })).await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("not found"));
}

#[tokio::test]
async fn skill_unknown_name_returns_error_with_available_list() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".loom").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("known.md"), "content").unwrap();

    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_SKILL, json!({ "name": "unknown_skill" }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().contains("unknown_skill"));
    assert!(err.to_string().contains("known"));
}
