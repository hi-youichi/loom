//! LSP type definitions and protocol structures.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// LSP Position in a document (0-based).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// LSP Range in a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// LSP Location (URI + Range).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// Text document item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentItem {
    pub uri: String,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

/// Completion item kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Unit = 11,
    Value = 12,
    Enum = 13,
    Keyword = 14,
    Snippet = 15,
    Color = 16,
    File = 17,
    Reference = 18,
    Folder = 19,
    EnumMember = 20,
    Constant = 21,
    Struct = 22,
    Event = 23,
    Operator = 24,
    TypeParameter = 25,
}

/// Completion item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CompletionItemKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// Diagnostic related information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRelatedInformation {
    pub location: Location,
    pub message: String,
}

/// Diagnostic (error/warning/info).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<DiagnosticSeverity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,
}

/// Symbol kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,
    Key = 20,
    Null = 21,
    EnumMember = 22,
    Struct = 23,
    Event = 24,
    Operator = 25,
    TypeParameter = 26,
}

/// Document symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<DocumentSymbol>>,
}

/// Symbol information (for workspace symbols).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInformation {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

/// Hover content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hover {
    pub contents: HoverContents,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// Hover contents (can be string or markup).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
    String(String),
    Markup(MarkupContent),
}

/// Markup content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: MarkupKind,
    pub value: String,
}

/// Markup kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarkupKind {
    #[serde(rename = "plaintext")]
    PlainText,
    #[serde(rename = "markdown")]
    Markdown,
}

/// Text edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// File event for file system watchers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEvent {
    pub uri: String,
    pub r#type: FileType,
}

/// File type for file events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileType {
    Created = 1,
    Changed = 2,
    Deleted = 3,
}

/// Initialize result from language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document_sync: Option<TextDocumentSyncCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_provider: Option<CompletionOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_symbol_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_symbol_provider: Option<bool>,
}

/// Text document sync capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextDocumentSyncCapability {
    Kind(TextDocumentSyncKind),
    Options(TextDocumentSyncOptions),
}

/// Text document sync kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextDocumentSyncKind {
    None = 0,
    Full = 1,
    Incremental = 2,
}

/// Text document sync options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentSyncOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_close: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<TextDocumentSyncKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_save: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_save_wait_until: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save: Option<bool>,
}

/// Completion options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolve_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_characters: Option<Vec<String>>,
}

