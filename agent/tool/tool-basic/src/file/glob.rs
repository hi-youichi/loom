//! Glob tool: list files under the working folder matching a glob pattern.
//!
//! Exposes `glob` as a tool with parameters `pattern`, `path`, and `include`.
//! Path is validated to stay under the working folder. Interacts with
//! [`Tool`](tool_core::Tool), [`ToolSpec`](tool_core::ToolSpec),
//! [`resolve_path_under`](super::path::resolve_path_under).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use glob::Pattern;
use serde_json::json;
use walkdir::WalkDir;

use tool_core::Tool;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};

use super::path::resolve_path;

/// Tool name for glob file search.
pub use tool_core::tool_name::TOOL_GLOB;

/// Normalizes a path string for glob matching: use forward slashes so that
/// `glob::Pattern` (Unix-style) matches correctly on all platforms.
fn path_str_for_glob(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Tool that lists files under the working folder matching a glob pattern.
///
/// Search root is given by `path` (default "."). Pattern is relative to that root.
/// Optional `include` filters results by additional patterns (path relative to working folder).
/// Interacts with [`resolve_path_under`] for path validation.
pub struct GlobTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
    pub(crate) allow_outside: bool,
}

impl GlobTool {
    /// Creates a new GlobTool with the given working folder.
    ///
    /// The path is not canonicalized here; the caller must pass a canonical path
    /// (e.g. from [`FileToolSource::new`](tool_core::FileToolSource::new)).
    pub fn new(working_folder: Arc<std::path::PathBuf>, allow_outside: bool) -> Self {
        Self {
            working_folder,
            allow_outside,
        }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        TOOL_GLOB
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_GLOB.to_string(),
            description: Some(
                "List files under the working folder that match a glob pattern. Use path to \
                 restrict search to a subdirectory; use include to filter results by additional \
                 patterns."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern relative to path (e.g. '*.rs', '**/*.yaml'). Use '**' for recursive."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory under working folder to search in. Default '.'."
                    },
                    "include": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of patterns; only include paths matching any of these (extra filter)."
                    }
                },
                "required": ["pattern"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let pattern_str = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("pattern is required".to_string()))?
            .trim();
        if pattern_str.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "pattern must be non-empty".to_string(),
            ));
        }
        if pattern_str.contains("..") {
            return Err(ToolSourceError::InvalidInput(
                "pattern must not contain '..'".to_string(),
            ));
        }

        let path_param = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .unwrap_or(".");
        let path_param = if path_param.is_empty() {
            "."
        } else {
            path_param
        };

        let search_root =
            resolve_path(self.working_folder.as_ref(), path_param, self.allow_outside)?;
        if !search_root.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "path is not a directory: {}",
                search_root.display()
            )));
        }

        let include_patterns: Vec<Pattern> = args
            .get("include")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim()).filter(|s| !s.is_empty()))
                    .map(Pattern::new)
                    .filter_map(|p| p.ok())
                    .collect()
            })
            .unwrap_or_default();

        let main_pattern = Pattern::new(pattern_str)
            .map_err(|e| ToolSourceError::InvalidInput(format!("invalid glob pattern: {}", e)))?;

        let working_folder_canon = self.working_folder.canonicalize().map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "working folder not found or not a directory: {}",
                e
            ))
        })?;

        let mut matched: Vec<String> = WalkDir::new(&search_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let full = e.path().canonicalize().ok()?;
                if !full.starts_with(&working_folder_canon) {
                    return None;
                }
                let rel_working = full.strip_prefix(&working_folder_canon).ok()?;
                let rel_search = full.strip_prefix(&search_root).ok()?;
                let rel_working_str = path_str_for_glob(rel_working);
                let rel_search_str = path_str_for_glob(rel_search);
                if !main_pattern.matches(&rel_search_str) {
                    return None;
                }
                if !include_patterns.is_empty()
                    && !include_patterns.iter().any(|p| p.matches(&rel_working_str))
                {
                    return None;
                }
                Some(rel_working_str)
            })
            .collect();
        matched.sort();
        matched.dedup();

        Ok(ToolCallContent::text(matched.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        // Create some files and directories for testing
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("test.txt"), "test content").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}").unwrap();
        fs::write(dir.path().join("src/utils.rs"), "pub fn utils() {}").unwrap();
        fs::create_dir(dir.path().join("src/models")).unwrap();
        fs::write(dir.path().join("src/models/user.rs"), "struct User {}").unwrap();
        fs::create_dir(dir.path().join("tests")).unwrap();
        fs::write(
            dir.path().join("tests/integration_test.rs"),
            "#[test] fn test() {}",
        )
        .unwrap();
        dir
    }

    #[test]
    fn test_path_str_for_glob_normalizes_separators() {
        // Test that backslashes are replaced with forward slashes
        let path = Path::new("src\\lib.rs");
        let result = path_str_for_glob(path);
        assert_eq!(result, "src/lib.rs");

        let path = Path::new("nested\\deep\\file.txt");
        let result = path_str_for_glob(path);
        assert_eq!(result, "nested/deep/file.txt");
    }

    #[test]
    fn test_path_str_for_glob_forward_slashes_unchanged() {
        // Test that forward slashes remain unchanged
        let path = Path::new("src/lib.rs");
        let result = path_str_for_glob(path);
        assert_eq!(result, "src/lib.rs");

        let path = Path::new("nested/deep/file.txt");
        let result = path_str_for_glob(path);
        assert_eq!(result, "nested/deep/file.txt");
    }

    #[test]
    fn test_path_str_for_glob_simple_filename() {
        // Test simple filename without path
        let path = Path::new("file.rs");
        let result = path_str_for_glob(path);
        assert_eq!(result, "file.rs");
    }

    #[test]
    fn test_glob_tool_new() {
        let dir = tempfile::tempdir().unwrap();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);
        assert_eq!(tool.working_folder.as_ref(), dir.path());
    }

    #[tokio::test]
    async fn test_glob_tool_name() {
        let dir = tempfile::tempdir().unwrap();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);
        assert_eq!(tool.name(), "glob");
    }

    #[tokio::test]
    async fn test_glob_tool_spec() {
        let dir = tempfile::tempdir().unwrap();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);
        let spec = tool.spec();

        assert_eq!(spec.name, "glob");
        assert!(spec.description.is_some());
        assert!(spec.description.unwrap().contains("glob pattern"));

        // Check input schema structure
        let schema = spec.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&"pattern".into()));

        // Check pattern parameter
        let pattern_props = &schema["properties"]["pattern"];
        assert_eq!(pattern_props["type"], "string");
        assert!(pattern_props["description"].is_string());

        // Check path parameter
        let path_props = &schema["properties"]["path"];
        assert_eq!(path_props["type"], "string");
        assert!(path_props["description"].is_string());

        // Check include parameter
        let include_props = &schema["properties"]["include"];
        assert_eq!(include_props["type"], "array");
        assert_eq!(include_props["items"]["type"], "string");
    }

    #[tokio::test]
    async fn test_glob_simple_pattern() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "*.rs"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        assert!(text.contains("main.rs"));
        assert!(!text.contains("test.txt")); // .txt files should not match
    }

    #[tokio::test]
    async fn test_glob_recursive_pattern() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "**/*.rs"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should find all .rs files recursively
        assert!(text.contains("main.rs"));
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("src/utils.rs"));
        assert!(text.contains("src/models/user.rs"));
        assert!(text.contains("tests/integration_test.rs"));

        // Should not find .txt files
        assert!(!text.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_glob_pattern_no_matches_empty_result() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "*.nonexistent"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        assert_eq!(text, "");
    }

    #[tokio::test]
    async fn test_glob_with_specific_path() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "*.rs", "path": "src"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should only find .rs files in src directory
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("src/utils.rs"));
        assert!(!text.contains("main.rs")); // Should not find root file
    }

    #[tokio::test]
    async fn test_glob_with_include_parameter() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(
                serde_json::json!({
                    "pattern": "*.rs",
                    "include": ["src/*", "tests/*"]
                }),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should include files that match include patterns
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("tests/integration_test.rs"));

        // Should not include root files that don't match include patterns
        assert!(!text.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_empty_pattern_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool.call(serde_json::json!({"pattern": ""}), None).await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("non-empty"));
        } else {
            panic!("Expected InvalidInput error for empty pattern");
        }
    }

    #[tokio::test]
    async fn test_glob_missing_pattern_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool.call(serde_json::json!({}), None).await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("required"));
        } else {
            panic!("Expected InvalidInput error for missing pattern");
        }
    }

    #[tokio::test]
    async fn test_glob_pattern_with_double_dot_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "../secret"}), None)
            .await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains(".."));
        } else {
            panic!("Expected InvalidInput error for pattern with ..");
        }
    }

    #[tokio::test]
    async fn test_glob_invalid_pattern_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "[invalid"}), None)
            .await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("invalid glob pattern"));
        } else {
            panic!("Expected InvalidInput error for invalid pattern");
        }
    }

    #[tokio::test]
    async fn test_glob_non_existent_path_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(
                serde_json::json!({"pattern": "*.rs", "path": "nonexistent"}),
                None,
            )
            .await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("not a directory") || msg.contains("not found"));
        } else {
            panic!("Expected InvalidInput error for non-existent path");
        }
    }

    #[tokio::test]
    async fn test_glob_path_is_file_error() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(
                serde_json::json!({"pattern": "*.rs", "path": "main.rs"}),
                None,
            )
            .await;

        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("not a directory"));
        } else {
            panic!("Expected InvalidInput error when path is a file");
        }
    }

    #[tokio::test]
    async fn test_glob_empty_path_defaults_to_dot() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        // Test with empty path argument (should default to ".")
        let result = tool
            .call(serde_json::json!({"pattern": "*.rs", "path": ""}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should find root .rs files
        assert!(text.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_whitespace_path_defaults_to_dot() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        // Test with whitespace path argument (should default to ".")
        let result = tool
            .call(serde_json::json!({"pattern": "*.rs", "path": "   "}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should find root .rs files
        assert!(text.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_default_path_is_dot() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        // Test with no path argument (should default to ".")
        let result1 = tool
            .call(serde_json::json!({"pattern": "*.rs"}), None)
            .await
            .unwrap();
        let text1 = result1.as_text().unwrap();

        // Test with explicit "."
        let result2 = tool
            .call(serde_json::json!({"pattern": "*.rs", "path": "."}), None)
            .await
            .unwrap();
        let text2 = result2.as_text().unwrap();

        // Both should produce the same result
        assert_eq!(text1, text2);
        assert!(text1.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_results_are_sorted() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "**/*.rs"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        let lines: Vec<&str> = text.lines().collect();
        // Results should be sorted alphabetically
        let mut sorted_lines = lines.clone();
        sorted_lines.sort();
        assert_eq!(lines, sorted_lines);
    }

    #[tokio::test]
    async fn test_glob_subdirectory_pattern() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(Arc::new(dir.path().to_path_buf()), false);

        let result = tool
            .call(serde_json::json!({"pattern": "src/models/*.rs"}), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();

        // Should find files only in src/models directory
        assert!(text.contains("src/models/user.rs"));
        assert!(!text.contains("src/lib.rs")); // Should not find files in parent directory
        assert!(!text.contains("main.rs")); // Should not find root files
    }
}
