//! Unit tests for FileToolSource and path validation.
//!
//! Scenarios: list_tools returns 9 tools (file tools + todo_write, todo_read); ls under working folder;
//! read/write_file roundtrip; path outside working folder returns InvalidInput;
//! create_dir and delete_file; move_file; glob (pattern/path/include).

mod init_logging;

use graphweave::tool_source::{FileToolSource, ToolSource, ToolSourceError};
use graphweave::tools::{
    TOOL_CREATE_DIR, TOOL_DELETE_FILE, TOOL_GLOB, TOOL_GREP, TOOL_LS, TOOL_MOVE_FILE,
    TOOL_READ_FILE, TOOL_TODO_READ, TOOL_TODO_WRITE, TOOL_WRITE_FILE,
};
use serde_json::json;

/// Scenario: FileToolSource::new with a valid directory returns a source that lists 10 tools (file + grep + todo_write, todo_read).
#[tokio::test]
async fn file_tool_source_list_tools_returns_ten_tools() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 10);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&TOOL_LS));
    assert!(names.contains(&TOOL_READ_FILE));
    assert!(names.contains(&TOOL_WRITE_FILE));
    assert!(names.contains(&TOOL_MOVE_FILE));
    assert!(names.contains(&TOOL_DELETE_FILE));
    assert!(names.contains(&TOOL_CREATE_DIR));
    assert!(names.contains(&TOOL_GLOB));
    assert!(names.contains(&TOOL_GREP));
    assert!(names.contains(&TOOL_TODO_WRITE));
    assert!(names.contains(&TOOL_TODO_READ));
}

/// Scenario: ls with path "." returns entries in the working folder.
#[tokio::test]
async fn file_tool_source_ls_root() {
    let dir = tempfile::tempdir().unwrap();
    let _ = std::fs::File::create(dir.path().join("a.txt")).unwrap();
    let _ = std::fs::File::create(dir.path().join("b.txt")).unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::File::create(dir.path().join("sub").join("c.txt")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_LS, json!({ "path": "." }))
        .await
        .unwrap();
    assert!(result.text.contains("a.txt"));
    assert!(result.text.contains("b.txt"));
    assert!(result.text.contains("sub"));
    assert!(result.text.contains("c.txt"));
}

/// Scenario: write_file then read returns the same content; path is under working folder.
#[tokio::test]
async fn file_tool_source_write_file_then_read_file_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    source
        .call_tool(
            TOOL_WRITE_FILE,
            json!({ "path": "f.txt", "content": "hello world" }),
        )
        .await
        .unwrap();
    let out = source
        .call_tool(TOOL_READ_FILE, json!({ "path": "f.txt" }))
        .await
        .unwrap();
    assert_eq!(out.text, "hello world");
}

/// Scenario: path parameter "../outside" is rejected with InvalidInput (path outside working folder).
#[tokio::test]
async fn file_tool_source_path_outside_working_folder_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_READ_FILE, json!({ "path": "../outside" }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("outside"));
}

/// Scenario: create_dir creates a subdirectory; delete_file with path removes it.
#[tokio::test]
async fn file_tool_source_create_dir_and_delete_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    source
        .call_tool(TOOL_CREATE_DIR, json!({ "path": "subdir" }))
        .await
        .unwrap();
    assert!(dir.path().join("subdir").is_dir());
    source
        .call_tool(TOOL_DELETE_FILE, json!({ "path": "subdir" }))
        .await
        .unwrap();
    assert!(!dir.path().join("subdir").exists());
}

/// Scenario: move_file moves a file from source to target under working folder.
#[tokio::test]
async fn file_tool_source_move_file() {
    let dir = tempfile::tempdir().unwrap();
    let _ = std::fs::File::create(dir.path().join("old.txt")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    source
        .call_tool(
            TOOL_MOVE_FILE,
            json!({ "source": "old.txt", "target": "new.txt" }),
        )
        .await
        .unwrap();
    assert!(!dir.path().join("old.txt").exists());
    assert!(dir.path().join("new.txt").exists());
}

/// Scenario: FileToolSource::new with a non-existent path returns InvalidInput.
#[test]
fn file_tool_source_new_nonexistent_path_returns_error() {
    let result = FileToolSource::new("/nonexistent/path/12345");
    assert!(result.is_err());
    if let Err(ToolSourceError::InvalidInput(_)) = result {
    } else {
        panic!("expected InvalidInput");
    }
}

// --- glob tool (BDD-style) ---

/// Scenario: glob with pattern only lists files matching the pattern under working folder root.
#[tokio::test]
async fn glob_pattern_only_returns_matching_files() {
    let dir = tempfile::tempdir().unwrap();
    let _ = std::fs::File::create(dir.path().join("a.rs")).unwrap();
    let _ = std::fs::File::create(dir.path().join("b.rs")).unwrap();
    let _ = std::fs::File::create(dir.path().join("c.txt")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_GLOB, json!({ "pattern": "*.rs" }))
        .await
        .unwrap();
    let lines: Vec<&str> = result.text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.contains(&"a.rs"));
    assert!(lines.contains(&"b.rs"));
    assert!(!result.text.contains("c.txt"));
}

/// Scenario: glob with path restricts search to that subdirectory.
#[tokio::test]
async fn glob_with_path_searches_under_subdir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let _ = std::fs::File::create(dir.path().join("src").join("lib.rs")).unwrap();
    let _ = std::fs::File::create(dir.path().join("root.rs")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_GLOB, json!({ "pattern": "*.rs", "path": "src" }))
        .await
        .unwrap();
    assert_eq!(result.text.trim(), "src/lib.rs");
    assert!(!result.text.contains("root.rs"));
}

/// Scenario: glob with include filters results by additional patterns.
#[tokio::test]
async fn glob_with_include_filters_by_include_patterns() {
    let dir = tempfile::tempdir().unwrap();
    let _ = std::fs::File::create(dir.path().join("a.rs")).unwrap();
    let _ = std::fs::File::create(dir.path().join("b.toml")).unwrap();
    let _ = std::fs::File::create(dir.path().join("c.yaml")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(
            TOOL_GLOB,
            json!({ "pattern": "**/*", "include": ["*.rs", "*.toml"] }),
        )
        .await
        .unwrap();
    let lines: Vec<&str> = result.text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.contains(&"a.rs"));
    assert!(lines.contains(&"b.toml"));
    assert!(!result.text.contains("c.yaml"));
}

/// Scenario: glob with path ".." is rejected with InvalidInput (path outside working folder).
#[tokio::test]
async fn glob_path_escape_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_GLOB, json!({ "pattern": "*.rs", "path": ".." }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

/// Scenario: glob with pattern containing ".." is rejected with InvalidInput.
#[tokio::test]
async fn glob_pattern_with_dot_dot_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_GLOB, json!({ "pattern": "../*.rs" }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().contains(".."));
}

/// Scenario: glob when no file matches returns empty list.
#[tokio::test]
async fn glob_no_match_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let _ = std::fs::File::create(dir.path().join("a.txt")).unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source
        .call_tool(TOOL_GLOB, json!({ "pattern": "*.rs" }))
        .await
        .unwrap();
    assert!(result.text.trim().is_empty());
}
