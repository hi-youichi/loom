//! Unit tests for GrepTool: regex content search under working folder (ripgrep lib + ignore).
//!
//! Scenarios: basic match; no match; missing/empty/invalid pattern; path escaping;
//! include glob filter; brace expansion; subdirectory scoping; binary file skipping;
//! multiple matches in one file; case sensitivity; mod-time sort order;
//! .gitignore / .ignore (ripgrep-style) exclusion.

mod init_logging;

use std::sync::Arc;
use tool_basic::GrepTool;
use tool_core::{Tool, ToolCallContent, ToolSourceError};
use serde_json::json;

fn make_grep(dir: &tempfile::TempDir) -> GrepTool {
    GrepTool::new(Arc::new(dir.path().to_path_buf()))
}

async fn grep(dir: &tempfile::TempDir, args: serde_json::Value) -> ToolCallContent {
    make_grep(dir).call(args, None).await.unwrap()
}

async fn grep_err(dir: &tempfile::TempDir, args: serde_json::Value) -> ToolSourceError {
    make_grep(dir).call(args, None).await.unwrap_err()
}

// ---------------------------------------------------------------------------
// basic matching
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_basic_match_returns_path_and_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello world\nfoo bar\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hello" })).await;

    assert!(result.as_text().unwrap().contains("a.txt"), "expected filename in output");
    assert!(result.as_text().unwrap().contains("Line 1"), "expected line number");
    assert!(result.as_text().unwrap().contains("hello world"), "expected matched line");
    assert!(!result.as_text().unwrap().contains("foo bar"), "non-matching line must not appear");
}

#[tokio::test]
async fn grep_multiple_lines_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.rs"), "fn foo() {}\nfn bar() {}\nfn baz() {}\n").unwrap();

    let result = grep(&dir, json!({ "pattern": r"fn \w+" })).await;

    assert!(result.as_text().unwrap().contains("Line 1"));
    assert!(result.as_text().unwrap().contains("Line 2"));
    assert!(result.as_text().unwrap().contains("Line 3"));
    assert!(result.as_text().unwrap().contains("Found 3 matches"));
}

#[tokio::test]
async fn grep_matches_in_multiple_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.txt"), "needle here\n").unwrap();
    std::fs::write(dir.path().join("y.txt"), "needle there\n").unwrap();
    std::fs::write(dir.path().join("z.txt"), "nothing\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "needle" })).await;

    assert!(result.as_text().unwrap().contains("x.txt"));
    assert!(result.as_text().unwrap().contains("y.txt"));
    assert!(!result.as_text().unwrap().contains("z.txt"));
    assert!(result.as_text().unwrap().contains("Found 2 matches"));
}

// ---------------------------------------------------------------------------
// no match / empty results
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_no_match_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "xyzzy" })).await;

    assert_eq!(result.as_text().unwrap(), "No files found");
}

#[tokio::test]
async fn grep_empty_folder_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();

    let result = grep(&dir, json!({ "pattern": "anything" })).await;

    assert_eq!(result.as_text().unwrap(), "No files found");
}

// ---------------------------------------------------------------------------
// error cases: pattern validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_missing_pattern_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({})).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn grep_empty_pattern_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

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

#[tokio::test]
async fn grep_path_outside_working_folder_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "x", "path": "../.." })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("outside"));
}

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

#[tokio::test]
async fn grep_include_filter_restricts_to_matching_extension() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "fn search() {}\n").unwrap();
    std::fs::write(dir.path().join("config.toml"), "search = true\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "search", "include": "*.rs" })).await;

    assert!(result.as_text().unwrap().contains("lib.rs"), "lib.rs should match");
    assert!(!result.as_text().unwrap().contains("config.toml"), "config.toml must be excluded");
}

#[tokio::test]
async fn grep_include_brace_expansion_matches_multiple_extensions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "token\n").unwrap();
    std::fs::write(dir.path().join("config.toml"), "token\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "token\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "token", "include": "*.{rs,toml}" })).await;

    assert!(result.as_text().unwrap().contains("lib.rs"));
    assert!(result.as_text().unwrap().contains("config.toml"));
    assert!(!result.as_text().unwrap().contains("notes.txt"), "txt must be excluded");
}

#[tokio::test]
async fn grep_include_no_match_returns_no_files_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hello", "include": "*.rs" })).await;

    assert_eq!(result.as_text().unwrap(), "No files found");
}

#[tokio::test]
async fn grep_include_invalid_glob_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();

    let err = grep_err(&dir, json!({ "pattern": "x", "include": "[invalid" })).await;

    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

// ---------------------------------------------------------------------------
// path scoping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_path_restricts_to_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("lib.rs"), "find_me\n").unwrap();
    std::fs::write(dir.path().join("root.rs"), "find_me\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "find_me", "path": "src" })).await;

    assert!(result.as_text().unwrap().contains("lib.rs"), "src/lib.rs should be found");
    assert!(!result.as_text().unwrap().contains("root.rs"), "root.rs is outside path, must be excluded");
}

