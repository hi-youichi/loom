//! Unit tests for GrepTool: pure-Rust regex content search under working folder.
//!
//! Scenarios: basic match; no match; missing/empty/invalid pattern; path escaping;
//! include glob filter; brace expansion; subdirectory scoping; binary file skipping;
//! multiple matches in one file; case sensitivity; mod-time sort order.

mod init_logging;

use loom::tool_source::{FileToolSource, ToolSource, ToolSourceError};
use loom::tools::TOOL_GREP;
use serde_json::json;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn grep(dir: &tempfile::TempDir, args: serde_json::Value) -> loom::tool_source::ToolCallContent {
    FileToolSource::new(dir.path())
        .unwrap()
        .call_tool(TOOL_GREP, args)
        .await
        .unwrap()
}

async fn grep_err(dir: &tempfile::TempDir, args: serde_json::Value) -> ToolSourceError {
    FileToolSource::new(dir.path())
        .unwrap()
        .call_tool(TOOL_GREP, args)
        .await
        .unwrap_err()
}

// ---------------------------------------------------------------------------
// basic matching
// ---------------------------------------------------------------------------

/// Scenario: pattern matches a line; output contains path, line number, and content.
#[tokio::test]
async fn grep_basic_match_returns_path_and_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello world\nfoo bar\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hello" })).await;

    assert!(result.text.contains("a.txt"), "expected filename in output");
    assert!(result.text.contains("Line 1"), "expected line number");
    assert!(result.text.contains("hello world"), "expected matched line");
    assert!(!result.text.contains("foo bar"), "non-matching line must not appear");
}

/// Scenario: pattern matches multiple lines in a single file.
#[tokio::test]
async fn grep_multiple_lines_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.rs"), "fn foo() {}\nfn bar() {}\nfn baz() {}\n").unwrap();

    let result = grep(&dir, json!({ "pattern": r"fn \w+" })).await;

    assert!(result.text.contains("Line 1"));
    assert!(result.text.contains("Line 2"));
    assert!(result.text.contains("Line 3"));
    assert!(result.text.contains("Found 3 matches"));
}

/// Scenario: pattern matches lines across multiple files; both appear in output.
#[tokio::test]
async fn grep_matches_in_multiple_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.txt"), "needle here\n").unwrap();
    std::fs::write(dir.path().join("y.txt"), "needle there\n").unwrap();
    std::fs::write(dir.path().join("z.txt"), "nothing\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "needle" })).await;

    assert!(result.text.contains("x.txt"));
    assert!(result.text.contains("y.txt"));
    assert!(!result.text.contains("z.txt"));
    assert!(result.text.contains("Found 2 matches"));
}

// ---------------------------------------------------------------------------
// no match / empty results
// ---------------------------------------------------------------------------

/// Scenario: pattern does not match any file; returns "No files found".
#[tokio::test]
async fn grep_no_match_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "xyzzy" })).await;

    assert_eq!(result.text, "No files found");
}

/// Scenario: working folder is empty; returns "No files found".
#[tokio::test]
async fn grep_empty_folder_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();

    let result = grep(&dir, json!({ "pattern": "anything" })).await;

    assert_eq!(result.text, "No files found");
}

// ---------------------------------------------------------------------------
// error cases: pattern validation
// ---------------------------------------------------------------------------

/// Scenario: missing pattern key returns InvalidInput.
#[tokio::test]
async fn grep_missing_pattern_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({})).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

/// Scenario: empty string pattern returns InvalidInput.
#[tokio::test]
async fn grep_empty_pattern_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

/// Scenario: syntactically invalid regex returns InvalidInput with "invalid regex" message.
#[tokio::test]
async fn grep_invalid_regex_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "(unclosed" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(
        err.to_string().to_lowercase().contains("invalid regex"),
        "error message should mention 'invalid regex', got: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// error cases: path validation
// ---------------------------------------------------------------------------

/// Scenario: path parameter escapes working folder; returns InvalidInput.
#[tokio::test]
async fn grep_path_outside_working_folder_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "x", "path": "../.." })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("outside"));
}

/// Scenario: path points to a file (not a directory); returns InvalidInput.
#[tokio::test]
async fn grep_path_is_file_not_directory_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plain.txt"), "content\n").unwrap();

    let err = grep_err(&dir, json!({ "pattern": "x", "path": "plain.txt" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

// ---------------------------------------------------------------------------
// include filter
// ---------------------------------------------------------------------------

/// Scenario: include="*.rs" restricts matches to .rs files only.
#[tokio::test]
async fn grep_include_filter_restricts_to_matching_extension() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "fn search() {}\n").unwrap();
    std::fs::write(dir.path().join("config.toml"), "search = true\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "search", "include": "*.rs" })).await;

    assert!(result.text.contains("lib.rs"), "lib.rs should match");
    assert!(!result.text.contains("config.toml"), "config.toml must be excluded");
}

/// Scenario: include with brace expansion "*.{rs,toml}" matches both extensions.
#[tokio::test]
async fn grep_include_brace_expansion_matches_multiple_extensions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "token\n").unwrap();
    std::fs::write(dir.path().join("config.toml"), "token\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "token\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "token", "include": "*.{rs,toml}" })).await;

    assert!(result.text.contains("lib.rs"));
    assert!(result.text.contains("config.toml"));
    assert!(!result.text.contains("notes.txt"), "txt must be excluded");
}

/// Scenario: include pattern that matches nothing returns "No files found".
#[tokio::test]
async fn grep_include_no_match_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hello", "include": "*.rs" })).await;

    assert_eq!(result.text, "No files found");
}

