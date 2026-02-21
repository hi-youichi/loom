//! ls tool: list files and directories as a tree under the working folder.
//!
//! Exposes `ls` as a tool with optional `path` and `ignore` parameters. Walks
//! the directory recursively using `walkdir`, skips common build/dependency
//! directories, caps results at 100 files, and renders a tree-style listing.
//! Interacts with [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use glob::Pattern;
use serde_json::json;
use walkdir::WalkDir;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

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
    IGNORE_DIRS.iter().any(|&d| d == name)
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

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
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

        Ok(ToolCallContent { text: output })
    }
}
