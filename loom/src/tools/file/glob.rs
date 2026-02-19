//! Glob tool: list files under the working folder matching a glob pattern.
//!
//! Exposes `glob` as a tool with parameters `pattern`, `path`, and `include`.
//! Path is validated to stay under the working folder. Interacts with
//! [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec),
//! [`resolve_path_under`](super::path::resolve_path_under).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use glob::Pattern;
use serde_json::json;
use walkdir::WalkDir;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for glob file search.
pub const TOOL_GLOB: &str = "glob";

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
}

impl GlobTool {
    /// Creates a new GlobTool with the given working folder.
    ///
    /// The path is not canonicalized here; the caller must pass a canonical path
    /// (e.g. from [`FileToolSource::new`](crate::tool_source::FileToolSource::new)).
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        TOOL_GLOB
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
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
        let path_param = if path_param.is_empty() { "." } else { path_param };

        let search_root = resolve_path_under(self.working_folder.as_ref(), path_param)?;
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
                    .map(|s| Pattern::new(s))
                    .filter_map(|p| p.ok())
                    .collect()
            })
            .unwrap_or_default();

        let main_pattern =
            Pattern::new(pattern_str).map_err(|e| {
                ToolSourceError::InvalidInput(format!("invalid glob pattern: {}", e))
            })?;

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
                    && !include_patterns
                        .iter()
                        .any(|p| p.matches(&rel_working_str))
                {
                    return None;
                }
                Some(rel_working_str)
            })
            .collect();
        matched.sort();
        matched.dedup();

        Ok(ToolCallContent {
            text: matched.join("\n"),
        })
    }
}