/// Server info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Language identifier based on file extension.
pub fn language_id_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;

    let language_id = match ext {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "cpp" | "cc" | "cxx" => "cpp",
        "c" => "c",
        "h" | "hpp" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" => "kotlin",
        "scala" => "scala",
        "lua" => "lua",
        _ => ext,
    };

    Some(language_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_new() {
        let pos = Position::new(5, 10);
        assert_eq!(pos.line, 5);
        assert_eq!(pos.character, 10);
    }

    #[test]
    fn test_range_new() {
        let start = Position::new(0, 0);
        let end = Position::new(5, 10);
        let range = Range::new(start, end);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.end.line, 5);
    }

    #[test]
    fn test_position_serialization() {
        let pos = Position {
            line: 5,
            character: 10,
        };
        let json = serde_json::to_string(&pos).unwrap();
        let deserialized: Position = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.line, 5);
        assert_eq!(deserialized.character, 10);
    }

    #[test]
    fn test_position_eq() {
        let pos1 = Position {
            line: 5,
            character: 10,
        };
        let pos2 = Position {
            line: 5,
            character: 10,
        };
        let pos3 = Position {
            line: 6,
            character: 10,
        };
        assert_eq!(pos1, pos2);
        assert_ne!(pos1, pos3);
    }

    #[test]
    fn test_completion_item_kind_variants() {
        assert_eq!(CompletionItemKind::Text as u32, 1);
        assert_eq!(CompletionItemKind::Method as u32, 2);
        assert_eq!(CompletionItemKind::Function as u32, 3);
        assert_eq!(CompletionItemKind::Constructor as u32, 4);
        assert_eq!(CompletionItemKind::Field as u32, 5);
        assert_eq!(CompletionItemKind::Variable as u32, 6);
        assert_eq!(CompletionItemKind::Class as u32, 7);
        assert_eq!(CompletionItemKind::Interface as u32, 8);
        assert_eq!(CompletionItemKind::Module as u32, 9);
    }

    #[test]
    fn test_completion_item_serialization() {
        let item = CompletionItem {
            label: "test_function".to_string(),
            kind: Some(CompletionItemKind::Function),
            detail: Some("fn test_function() -> i32".to_string()),
            documentation: Some("Test function documentation".to_string()),
            insert_text: Some("test_function()".to_string()),
            sort_text: Some("test_function".to_string()),
        };

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: CompletionItem = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.label, "test_function");
        assert_eq!(deserialized.kind, Some(CompletionItemKind::Function));
        assert_eq!(
            deserialized.detail,
            Some("fn test_function() -> i32".to_string())
        );
    }

    #[test]
    fn test_completion_item_minimal() {
        let item = CompletionItem {
            label: "minimal".to_string(),
            kind: None,
            detail: None,
            documentation: None,
            insert_text: None,
            sort_text: None,
        };

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: CompletionItem = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.label, "minimal");
        assert!(deserialized.kind.is_none());
    }

    #[test]
    fn test_diagnostic_severity_variants() {
        assert_eq!(DiagnosticSeverity::Error as u32, 1);
        assert_eq!(DiagnosticSeverity::Warning as u32, 2);
        assert_eq!(DiagnosticSeverity::Information as u32, 3);
        assert_eq!(DiagnosticSeverity::Hint as u32, 4);
    }

    #[test]
    fn test_diagnostic_serialization() {
        let diagnostic = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 10,
                },
            },
            message: "Test error message".to_string(),
            severity: Some(DiagnosticSeverity::Error),
            code: Some("E0001".to_string()),
            source: Some("test".to_string()),
            related_information: None,
        };

        let json = serde_json::to_string(&diagnostic).unwrap();
        let deserialized: Diagnostic = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.message, "Test error message");
        assert_eq!(deserialized.severity, Some(DiagnosticSeverity::Error));
        assert_eq!(deserialized.code, Some("E0001".to_string()));
        assert_eq!(deserialized.source, Some("test".to_string()));
    }

    #[test]
    fn test_symbol_kind_variants() {
        assert_eq!(SymbolKind::File as u32, 1);
        assert_eq!(SymbolKind::Module as u32, 2);
        assert_eq!(SymbolKind::Namespace as u32, 3);
        assert_eq!(SymbolKind::Package as u32, 4);
        assert_eq!(SymbolKind::Class as u32, 5);
        assert_eq!(SymbolKind::Method as u32, 6);
        assert_eq!(SymbolKind::Function as u32, 12);
        assert_eq!(SymbolKind::Variable as u32, 13);
    }

    #[test]
    fn test_document_symbol_serialization() {
        let symbol = DocumentSymbol {
            name: "test_function".to_string(),
            detail: Some("fn test_function()".to_string()),
            kind: SymbolKind::Function,
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 5,
                    character: 0,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 16,
                },
            },
            children: None,
        };

        let json = serde_json::to_string(&symbol).unwrap();
        let deserialized: DocumentSymbol = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test_function");
        assert_eq!(deserialized.kind, SymbolKind::Function);
    }

    #[test]
    fn test_symbol_information_serialization() {
        let info = SymbolInformation {
            name: "test_symbol".to_string(),
            kind: SymbolKind::Variable,
            location: Location {
                uri: "file:///test.rs".to_string(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
            },
            container_name: Some("test_module".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SymbolInformation = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test_symbol");
        assert_eq!(deserialized.kind, SymbolKind::Variable);
        assert_eq!(deserialized.container_name, Some("test_module".to_string()));
    }

    #[test]
    fn test_hover_contents_string() {
        let contents = HoverContents::String("Simple hover text".to_string());
        let json = serde_json::to_string(&contents).unwrap();
        let deserialized: HoverContents = serde_json::from_str(&json).unwrap();

        match deserialized {
            HoverContents::String(s) => assert_eq!(s, "Simple hover text"),
            _ => panic!("Expected String variant"),
        }
    }

    #[test]
    fn test_hover_contents_markup() {
        let markup = MarkupContent {
            kind: MarkupKind::Markdown,
            value: "**Bold text**".to_string(),
        };
        let contents = HoverContents::Markup(markup);
        let json = serde_json::to_string(&contents).unwrap();
        let deserialized: HoverContents = serde_json::from_str(&json).unwrap();

        match deserialized {
            HoverContents::Markup(m) => {
                assert_eq!(m.kind, MarkupKind::Markdown);
                assert_eq!(m.value, "**Bold text**");
            }
            _ => panic!("Expected Markup variant"),
        }
    }

    #[test]
    fn test_markup_kind_serialization() {
        let plaintext = MarkupKind::PlainText;
        let json = serde_json::to_string(&plaintext).unwrap();
        assert!(json.contains("plaintext"));

        let markdown = MarkupKind::Markdown;
        let json = serde_json::to_string(&markdown).unwrap();
        assert!(json.contains("markdown"));
    }

    #[test]
    fn test_text_edit_serialization() {
        let edit = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            },
            new_text: "replaced".to_string(),
        };

        let json = serde_json::to_string(&edit).unwrap();
        let deserialized: TextEdit = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.new_text, "replaced");
        assert_eq!(deserialized.range.start.line, 0);
    }

    #[test]
    fn test_file_type_variants() {
        assert_eq!(FileType::Created as u32, 1);
        assert_eq!(FileType::Changed as u32, 2);
        assert_eq!(FileType::Deleted as u32, 3);
    }

    #[test]
    fn test_file_event_serialization() {
        let event = FileEvent {
            uri: "file:///test.rs".to_string(),
            r#type: FileType::Changed,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: FileEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.uri, "file:///test.rs");
        assert_eq!(deserialized.r#type, FileType::Changed);
    }

    #[test]
    fn test_text_document_sync_kind_variants() {
        assert_eq!(TextDocumentSyncKind::None as u32, 0);
        assert_eq!(TextDocumentSyncKind::Full as u32, 1);
        assert_eq!(TextDocumentSyncKind::Incremental as u32, 2);
    }

    #[test]
    fn test_text_document_sync_capability_kind() {
        let capability = TextDocumentSyncCapability::Kind(TextDocumentSyncKind::Full);
        let json = serde_json::to_string(&capability).unwrap();
        let deserialized: TextDocumentSyncCapability = serde_json::from_str(&json).unwrap();

        match deserialized {
            TextDocumentSyncCapability::Kind(kind) => {
                assert_eq!(kind, TextDocumentSyncKind::Full);
            }
            _ => panic!("Expected Kind variant"),
        }
    }

    #[test]
    fn test_text_document_sync_capability_options() {
        let options = TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::Incremental),
            will_save: Some(false),
            will_save_wait_until: None,
            save: Some(true),
        };

        let capability = TextDocumentSyncCapability::Options(options);
        let json = serde_json::to_string(&capability).unwrap();
        let deserialized: TextDocumentSyncCapability = serde_json::from_str(&json).unwrap();

        match deserialized {
            TextDocumentSyncCapability::Options(opts) => {
                assert_eq!(opts.open_close, Some(true));
                assert_eq!(opts.change, Some(TextDocumentSyncKind::Incremental));
            }
            _ => panic!("Expected Options variant"),
        }
    }

    #[test]
    fn test_completion_options() {
        let options = CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
        };

        let json = serde_json::to_string(&options).unwrap();
        let deserialized: CompletionOptions = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.resolve_provider, Some(true));
        assert!(deserialized.trigger_characters.is_some());
        let triggers = deserialized.trigger_characters.unwrap();
        assert_eq!(triggers.len(), 2);
    }

    #[test]
    fn test_server_info() {
        let info = ServerInfo {
            name: "test-server".to_string(),
            version: Some("1.0.0".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ServerInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test-server");
        assert_eq!(deserialized.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_server_capabilities() {
        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::Full)),
            completion_provider: Some(CompletionOptions {
                resolve_provider: Some(false),
                trigger_characters: None,
            }),
            hover_provider: Some(true),
            definition_provider: Some(true),
            references_provider: Some(true),
            document_symbol_provider: Some(true),
            workspace_symbol_provider: Some(false),
        };

        let json = serde_json::to_string(&capabilities).unwrap();
        let deserialized: ServerCapabilities = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.hover_provider, Some(true));
        assert_eq!(deserialized.definition_provider, Some(true));
        assert_eq!(deserialized.references_provider, Some(true));
    }

    #[test]
    fn test_initialize_result() {
        let result = InitializeResult {
            capabilities: ServerCapabilities {
                hover_provider: Some(true),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "test-server".to_string(),
                version: Some("1.0.0".to_string()),
            }),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: InitializeResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_info.unwrap().name, "test-server");
        assert_eq!(deserialized.capabilities.hover_provider, Some(true));
    }

    #[test]
    fn test_language_id_from_path_rust() {
        let path = Path::new("/path/to/file.rs");
        assert_eq!(language_id_from_path(path), Some("rust".to_string()));
    }

    #[test]
    fn test_language_id_from_path_typescript() {
        let path = Path::new("/path/to/file.ts");
        assert_eq!(language_id_from_path(path), Some("typescript".to_string()));

        let path = Path::new("/path/to/file.tsx");
        assert_eq!(
            language_id_from_path(path),
            Some("typescriptreact".to_string())
        );
    }

    #[test]
    fn test_language_id_from_path_javascript() {
        let path = Path::new("/path/to/file.js");
        assert_eq!(language_id_from_path(path), Some("javascript".to_string()));

        let path = Path::new("/path/to/file.jsx");
        assert_eq!(
            language_id_from_path(path),
            Some("javascriptreact".to_string())
        );
    }

    #[test]
    fn test_language_id_from_path_python() {
        let path = Path::new("/path/to/file.py");
        assert_eq!(language_id_from_path(path), Some("python".to_string()));
    }

    #[test]
    fn test_language_id_from_path_go() {
        let path = Path::new("/path/to/file.go");
        assert_eq!(language_id_from_path(path), Some("go".to_string()));
    }

    #[test]
    fn test_language_id_from_path_java() {
        let path = Path::new("/path/to/file.java");
        assert_eq!(language_id_from_path(path), Some("java".to_string()));
    }

    #[test]
    fn test_language_id_from_path_cpp() {
        let path = Path::new("/path/to/file.cpp");
        assert_eq!(language_id_from_path(path), Some("cpp".to_string()));

        let path = Path::new("/path/to/file.cc");
        assert_eq!(language_id_from_path(path), Some("cpp".to_string()));

        let path = Path::new("/path/to/file.cxx");
        assert_eq!(language_id_from_path(path), Some("cpp".to_string()));
    }

    #[test]
    fn test_language_id_from_path_c() {
        let path = Path::new("/path/to/file.c");
        assert_eq!(language_id_from_path(path), Some("c".to_string()));

        let path = Path::new("/path/to/file.h");
        assert_eq!(language_id_from_path(path), Some("cpp".to_string()));

        let path = Path::new("/path/to/file.hpp");
        assert_eq!(language_id_from_path(path), Some("cpp".to_string()));
    }

    #[test]
    fn test_language_id_from_path_csharp() {
        let path = Path::new("/path/to/file.cs");
        assert_eq!(language_id_from_path(path), Some("csharp".to_string()));
    }

    #[test]
    fn test_language_id_from_path_ruby() {
        let path = Path::new("/path/to/file.rb");
        assert_eq!(language_id_from_path(path), Some("ruby".to_string()));
    }

    #[test]
    fn test_language_id_from_path_php() {
        let path = Path::new("/path/to/file.php");
        assert_eq!(language_id_from_path(path), Some("php".to_string()));
    }

    #[test]
    fn test_language_id_from_path_swift() {
        let path = Path::new("/path/to/file.swift");
        assert_eq!(language_id_from_path(path), Some("swift".to_string()));
    }

    #[test]
    fn test_language_id_from_path_kotlin() {
        let path = Path::new("/path/to/file.kt");
        assert_eq!(language_id_from_path(path), Some("kotlin".to_string()));
    }

    #[test]
    fn test_language_id_from_path_scala() {
        let path = Path::new("/path/to/file.scala");
        assert_eq!(language_id_from_path(path), Some("scala".to_string()));
    }

    #[test]
    fn test_language_id_from_path_lua() {
        let path = Path::new("/path/to/file.lua");
        assert_eq!(language_id_from_path(path), Some("lua".to_string()));
    }

    #[test]
    fn test_language_id_from_path_unknown() {
        let path = Path::new("/path/to/file.unknown");
        assert_eq!(language_id_from_path(path), Some("unknown".to_string()));
    }

    #[test]
    fn test_language_id_from_path_no_extension() {
        let path = Path::new("/path/to/file");
        assert_eq!(language_id_from_path(path), None);
    }

    #[test]
    fn test_language_id_from_path_utf8() {
        let path = Path::new("/path/to/文件.rs");
        assert_eq!(language_id_from_path(path), Some("rust".to_string()));
    }

    #[test]
    fn test_text_document_item() {
        let item = TextDocumentItem {
            uri: "file:///test.rs".to_string(),
            language_id: "rust".to_string(),
            version: 1,
            text: "fn main() {}".to_string(),
        };

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: TextDocumentItem = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.uri, "file:///test.rs");
        assert_eq!(deserialized.language_id, "rust");
        assert_eq!(deserialized.version, 1);
        assert_eq!(deserialized.text, "fn main() {}");
    }

    #[test]
    fn test_location_serialization() {
        let location = Location {
            uri: "file:///test.rs".to_string(),
            range: Range {
                start: Position {
                    line: 5,
                    character: 10,
                },
                end: Position {
                    line: 5,
                    character: 15,
                },
            },
        };

        let json = serde_json::to_string(&location).unwrap();
        let deserialized: Location = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.uri, "file:///test.rs");
        assert_eq!(deserialized.range.start.line, 5);
        assert_eq!(deserialized.range.start.character, 10);
    }

    #[test]
    fn test_hover_serialization() {
        let hover = Hover {
            contents: HoverContents::String("Test hover content".to_string()),
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 5,
                },
            }),
        };

        let json = serde_json::to_string(&hover).unwrap();
        let deserialized: Hover = serde_json::from_str(&json).unwrap();

        assert!(deserialized.range.is_some());
        let range = deserialized.range.unwrap();
        assert_eq!(range.start.line, 0);
    }
}
