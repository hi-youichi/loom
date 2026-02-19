//! Grep tool: search file contents under the working folder using regular expressions.
//!
//! Exposes `grep` as a tool with parameters `pattern`, `path`, and `include`.
//! Pure-Rust implementation using [`regex`] for content matching and [`walkdir`] for
//! recursive directory traversal; no external binary is required.
//! Results are sorted by file modification time (most recently modified first)
//! and capped at [`MAX_MATCHES`]. Binary files (detected by null bytes) are skipped.
//! Interacts with [`Tool`](crate::tools::Tool),
//! [`ToolSpec`](crate::tool_source::ToolSpec), and
//! [`resolve_path_under`](super::path::resolve_path_under).

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use glob::Pattern;
use regex::Regex;
use serde_json::json;
use walkdir::WalkDir;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for grep file content search.
pub const TOOL_GREP: &str = "grep";

/// Maximum bytes per matched line before truncation (char-boundary safe).
const MAX_LINE_LENGTH: usize = 2000;

/// Maximum number of match entries returned.
const MAX_MATCHES: usize = 100;

/// A single match entry collected during the directory walk.
struct Match {
    path: String,
    mod_time: SystemTime,
    line_num: usize,
    line_text: String,
}

/// Tool that searches file contents under the working folder using regular expressions.
///
/// Walks the directory tree with [`walkdir`], filters filenames with [`glob::Pattern`],
/// and matches lines with [`regex::Regex`]. Results are sorted by file modification
/// time descending and capped at [`MAX_MATCHES`].
pub struct GrepTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl GrepTool {
    /// Creates a new GrepTool with the given working folder.
    ///
    /// The caller must pass a canonical, existing directory path.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

/// Expand one level of brace alternatives in a glob pattern.
///
/// `"*.{ts,tsx}"` → `["*.ts", "*.tsx"]`.  
/// Patterns without braces, or with malformed braces, are returned as-is.
fn expand_braces(pattern: &str) -> Vec<String> {
    if let (Some(start), Some(end)) = (pattern.find('{'), pattern.rfind('}')) {
        if start < end {
            let prefix = &pattern[..start];
            let suffix = &pattern[end + 1..];
            return pattern[start + 1..end]
                .split(',')
                .map(|alt| format!("{}{}{}", prefix, alt.trim(), suffix))
                .collect();
        }
    }
    vec![pattern.to_string()]
}

/// Build a set of [`glob::Pattern`]s from a single include string.
///
/// Brace alternatives are expanded before compiling each pattern.
fn build_include_patterns(include: &str) -> Result<Vec<Pattern>, ToolSourceError> {
    expand_braces(include)
        .iter()
        .map(|p| {
            Pattern::new(p)
                .map_err(|e| ToolSourceError::InvalidInput(format!("invalid glob pattern: {}", e)))
        })
        .collect()
}

/// Returns `true` if the byte slice contains a null byte, indicating binary content.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0u8)
}

/// Truncates a string to at most `max_bytes` bytes, respecting UTF-8 char boundaries.
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        TOOL_GREP
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_GREP.to_string(),
            description: Some(
                "Search file contents under the working folder using a regular expression. \
                 Returns matching file paths and line numbers sorted by modification time \
                 (most recently modified first)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The regex pattern to search for in file contents."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory under working folder to search in. Defaults to '.'."
                    },
                    "include": {
                        "type": "string",
                        "description": "File glob pattern to restrict search (e.g. '*.rs', '*.{ts,tsx}')."
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
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("pattern is required".to_string()))?
            .trim();
        if pattern.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "pattern must be non-empty".to_string(),
            ));
        }

        let re = Regex::new(pattern)
            .map_err(|e| ToolSourceError::InvalidInput(format!("invalid regex: {}", e)))?;

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

        let include = args
            .get("include")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        let include_patterns: Option<Vec<Pattern>> = include
            .map(build_include_patterns)
            .transpose()?;

        let mut matches: Vec<Match> = Vec::new();

        for entry in WalkDir::new(&search_root).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();

            // Apply include glob filter against the filename (basename).
            if let Some(ref patterns) = include_patterns {
                let fname = file_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                if !patterns.iter().any(|p| p.matches(&fname)) {
                    continue;
                }
            }

            let mod_time = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            // Read raw bytes first; skip binary files and unreadable entries.
            let bytes = match std::fs::read(file_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if is_binary(&bytes) {
                continue;
            }

            let content = String::from_utf8_lossy(&bytes);
            let path_str = file_path.to_string_lossy().into_owned();

            for (line_idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    matches.push(Match {
                        path: path_str.clone(),
                        mod_time,
                        line_num: line_idx + 1,
                        line_text: line.to_string(),
                    });
                }
            }
        }

        // Sort by modification time descending (most recently modified first).
        matches.sort_by(|a, b| b.mod_time.cmp(&a.mod_time));

        let truncated = matches.len() > MAX_MATCHES;
        if truncated {
            matches.truncate(MAX_MATCHES);
        }

        if matches.is_empty() {
            return Ok(ToolCallContent {
                text: "No files found".to_string(),
            });
        }

        let mut output_lines: Vec<String> = vec![format!("Found {} matches", matches.len())];
        let mut current_file = String::new();
        for m in &matches {
            if current_file != m.path {
                if !current_file.is_empty() {
                    output_lines.push(String::new());
                }
                current_file = m.path.clone();
                output_lines.push(format!("{}:", m.path));
            }
            let text = truncate_str(&m.line_text, MAX_LINE_LENGTH);
            let line_entry = if text.len() < m.line_text.len() {
                format!("  Line {}: {}...", m.line_num, text)
            } else {
                format!("  Line {}: {}", m.line_num, text)
            };
            output_lines.push(line_entry);
        }

        if truncated {
            output_lines.push(String::new());
            output_lines.push(
                "(Results are truncated. Consider using a more specific path or pattern.)"
                    .to_string(),
            );
        }

        Ok(ToolCallContent {
            text: output_lines.join("\n"),
        })
    }
}
