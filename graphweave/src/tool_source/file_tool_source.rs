//! File tool source: file operations under a working folder as tools.
//!
//! Exposes ls, read, write_file, move_file, delete_file, create_dir, glob.
//! All paths are validated to stay under the working folder. Uses
//! [`AggregateToolSource`](crate::tools::AggregateToolSource) internally.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::file::{
    CreateDirTool, DeleteFileTool, EditFileTool, GlobTool, GrepTool, LsTool, MoveFileTool,
    ReadFileTool, WriteFileTool,
};
use crate::tools::todo::{TodoReadTool, TodoWriteTool};
use crate::tools::AggregateToolSource;

/// Registers file tools (ls, read, write_file, edit, move_file, delete_file, create_dir, glob, grep, todo_write, todo_read)
/// on an existing [`AggregateToolSource`].
///
/// Use this to combine file tools with memory, web, or MCP tools in one source.
/// The path must exist and be a directory; it is canonicalized before use.
///
/// # Errors
///
/// - [`ToolSourceError::InvalidInput`] if the path does not exist, is not a directory,
///   or canonicalization fails.
///
/// # Interaction
///
/// Used by the ReAct builder when a working folder is set so file tools are
/// aggregated with memory, web, and MCP tools.
pub fn register_file_tools(
    aggregate: &AggregateToolSource,
    working_folder: impl AsRef<Path>,
) -> Result<(), ToolSourceError> {
    let path = working_folder.as_ref();
    let canonical = path.canonicalize().map_err(|e| {
        ToolSourceError::InvalidInput(format!(
            "working folder not found or not a directory: {}",
            e
        ))
    })?;
    if !canonical.is_dir() {
        return Err(ToolSourceError::InvalidInput(
            "working folder is not a directory".to_string(),
        ));
    }
    let working_folder = Arc::new(canonical);
    aggregate.register_sync(Box::new(LsTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(ReadFileTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(WriteFileTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(EditFileTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(MoveFileTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(DeleteFileTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(CreateDirTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(GlobTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(GrepTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(TodoWriteTool::new(working_folder.clone())));
    aggregate.register_sync(Box::new(TodoReadTool::new(working_folder)));
    Ok(())
}

/// Tool source that exposes file operations under a fixed working folder.
///
/// Paths are validated to be under the working folder (canonical subpath).
/// Use with ActNode or aggregate with other tool sources. Creates the
/// working folder as canonical at construction; it must exist and be a directory.
pub struct FileToolSource {
    _source: AggregateToolSource,
}

impl FileToolSource {
    /// Creates a file tool source with the given working folder.
    ///
    /// The path must exist and be a directory; it is canonicalized and stored.
    /// Returns a [`FileToolSource`] that implements [`ToolSource`] for use with ActNode.
    ///
    /// # Errors
    ///
    /// - [`ToolSourceError::InvalidInput`] if the path does not exist, is not
    ///   a directory, or canonicalization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tool_source::FileToolSource;
    /// use std::path::Path;
    /// # fn main() -> Result<(), graphweave::tool_source::ToolSourceError> {
    /// let source = FileToolSource::new(Path::new("/tmp/my_workspace"))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(working_folder: impl AsRef<Path>) -> Result<Self, ToolSourceError> {
        let path = working_folder.as_ref();
        let canonical = path.canonicalize().map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "working folder not found or not a directory: {}",
                e
            ))
        })?;
        if !canonical.is_dir() {
            return Err(ToolSourceError::InvalidInput(
                "working folder is not a directory".to_string(),
            ));
        }
        let working_folder = Arc::new(canonical);
        let source = AggregateToolSource::new();
        source.register_sync(Box::new(LsTool::new(working_folder.clone())));
        source.register_sync(Box::new(ReadFileTool::new(working_folder.clone())));
        source.register_sync(Box::new(WriteFileTool::new(working_folder.clone())));
        source.register_sync(Box::new(EditFileTool::new(working_folder.clone())));
        source.register_sync(Box::new(MoveFileTool::new(working_folder.clone())));
        source.register_sync(Box::new(DeleteFileTool::new(working_folder.clone())));
        source.register_sync(Box::new(CreateDirTool::new(working_folder.clone())));
        source.register_sync(Box::new(GlobTool::new(working_folder.clone())));
        source.register_sync(Box::new(GrepTool::new(working_folder.clone())));
        source.register_sync(Box::new(TodoWriteTool::new(working_folder.clone())));
        source.register_sync(Box::new(TodoReadTool::new(working_folder)));
        Ok(FileToolSource { _source: source })
    }
}

#[async_trait]
impl ToolSource for FileToolSource {
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
        ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source
            .call_tool_with_context(name, arguments, ctx)
            .await
    }

    fn set_call_context(&self, ctx: Option<crate::tool_source::ToolCallContext>) {
        self._source.set_call_context(ctx)
    }
}
