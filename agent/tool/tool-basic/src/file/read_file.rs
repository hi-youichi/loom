//! Read-file tool: read text content of a file under the working folder.
//!
//! Exposes `read` as a tool for the LLM. Path is validated to be under
//! working folder. Supports offset/limit for long files. Interacts with
//! [`Tool`](tool_core::Tool), [`ToolSpec`](tool_core::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use anureo_util::text::truncate::truncate;
use tool_core::Tool;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};

use super::path::resolve_path;

/// Tool name for reading a file.
pub use tool_core::tool_name::TOOL_READ_FILE;

const DEFAULT_READ_LIMIT: usize = 2000;
const MAX_LINE_LENGTH: usize = 2000;

/// Tool that reads text content of a file under the working folder.
///
/// Supports offset (0-based line index) and limit. Uses UTF-8; lines longer
/// than MAX_LINE_LENGTH are truncated. Output format: "  {line_num}\t{content}".
pub struct ReadFileTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
    pub(crate) allow_outside: bool,
}

impl ReadFileTool {
    /// Creates a new ReadFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>, allow_outside: bool) -> Self {
        Self {
            working_folder,
            allow_outside,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        TOOL_READ_FILE
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_READ_FILE.to_string(),
            description: Some(
                "Read file content. Path relative to working folder. Optional offset (0-based) and limit (default 2000). \
                 Output in cat -n style with line numbers."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "0-based line number to start reading from.",
                        "minimum": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max lines to read (default 2000).",
                        "minimum": 1,
                        "default": 2000
                    },
                    "encoding": {
                        "type": "string",
                        "description": "Optional encoding (e.g. 'utf-8'). Default utf-8.",
                        "default": "utf-8"
                    }
                },
                "required": ["path"]
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
            .ok_or_else(|| ToolSourceError::InvalidInput("missing path".to_string()))?;
        let path = resolve_path(self.working_folder.as_ref(), path_param, self.allow_outside)?;
        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "file not found: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "is a directory, not a file: {}",
                path.display()
            )));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?;

        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_READ_LIMIT);

        let lines: Vec<&str> = content.split('\n').collect();
        let total = lines.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        let selected = &lines[start..end];

        let mut out = String::new();
        for (i, line) in selected.iter().enumerate() {
            let line_num = start + i + 1;
            let truncated = if line.len() > MAX_LINE_LENGTH {
                format!("{}...", truncate(line, MAX_LINE_LENGTH))
            } else {
                (*line).to_string()
            };
            out.push_str(&format!("  {}\t{}\n", line_num, truncated));
        }
        Ok(ToolCallContent::text(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: byte slice at MAX_LINE_LENGTH=2000 panicked when a line
    /// exceeded 2000 bytes and byte 2000 fell inside a CJK char (e.g. '户').
    /// `用` (U+7528) is 3 bytes in UTF-8; 700 copies = 2100 bytes > 2000,
    /// and byte 2000..2003 of that range is inside the 667th character.
    #[tokio::test]
    async fn read_file_long_cjk_line_truncates_on_char_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let long_cjk = "用".repeat(700); // 2100 bytes, > MAX_LINE_LENGTH=2000
        let file = tmp.path().join("cjk.txt");
        std::fs::write(&file, format!("{}\nshort\n", long_cjk)).unwrap();

        let tool = ReadFileTool::new(Arc::new(tmp.path().to_path_buf()), false);
        let res = tool
            .call(json!({"path": "cjk.txt"}), None)
            .await
            .expect("CJK line must not panic during truncation");
        let text = res.as_text().expect("text content");

        // First line is the truncated CJK line; must start with `"  1\t"` and
        // end with the truncation marker. The trailing empty line (from the
        // terminal `\n`) makes the whole text end with `\n`, so we check the
        // first line in isolation.
        assert!(text.starts_with("  1\t"));
        assert!(text.contains("..."));
        let first_line_end = text.find('\n').expect("first line present");
        assert!(text[..first_line_end].ends_with("..."));

        // The CJK content must be carried through. After floor_char_boundary(2000)
        // it lands at byte 1998 (one full "用" earlier), so the line starts with
        // 666 visible CJK chars.
        assert!(text.contains("用"));
    }

    /// Short lines (including non-ASCII but under the limit) pass through unchanged.
    #[tokio::test]
    async fn read_file_short_multibyte_line_passes_through() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("hi.txt");
        std::fs::write(&file, "你好世界\n").unwrap();

        let tool = ReadFileTool::new(Arc::new(tmp.path().to_path_buf()), false);
        let res = tool
            .call(json!({"path": "hi.txt"}), None)
            .await
            .expect("must succeed");
        let text = res.as_text().expect("text content");
        assert!(text.contains("你好世界"));
        assert!(!text.contains("..."));
    }
}
