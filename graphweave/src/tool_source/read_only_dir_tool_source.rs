//! Read-only directory connector: list_dir and read_file for a second root (no write/delete).
//!
//! Use as a connector example: expose a read-only view of a directory (e.g. reference docs,
//! cloud mount). Register with [`register_read_only_dir_tools`] on an [`AggregateToolSource`]
//! to aggregate with [`FileToolSource`](crate::tool_source::FileToolSource) (writable working folder).
//! Tool names are prefixed (`read_only_list_dir`, `read_only_read_file`) to avoid collision.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::file::resolve_path_under;
use crate::tools::{AggregateToolSource, Tool};

/// Tool name for read-only list_dir (connector).
pub const TOOL_READ_ONLY_LIST_DIR: &str = "read_only_list_dir";
/// Tool name for read-only read_file (connector).
pub const TOOL_READ_ONLY_READ_FILE: &str = "read_only_read_file";

/// Registers read-only directory tools on an existing [`AggregateToolSource`].
///
/// The path must exist and be a directory; it is canonicalized. Tools are
/// `read_only_list_dir` and `read_only_read_file` (path relative to this root).
/// Use with [`register_file_tools`](crate::tool_source::file_tool_source::register_file_tools)
/// to combine a writable working folder and a read-only connector root.
///
/// # Errors
///
/// - [`ToolSourceError::InvalidInput`] if the path does not exist or is not a directory.
pub fn register_read_only_dir_tools(
    aggregate: &AggregateToolSource,
    read_only_root: impl AsRef<Path>,
) -> Result<(), ToolSourceError> {
    let path = read_only_root.as_ref();
    let canonical = path.canonicalize().map_err(|e| {
        ToolSourceError::InvalidInput(format!(
            "read_only root not found or not a directory: {}",
            e
        ))
    })?;
    if !canonical.is_dir() {
        return Err(ToolSourceError::InvalidInput(
            "read_only root is not a directory".to_string(),
        ));
    }
    let root = Arc::new(canonical);
    aggregate.register_sync(Box::new(ReadOnlyListDirTool { root: root.clone() }));
    aggregate.register_sync(Box::new(ReadOnlyReadFileTool { root }));
    Ok(())
}

/// Tool that lists entries in the read-only connector root.
struct ReadOnlyListDirTool {
    root: Arc<std::path::PathBuf>,
}

#[async_trait]
impl Tool for ReadOnlyListDirTool {
    fn name(&self) -> &str {
        TOOL_READ_ONLY_LIST_DIR
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_READ_ONLY_LIST_DIR.to_string(),
            description: Some(
                "List entries in a directory (read-only connector root). Path is relative to the read-only root.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to read-only root. Use '.' for root."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        let path_param = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing path".to_string()))?;
        let path = resolve_path_under(self.root.as_ref(), path_param)?;
        if !path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "not a directory: {}",
                path.display()
            )));
        }
        let mut names: Vec<String> = std::fs::read_dir(&path)
            .map_err(|e| ToolSourceError::Transport(format!("list_dir failed: {}", e)))?
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        let text = names.join("\n");
        Ok(ToolCallContent { text })
    }
}

/// Tool that reads a file from the read-only connector root.
struct ReadOnlyReadFileTool {
    root: Arc<std::path::PathBuf>,
}

#[async_trait]
impl Tool for ReadOnlyReadFileTool {
    fn name(&self) -> &str {
        TOOL_READ_ONLY_READ_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_READ_ONLY_READ_FILE.to_string(),
            description: Some(
                "Read file content (read-only connector root). Path is relative to the read-only root.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to read-only root."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        let path_param = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing path".to_string()))?;
        let path = resolve_path_under(self.root.as_ref(), path_param)?;
        if !path.is_file() {
            return Err(ToolSourceError::InvalidInput(format!(
                "not a file: {}",
                path.display()
            )));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("read_file failed: {}", e)))?;
        Ok(ToolCallContent { text })
    }
}

/// Read-only directory tool source: list_dir and read_file for a single root.
///
/// Use when you need a second, read-only root in addition to the writable working folder.
/// For aggregation with [`FileToolSource`], use [`register_read_only_dir_tools`] on the
/// same [`AggregateToolSource`] after [`register_file_tools`](crate::tool_source::file_tool_source::register_file_tools).
pub struct ReadOnlyDirToolSource {
    _source: AggregateToolSource,
}

impl ReadOnlyDirToolSource {
    /// Creates a read-only directory tool source for the given root.
    ///
    /// The path must exist and be a directory. Returns a [`ReadOnlyDirToolSource`]
    /// that implements [`ToolSource`](crate::tool_source::ToolSource).
    ///
    /// # Errors
    ///
    /// - [`ToolSourceError::InvalidInput`] if the path does not exist or is not a directory.
    pub fn new(read_only_root: impl AsRef<Path>) -> Result<Self, ToolSourceError> {
        let source = AggregateToolSource::new();
        register_read_only_dir_tools(&source, read_only_root)?;
        Ok(Self { _source: source })
    }
}

#[async_trait]
impl crate::tool_source::ToolSource for ReadOnlyDirToolSource {
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self._source.list_tools().await
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments).await
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source
            .call_tool_with_context(name, arguments, ctx)
            .await
    }
}