#[tokio::test]
async fn grep_default_path_searches_recursively() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("deep")).unwrap();
    std::fs::create_dir(dir.path().join("deep").join("nested")).unwrap();
    std::fs::write(dir.path().join("deep").join("nested").join("file.txt"), "buried_token\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "buried_token" })).await;

    assert!(result.as_text().unwrap().contains("buried_token"), "nested file should be found");
    assert!(result.as_text().unwrap().contains("file.txt"));
}

// ---------------------------------------------------------------------------
// binary file handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_binary_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let binary: Vec<u8> = b"binary_match\x00garbage\n".to_vec();
    std::fs::write(dir.path().join("data.bin"), &binary).unwrap();
    std::fs::write(dir.path().join("text.txt"), "binary_match\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "binary_match" })).await;

    assert!(result.as_text().unwrap().contains("text.txt"), "text file must appear");
    assert!(!result.as_text().unwrap().contains("data.bin"), "binary file must be skipped");
}

// ---------------------------------------------------------------------------
// regex features
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_is_case_sensitive_by_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "Hello World\n").unwrap();

    let lower_result = grep(&dir, json!({ "pattern": "hello world" })).await;
    assert_eq!(lower_result.as_text().unwrap(), "No files found", "lowercase should not match uppercase file");

    let upper_result = grep(&dir, json!({ "pattern": "Hello World" })).await;
    assert!(upper_result.as_text().unwrap().contains("f.txt"), "exact-case should match");
}

#[tokio::test]
async fn grep_inline_case_insensitive_flag_works() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "Hello World\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "(?i)hello world" })).await;

    assert!(result.as_text().unwrap().contains("f.txt"));
    assert!(result.as_text().unwrap().contains("Hello World"));
}

#[tokio::test]
async fn grep_anchored_pattern_matches_exact_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "exact\nexact match\nnot exact\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "^exact$" })).await;

    assert!(result.as_text().unwrap().contains("Line 1"), "line 1 'exact' should match");
    assert!(!result.as_text().unwrap().contains("Line 2"), "line 2 'exact match' must not match ^exact$");
    assert!(!result.as_text().unwrap().contains("Line 3"));
}

// ---------------------------------------------------------------------------
// output format
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_output_header_shows_match_count() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hit\nhit\nnope\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "hit" })).await;

    assert!(result.as_text().unwrap().starts_with("Found 2 matches"), "output: {}", result.as_text().unwrap());
}

#[tokio::test]
async fn grep_results_truncated_at_100_matches() {
    let dir = tempfile::tempdir().unwrap();
    let content: String = (0..110).map(|i| format!("match_line_{}\n", i)).collect();
    std::fs::write(dir.path().join("big.txt"), content).unwrap();

    let result = grep(&dir, json!({ "pattern": "match_line_" })).await;

    assert!(result.as_text().unwrap().contains("Found 100 matches"), "output: {}", result.as_text().unwrap());
    assert!(result.as_text().unwrap().contains("Results are truncated"), "truncation notice missing");
}

#[tokio::test]
async fn grep_long_lines_are_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let long_line = format!("START{}END", "x".repeat(3000));
    std::fs::write(dir.path().join("f.txt"), format!("{}\n", long_line)).unwrap();

    let result = grep(&dir, json!({ "pattern": "START" })).await;

    assert!(result.as_text().unwrap().contains("..."), "long line should be truncated with '...'");
    assert!(!result.as_text().unwrap().contains("END"), "truncated part must not appear");
}

// ---------------------------------------------------------------------------
// ignore (ripgrep-style: .gitignore / .ignore / .rgignore)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grep_gitignore_excludes_listed_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(dir.path().join("ignored.txt"), "needle\n").unwrap();
    std::fs::write(dir.path().join("ok.txt"), "needle\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "needle" })).await;

    assert!(result.as_text().unwrap().contains("ok.txt"), "non-ignored file must appear; output: {}", result.as_text().unwrap());
    assert!(!result.as_text().unwrap().contains("ignored.txt"), "gitignored file must not be searched; output: {}", result.as_text().unwrap());
}

#[tokio::test]
async fn grep_gitignore_excludes_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".gitignore"), "skip_dir/\n").unwrap();
    std::fs::create_dir(dir.path().join("skip_dir")).unwrap();
    std::fs::write(dir.path().join("skip_dir").join("secret.txt"), "needle\n").unwrap();
    std::fs::write(dir.path().join("public.txt"), "needle\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "needle" })).await;

    assert!(result.as_text().unwrap().contains("public.txt"), "file outside ignored dir must appear; output: {}", result.as_text().unwrap());
    assert!(!result.as_text().unwrap().contains("skip_dir") && !result.as_text().unwrap().contains("secret.txt"), "files under gitignored dir must not be searched; output: {}", result.as_text().unwrap());
}

#[tokio::test]
async fn grep_dot_ignore_excludes_listed_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".ignore"), "ignored_by_ignore\n").unwrap();
    std::fs::write(dir.path().join("ignored_by_ignore"), "needle\n").unwrap();
    std::fs::write(dir.path().join("visible.txt"), "needle\n").unwrap();

    let result = grep(&dir, json!({ "pattern": "needle" })).await;

    assert!(result.as_text().unwrap().contains("visible.txt"));
    assert!(!result.as_text().unwrap().contains("ignored_by_ignore"));
}