/// Scenario: invalid glob in include returns InvalidInput.
#[tokio::test]
async fn grep_include_invalid_glob_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "x", "include": "[invalid" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

// ---------------------------------------------------------------------------
// path scoping
// ---------------------------------------------------------------------------

/// Scenario: path restricts search to subdirectory; files outside are not matched.
#[tokio::test]
async fn grep_path_restricts_to_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("lib.rs"), "find_me\n").unwrap();
    std::fs::write(dir.path().join("root.rs"), "find_me\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "find_me", "path": "src" })).await;

    assert!(result.text.contains("lib.rs"), "src/lib.rs should be found");
    assert!(!result.text.contains("root.rs"), "root.rs is outside path, must be excluded");
}

/// Scenario: path defaults to "." and searches all files recursively.
#[tokio::test]
async fn grep_default_path_searches_recursively() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("deep")).unwrap();
    std::fs::create_dir(dir.path().join("deep").join("nested")).unwrap();
    std::fs::write(
        dir.path().join("deep").join("nested").join("file.txt"),
        "buried_token\n",
    )
    .unwrap();

    let result = grep(&dir, json!({ "pattern": "buried_token" })).await;

    assert!(result.text.contains("buried_token"), "nested file should be found");
    assert!(result.text.contains("file.txt"));
}

// ---------------------------------------------------------------------------
// binary file handling
// ---------------------------------------------------------------------------

/// Scenario: binary files (containing null bytes) are silently skipped.
#[tokio::test]
async fn grep_binary_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    // Binary file with null byte and the target pattern
    let binary: Vec<u8> = b"binary_match\x00garbage\n".to_vec();
    std::fs::write(dir.path().join("data.bin"), &binary).unwrap();
    // A plain text file with the same pattern
    std::fs::write(dir.path().join("text.txt"), "binary_match\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "binary_match" })).await;

    assert!(result.text.contains("text.txt"), "text file must appear");
    assert!(!result.text.contains("data.bin"), "binary file must be skipped");
}

// ---------------------------------------------------------------------------
// regex features
// ---------------------------------------------------------------------------

/// Scenario: grep is case-sensitive by default; uppercase pattern does not match lowercase content.
#[tokio::test]
async fn grep_is_case_sensitive_by_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "Hello World\n").unwrap();

    let lower_result = grep(&dir, json!({ "pattern": "hello world" })).await;
    assert_eq!(lower_result.text, "No files found", "lowercase should not match uppercase file");

    let upper_result = grep(&dir, json!({ "pattern": "Hello World" })).await;
    assert!(upper_result.text.contains("f.txt"), "exact-case should match");
}

/// Scenario: case-insensitive matching via inline flag (?i).
#[tokio::test]
async fn grep_inline_case_insensitive_flag_works() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "Hello World\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "(?i)hello world" })).await;

    assert!(result.text.contains("f.txt"));
    assert!(result.text.contains("Hello World"));
}

/// Scenario: anchored regex (^word$) matches full-line content exactly.
#[tokio::test]
async fn grep_anchored_pattern_matches_exact_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "exact\nexact match\nnot exact\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "^exact$" })).await;

    assert!(result.text.contains("Line 1"), "line 1 'exact' should match");
    assert!(!result.text.contains("Line 2"), "line 2 'exact match' must not match ^exact$");
    assert!(!result.text.contains("Line 3"));
}

// ---------------------------------------------------------------------------
// output format
// ---------------------------------------------------------------------------

/// Scenario: output header shows "Found N matches" with correct count.
#[tokio::test]
async fn grep_output_header_shows_match_count() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hit\nhit\nnope\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hit" })).await;

    assert!(result.text.starts_with("Found 2 matches"), "output: {}", result.text);
}

/// Scenario: results truncated at 100 matches; truncation notice appended.
#[tokio::test]
async fn grep_results_truncated_at_100_matches() {
    let dir = tempfile::tempdir().unwrap();
    // 110 matching lines
    let content: String = (0..110).map(|i| format!("match_line_{}\n", i)).collect();
    std::fs::write(dir.path().join("big.txt"), content).unwrap();

    let result = grep(&dir, json!({ "pattern": "match_line_" })).await;

    assert!(result.text.contains("Found 100 matches"), "output: {}", result.text);
    assert!(
        result.text.contains("Results are truncated"),
        "truncation notice missing"
    );
}

/// Scenario: lines longer than 2000 bytes are truncated with "..." suffix.
#[tokio::test]
async fn grep_long_lines_are_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let long_line = format!("START{}END", "x".repeat(3000));
    std::fs::write(dir.path().join("f.txt"), format!("{}\n", long_line)).unwrap();

    let result = grep(&dir, json!({ "pattern": "START" })).await;

    assert!(result.text.contains("..."), "long line should be truncated with '...'");
    assert!(!result.text.contains("END"), "truncated part must not appear");
}

// ---------------------------------------------------------------------------
// mod-time sort
// ---------------------------------------------------------------------------

/// Scenario: most recently modified file appears first in output.
#[tokio::test]
async fn grep_results_sorted_by_modification_time_desc() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("older.txt"), "target\n").unwrap();
    // Sleep >1 s to guarantee a distinct mtime on HFS+ (1-second resolution).
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.path().join("newer.txt"), "target\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "target" })).await;

    let older_pos = result.text.find("older.txt").expect("older.txt must appear");
    let newer_pos = result.text.find("newer.txt").expect("newer.txt must appear");
    assert!(
        newer_pos < older_pos,
        "newer file should appear before older file; output:\n{}",
        result.text
    );
}
