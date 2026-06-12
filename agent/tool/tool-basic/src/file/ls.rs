//! ls tool: list files and directories as a tree under the working folder.
//!
//! Exposes `ls` as a tool with optional `path` and `ignore` parameters. Walks
//! the directory recursively using `walkdir`, skips common build/dependency
//! directories, caps results at 100 files, and renders a tree-style listing.
//! Interacts with [`Tool`](tool_core::Tool), [`ToolSpec`](tool_core::ToolSpec).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use glob::Pattern;
use serde_json::json;
use walkdir::WalkDir;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use tool_core::Tool;

use super::path::resolve_path_under;

/// Tool name for tree-style directory listing.
pub const TOOL_LS: &str = "ls";

/// Maximum number of files returned before truncating.
const LIMIT: usize = 100;

/// Directory/path segments that are ignored by default.
const IGNORE_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".git",
    "dist",
    "build",
    "target",
    "vendor",
    "bin",
    "obj",
    ".idea",
    ".vscode",
    ".zig-cache",
    "zig-out",
    ".coverage",
    "coverage",
    "tmp",
    "temp",
    ".cache",
    "cache",
    "logs",
    ".venv",
    "venv",
    "env",
];

/// Returns `true` if the directory entry's file name matches a default-ignored segment.
fn is_default_ignored(name: &str) -> bool {
    IGNORE_DIRS.contains(&name)
}

