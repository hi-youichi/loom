//! LSP tool: provides code completion, diagnostics, and navigation using Language Server Protocol.
//!
//! This tool integrates with language servers to provide intelligent code analysis capabilities.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::lsp::LspManager;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for LSP.
pub const TOOL_LSP: &str = "lsp";

/// LSP tool provides code completion, diagnostics, and navigation features.
pub struct LspTool {
    manager: std::sync::Arc<tokio::sync::RwLock<LspManager>>,
}

impl LspTool {
    /// Create a new LSP tool with the given manager.
    pub fn new(manager: std::sync::Arc<tokio::sync::RwLock<LspManager>>) -> Self {
        Self { manager }
    }

    /// Create a placeholder LSP tool (for backwards compatibility).
    pub fn placeholder() -> Self {
        let manager = std::sync::Arc::new(tokio::sync::RwLock::new(
            LspManager::from_configs(env_config::get_default_lsp_servers()),
        ));
        Self { manager }
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::placeholder()
    }
}

/// LSP action types
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum LspAction {
    /// Get code completions
    Completion {
        file_path: String,
        line: u32,
        character: u32,
    },
    /// Get diagnostics
    Diagnostics {
        file_path: String,
    },
    /// Go to definition
    GotoDefinition {
        file_path: String,
        line: u32,
        character: u32,
    },
    /// Find references
    FindReferences {
        file_path: String,
        line: u32,
        character: u32,
    },
    /// Get hover information
    Hover {
        file_path: String,
        line: u32,
        character: u32,
    },
    /// Get document symbols
    DocumentSymbols {
        file_path: String,
    },
}

/// Simplified completion item for output
#[derive(Debug, Serialize)]
struct CompletionItem {
    label: String,
    kind: String,
    detail: Option<String>,
}

/// Simplified diagnostic item for output
#[derive(Debug, Serialize)]
struct DiagnosticItem {
    severity: String,
    message: String,
    line: u32,
    character: u32,
    source: Option<String>,
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        TOOL_LSP
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_LSP.to_string(),
            description: Some(
                "LSP-based code completions and diagnostics. Provides intelligent code completion, \
                 diagnostics, go-to-definition, find references, hover information, and document symbols."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["completion", "diagnostics", "gotoDefinition", "findReferences", "hover", "documentSymbols"],
                        "description": "LSP action to perform"
                    },
                    "file_path": { "type": "string", "description": "Path to the file" },
                    "line": { "type": "integer", "description": "Line number (0-based)" },
                    "character": { "type": "integer", "description": "Character position (0-based)" }
                },
                "required": ["action", "file_path"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let action: LspAction = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let manager = self.manager.read().await;

        let file_path = match &action {
            LspAction::Completion { file_path, .. } => file_path.clone(),
            LspAction::Diagnostics { file_path } => file_path.clone(),
            LspAction::GotoDefinition { file_path, .. } => file_path.clone(),
            LspAction::FindReferences { file_path, .. } => file_path.clone(),
            LspAction::Hover { file_path, .. } => file_path.clone(),
            LspAction::DocumentSymbols { file_path } => file_path.clone(),
        };

        let path = std::path::Path::new(&file_path);
        let content = std::fs::read_to_string(path)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Failed to read file: {}", e)))?;

        manager.open_document(path, &content).await
            .map_err(|e| ToolSourceError::InvalidInput(format!("Failed to open document: {}", e)))?;

        match action {
            LspAction::Completion { file_path, line, character } => {
                let path = std::path::Path::new(&file_path);
                let result = manager
                    .completion(path, line, character)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Completion failed: {}", e)))?;

                let items: Vec<CompletionItem> = result
                    .iter()
                    .map(|item| CompletionItem {
                        label: item.label.clone(),
                        kind: format!("{:?}", item.kind),
                        detail: item.detail.clone(),
                    })
                    .collect();

                let output = serde_json::to_string_pretty(&items)
                    .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;

                Ok(ToolCallContent { text: output })
            }

            LspAction::Diagnostics { file_path } => {
                let path = std::path::Path::new(&file_path);
                let diagnostics = manager
                    .diagnostics(path)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Diagnostics failed: {}", e)))?;

                let items: Vec<DiagnosticItem> = diagnostics
                    .iter()
                    .map(|diag| DiagnosticItem {
                        severity: format!("{:?}", diag.severity),
                        message: diag.message.clone(),
                        line: diag.range.start.line,
                        character: diag.range.start.character,
                        source: diag.source.clone(),
                    })
                    .collect();

                let output = serde_json::to_string_pretty(&items)
                    .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;

                Ok(ToolCallContent { text: output })
            }

            LspAction::GotoDefinition { file_path, line, character } => {
                let path = std::path::Path::new(&file_path);
                let locations = manager
                    .goto_definition(path, line, character)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Goto definition failed: {}", e)))?;

                let output = if locations.is_empty() {
                    "No definition found".to_string()
                } else {
                    serde_json::to_string_pretty(&locations)
                        .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?
                };

                Ok(ToolCallContent { text: output })
            }

            LspAction::FindReferences { file_path, line, character } => {
                let path = std::path::Path::new(&file_path);
                let result = manager
                    .find_references(path, line, character)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Find references failed: {}", e)))?;

                let output = serde_json::to_string_pretty(&result)
                    .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;

                Ok(ToolCallContent { text: output })
            }

            LspAction::Hover { file_path, line, character } => {
                let path = std::path::Path::new(&file_path);
                let result = manager
                    .hover(path, line, character)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Hover failed: {}", e)))?;

                let output = match result {
                    Some(hover) => {
                        let content = match hover.contents {
                            lsp_types::HoverContents::Scalar(s) => format!("{:?}", s),
                            lsp_types::HoverContents::Array(arr) => {
                                arr.iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>().join("\n")
                            }
                            lsp_types::HoverContents::Markup(mc) => mc.value,
                        };
                        content
                    }
                    None => "No hover information available".to_string(),
                };

                Ok(ToolCallContent { text: output })
            }

            LspAction::DocumentSymbols { file_path } => {
                let path = std::path::Path::new(&file_path);
                let result = manager
                    .document_symbols(path)
                    .await
                    .map_err(|e| ToolSourceError::InvalidInput(format!("Document symbols failed: {}", e)))?;

                let output = serde_json::to_string_pretty(&result)
                    .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;

                Ok(ToolCallContent { text: output })
            }
        }
    }
}