/// Tool that lists files and subdirectories as a tree.
///
/// Path is relative to the working folder; defaults to ".". Optional `ignore`
/// provides additional glob patterns (matched against the relative path from the
/// search root). Results are capped at [`LIMIT`] files.
pub struct LsTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl LsTool {
    /// Creates a new LsTool with the given working folder.
    ///
    /// The path is not canonicalized here; the caller must pass a canonical path.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        TOOL_LS
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_LS.to_string(),
            description: Some(
                "List files and directories as a tree. Path is relative to the working folder \
                 (default \".\"). Common build/dependency directories are ignored. Results are \
                 capped at 100 files. Prefer Glob and Grep when you know which directories to search."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to working folder (use \".\" or omit for root)."
                    },
                    "ignore": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Additional glob patterns to ignore."
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

        let search_root = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if !search_root.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "not a directory: {}",
                search_root.display()
            )));
        }

        let extra_ignores: Vec<Pattern> = args
            .get("ignore")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim()).filter(|s| !s.is_empty()))
                    .filter_map(|s| Pattern::new(s).ok())
                    .collect()
            })
            .unwrap_or_default();

        let mut files: Vec<String> = Vec::new();
        let mut truncated = false;

        'walk: for entry in WalkDir::new(&search_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                if e.file_type().is_dir() && is_default_ignored(&name) {
                    return false;
                }
                true
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = match entry.path().strip_prefix(&search_root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");

            if extra_ignores.iter().any(|p| p.matches(&rel_str)) {
                continue;
            }

            files.push(rel_str);
            if files.len() >= LIMIT {
                truncated = true;
                break 'walk;
            }
        }

        files.sort();

        // Build directory tree
        let mut dirs: HashSet<String> = HashSet::new();
        let mut files_by_dir: HashMap<String, Vec<String>> = HashMap::new();

        for file in &files {
            let dir = match Path::new(file).parent() {
                Some(p) if p.as_os_str().is_empty() => ".".to_string(),
                Some(p) => p.to_string_lossy().replace('\\', "/"),
                None => ".".to_string(),
            };
            let filename = Path::new(file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Add all parent directories
            dirs.insert(".".to_string());
            let parts: Vec<&str> = if dir == "." {
                vec![]
            } else {
                dir.split('/').collect()
            };
            for i in 1..=parts.len() {
                dirs.insert(parts[..i].join("/"));
            }
            dirs.insert(dir.clone());

            files_by_dir.entry(dir).or_default().push(filename);
        }

        fn render_dir(
            dir_path: &str,
            depth: usize,
            dirs: &HashSet<String>,
            files_by_dir: &HashMap<String, Vec<String>>,
        ) -> String {
            let indent = "  ".repeat(depth);
            let mut output = String::new();

            if depth > 0 {
                let name = dir_path.rsplit('/').next().unwrap_or(dir_path);
                output.push_str(&format!("{}{}/\n", indent, name));
            }

            let child_indent = "  ".repeat(depth + 1);

            // Collect and sort child directories
            let mut children: Vec<&str> = dirs
                .iter()
                .map(|d| d.as_str())
                .filter(|&d| {
                    let parent = match d.rfind('/') {
                        Some(i) => &d[..i],
                        None => ".",
                    };
                    parent == dir_path && d != dir_path
                })
                .collect();
            children.sort();

            for child in children {
                output.push_str(&render_dir(child, depth + 1, dirs, files_by_dir));
            }

            // Render files in this directory
            let mut dir_files: Vec<&str> = files_by_dir
                .get(dir_path)
                .map(|v| v.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();
            dir_files.sort();
            for file in dir_files {
                output.push_str(&format!("{}{}\n", child_indent, file));
            }

            output
        }

        let root_label = search_root.display().to_string();
        let mut output = format!("{}/\n", root_label);
        output.push_str(&render_dir(".", 0, &dirs, &files_by_dir));

        if truncated {
            output.push_str(&format!("\n(truncated: showing first {} files)\n", LIMIT));
        }

        Ok(ToolCallContent::text(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "hello").unwrap();
        fs::write(dir.path().join("file2.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/build.out"), "").unwrap();
        dir
    }

    #[test]
    fn test_tool_ls_constant() {
        assert_eq!(TOOL_LS, "ls");
    }

    #[test]
    fn test_is_default_ignored() {
        assert!(is_default_ignored("target"));
        assert!(is_default_ignored(".git"));
        assert!(is_default_ignored("node_modules"));
        assert!(is_default_ignored("dist"));
        assert!(is_default_ignored("build"));
        assert!(is_default_ignored("vendor"));
        assert!(is_default_ignored("bin"));
        assert!(is_default_ignored("obj"));
        assert!(is_default_ignored(".idea"));
        assert!(is_default_ignored(".vscode"));
        assert!(is_default_ignored(".zig-cache"));
        assert!(is_default_ignored("zig-out"));
        assert!(is_default_ignored(".coverage"));
        assert!(is_default_ignored("coverage"));
        assert!(is_default_ignored("tmp"));
        assert!(is_default_ignored("temp"));
        assert!(is_default_ignored(".cache"));
        assert!(is_default_ignored("cache"));
        assert!(is_default_ignored("logs"));
        assert!(is_default_ignored(".venv"));
        assert!(is_default_ignored("venv"));
        assert!(is_default_ignored("env"));
        assert!(!is_default_ignored("src"));
        assert!(!is_default_ignored("my_dir"));
        assert!(!is_default_ignored("custom"));
    }

    #[test]
    fn test_ls_tool_new() {
        let dir = tempfile::tempdir().unwrap();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        assert_eq!(tool.working_folder.as_ref(), dir.path());
    }

    #[tokio::test]
    async fn test_ls_tool_name_and_spec() {
        let dir = tempfile::tempdir().unwrap();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        assert_eq!(tool.name(), "ls");
        
        let spec = tool.spec();
        assert_eq!(spec.name, "ls");
        assert!(spec.description.is_some());
        assert!(spec.description.unwrap().contains("tree"));
        assert!(spec.input_schema.is_object());
    }

    #[tokio::test]
    async fn test_ls_lists_files_in_tree_format() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        assert!(text.contains("file1.txt"));
        assert!(text.contains("file2.rs"));
        assert!(text.contains("main.rs"));
        assert!(text.contains("src/"));
        // target should be ignored
        assert!(!text.contains("build.out"));
        assert!(!text.contains("target/"));
    }

    #[tokio::test]
    async fn test_ls_with_path_param() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({"path": "src"}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        assert!(text.contains("main.rs"));
        assert!(!text.contains("file1.txt")); // root files should not appear
        assert!(!text.contains("file2.rs"));
    }

    #[tokio::test]
    async fn test_ls_with_ignore_patterns() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(
            serde_json::json!({"ignore": ["*.txt"]}), 
            None
        ).await.unwrap();
        let text = result.as_text().unwrap();
        
        assert!(!text.contains("file1.txt")); // .txt files should be ignored
        assert!(text.contains("file2.rs"));   // .rs files should appear
        assert!(text.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_ls_with_complex_ignore_patterns() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(
            serde_json::json!({"ignore": ["src/*", "*.txt"]}), 
            None
        ).await.unwrap();
        let text = result.as_text().unwrap();
        
        assert!(!text.contains("main.rs"));  // src/* should ignore src contents
        assert!(!text.contains("file1.txt")); // .txt files should be ignored
        assert!(text.contains("file2.rs"));   // .rs file in root should appear
    }

    #[tokio::test]
    async fn test_ls_not_a_directory_error() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();
        
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({"path": "file.txt"}), None).await;
        
        assert!(result.is_err());
        if let Err(ToolSourceError::InvalidInput(msg)) = result {
            assert!(msg.contains("not a directory"));
        } else {
            panic!("Expected InvalidInput error");
        }
    }

    #[tokio::test]
    async fn test_ls_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let empty_dir = dir.path().join("empty");
        fs::create_dir(&empty_dir).unwrap();
        
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({"path": "empty"}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        assert!(text.contains("empty/")); // should show directory name
        assert!(!text.contains("truncated")); // should not show truncation message
    }

    #[tokio::test]
    async fn test_ls_default_path_is_dot() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        
        // Test with no path argument (should default to ".")
        let result1 = tool.call(serde_json::json!({}), None).await.unwrap();
        let text1 = result1.as_text().unwrap();
        
        // Test with explicit "."
        let result2 = tool.call(serde_json::json!({"path": "."}), None).await.unwrap();
        let text2 = result2.as_text().unwrap();
        
        // Both should produce the same content
        assert_eq!(text1, text2);
        
        // Should contain our test files
        assert!(text1.contains("file1.txt"));
        assert!(text1.contains("file2.rs"));
    }

    #[tokio::test]
    async fn test_ls_empty_path_defaults_to_dot() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        
        // Test with empty path argument (should default to ".")
        let result = tool.call(serde_json::json!({"path": ""}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        // Should contain our test files
        assert!(text.contains("file1.txt"));
        assert!(text.contains("file2.rs"));
    }

    #[tokio::test]
    async fn test_ls_with_whitespace_path() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        
        // Test with whitespace path argument (should default to ".")
        let result = tool.call(serde_json::json!({"path": "   "}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        // Should contain our test files
        assert!(text.contains("file1.txt"));
        assert!(text.contains("file2.rs"));
    }

    #[tokio::test]
    async fn test_ls_spec_json_schema() {
        let dir = tempfile::tempdir().unwrap();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let spec = tool.spec();
        
        // Check input schema structure
        let schema = spec.input_schema;
        assert_eq!(schema["type"], "object");
        
        // Check path parameter
        let path_props = &schema["properties"]["path"];
        assert_eq!(path_props["type"], "string");
        assert!(path_props["description"].is_string());
        assert!(path_props["description"].as_str().unwrap().contains("relative to working folder"));
        
        // Check ignore parameter
        let ignore_props = &schema["properties"]["ignore"];
        assert_eq!(ignore_props["type"], "array");
        assert_eq!(ignore_props["items"]["type"], "string");
        assert!(ignore_props["description"].is_string());
    }

    #[tokio::test]
    async fn test_ls_tree_structure_format() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({}), None).await.unwrap();
        let text = result.as_text().unwrap();
        
        // Check that tree structure is present
        assert!(text.contains("/")); // root directory
        assert!(text.contains("src/")); // subdirectory with trailing slash
        
        // Files should not have trailing slashes
        let lines: Vec<&str> = text.lines().collect();
        for line in lines {
            if line.contains("file1.txt") || line.contains("file2.rs") || line.contains("main.rs") {
                assert!(!line.ends_with("/"), "Files should not have trailing slashes: {}", line);
            }
        }
    }

    #[tokio::test]
    async fn test_ls_multiple_ignore_patterns() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        
        // Add more test files for testing multiple patterns
        fs::write(dir.path().join("test.py"), "print('hello')").unwrap();
        fs::write(dir.path().join("data.json"), "{}").unwrap();
        
        let result = tool.call(
            serde_json::json!({"ignore": ["*.py", "*.json", "*.txt"]}), 
            None
        ).await.unwrap();
        let text = result.as_text().unwrap();
        
        // All specified patterns should be ignored
        assert!(!text.contains("file1.txt")); // .txt ignored
        assert!(!text.contains("test.py"));   // .py ignored
        assert!(!text.contains("data.json")); // .json ignored
        
        // Other files should still appear
        assert!(text.contains("file2.rs"));
        assert!(text.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_ls_invalid_ignore_patterns() {
        let dir = setup_test_dir();
        let tool = LsTool::new(Arc::new(dir.path().to_path_buf()));
        
        // Invalid glob patterns should be ignored without error
        let result = tool.call(
            serde_json::json!({"ignore": ["[invalid[", "*.txt"]}), 
            None
        ).await.unwrap();
        let text = result.as_text().unwrap();
        
        // Should still work, just ignoring the invalid pattern
        assert!(!text.contains("file1.txt")); // .txt ignored by valid pattern
        assert!(text.contains("file2.rs"));
    }
}
