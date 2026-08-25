//! Incremental document synchronization for LSP.
//!
//! Implements efficient text synchronization using LSP's TextDocumentContentChangeEvent.

use lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};
use std::collections::HashMap;

/// Represents a document being tracked for synchronization.
#[derive(Debug, Clone)]
pub struct DocumentState {
    /// Document URI
    pub uri: Url,
    /// Current document version
    pub version: i32,
    /// Full document text
    pub text: String,
    /// Language ID
    pub language_id: String,
}

/// Manages document states and computes incremental changes.
#[derive(Debug, Default)]
pub struct DocumentSyncManager {
    /// Maps URI -> DocumentState
    documents: HashMap<Url, DocumentState>,
}

/// Type alias for DocumentSyncManager
pub type DocumentSync = DocumentSyncManager;

impl DocumentSyncManager {
    /// Create a new document sync manager.
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    /// Open a document and track its state.
    pub fn open_document(&mut self, uri: Url, language_id: String, version: i32, text: String) {
        self.documents.insert(
            uri.clone(),
            DocumentState {
                uri,
                version,
                text,
                language_id,
            },
        );
    }

    /// Update a document and compute incremental changes.
    pub fn update_document(
        &mut self,
        uri: &Url,
        version: i32,
        new_text: String,
    ) -> Option<TextDocumentContentChangeEvent> {
        let doc = self.documents.get_mut(uri)?;

        // Compute diff between old and new text
        let change = if doc.text == new_text {
            // No changes
            return None;
        } else {
            // Simple implementation: full text update
            // TODO: Implement proper diff algorithm for incremental sync
            TextDocumentContentChangeEvent {
                range: None, // None means full text update
                range_length: None,
                text: new_text.clone(),
            }
        };

        doc.version = version;
        doc.text = new_text;
        Some(change)
    }

    /// Close a document and remove it from tracking.
    pub fn close_document(&mut self, uri: &Url) -> Option<DocumentState> {
        self.documents.remove(uri)
    }

    /// Get the current state of a document.
    pub fn get_document(&self, uri: &Url) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    /// Check if a document is currently open.
    pub fn is_document_open(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    /// Get all open documents.
    pub fn get_all_documents(&self) -> Vec<&DocumentState> {
        self.documents.values().collect()
    }

    /// Compute incremental change using a simple line-based diff.
    /// This is a simplified implementation; a production version would use
    /// a proper diff algorithm like Myers' diff.
    pub fn compute_incremental_change(
        old_text: &str,
        new_text: &str,
    ) -> TextDocumentContentChangeEvent {
        let old_lines: Vec<&str> = old_text.lines().collect();
        let new_lines: Vec<&str> = new_text.lines().collect();

        // Find first differing line
        let first_diff = old_lines
            .iter()
            .zip(new_lines.iter())
            .position(|(old, new)| old != new);

        // Find last differing line
        let last_diff_old = old_lines.iter().rev().enumerate().find(|(i, line)| {
            if let Some(new_line) = new_lines.iter().rev().nth(*i) {
                line != &new_line
            } else {
                true
            }
        });

        let last_diff_new = new_lines.iter().rev().enumerate().find(|(i, line)| {
            if let Some(old_line) = old_lines.iter().rev().nth(*i) {
                line != &old_line
            } else {
                true
            }
        });

        match (first_diff, last_diff_old, last_diff_new) {
            (Some(fd), Some((ldo, _)), Some((ldn, _))) => {
                // Compute range
                let start_line = fd as u32;
                let end_line_old = (old_lines.len() - 1 - ldo) as u32;
                let end_line_new = (new_lines.len() - 1 - ldn) as u32;

                let start = Position {
                    line: start_line,
                    character: 0,
                };

                let end = Position {
                    line: end_line_old,
                    character: old_lines
                        .get(end_line_old as usize)
                        .map(|l| l.len())
                        .unwrap_or(0) as u32,
                };

                let range = Range { start, end };

                // Extract the changed text from new document
                let changed_text: String = new_lines
                    .iter()
                    .skip(start_line as usize)
                    .take((end_line_new - start_line + 1) as usize)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");

                TextDocumentContentChangeEvent {
                    range: Some(range),
                    range_length: Some(
                        (end_line_old - start_line) + if end_line_old > start_line { 1 } else { 0 },
                    ),
                    text: changed_text,
                }
            }
            _ => {
                // Full update if we can't compute incremental change
                TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: new_text.to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_document() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        manager.open_document(
            uri.clone(),
            "rust".to_string(),
            1,
            "fn main() {}".to_string(),
        );

        assert!(manager.is_document_open(&uri));
        let doc = manager.get_document(&uri).unwrap();
        assert_eq!(doc.text, "fn main() {}");
        assert_eq!(doc.version, 1);
    }

    #[test]
    fn test_update_document() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        manager.open_document(
            uri.clone(),
            "rust".to_string(),
            1,
            "fn main() {}".to_string(),
        );

        let change =
            manager.update_document(&uri, 2, "fn main() { println!(\"test\"); }".to_string());

        assert!(change.is_some());
        let doc = manager.get_document(&uri).unwrap();
        assert_eq!(doc.version, 2);
        assert_eq!(doc.text, "fn main() { println!(\"test\"); }");
    }

    #[test]
    fn test_close_document() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        manager.open_document(
            uri.clone(),
            "rust".to_string(),
            1,
            "fn main() {}".to_string(),
        );

        let closed = manager.close_document(&uri);
        assert!(closed.is_some());
        assert!(!manager.is_document_open(&uri));
    }

    #[test]
    fn test_incremental_change() {
        let old_text = "line1\nline2\nline3";
        let new_text = "line1\nmodified\nline3";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_some());
        let range = change.range.unwrap();
        assert_eq!(range.start.line, 1);
        assert_eq!(range.end.line, 1);
        assert_eq!(change.text, "modified");
    }

    #[test]
    fn test_document_state_clone() {
        let uri = Url::parse("file:///test.rs").unwrap();
        let state = DocumentState {
            uri: uri.clone(),
            version: 1,
            text: "fn main() {}".to_string(),
            language_id: "rust".to_string(),
        };

        let cloned = state.clone();
        assert_eq!(state.uri, cloned.uri);
        assert_eq!(state.version, cloned.version);
        assert_eq!(state.text, cloned.text);
        assert_eq!(state.language_id, cloned.language_id);
    }

    #[test]
    fn test_document_state_debug() {
        let uri = Url::parse("file:///test.rs").unwrap();
        let state = DocumentState {
            uri: uri.clone(),
            version: 1,
            text: "fn main() {}".to_string(),
            language_id: "rust".to_string(),
        };

        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("DocumentState"));
        assert!(debug_str.contains("1"));
        assert!(debug_str.contains("rust"));
    }

    #[test]
    fn test_document_sync_manager_debug() {
        let manager = DocumentSyncManager::new();
        let debug_str = format!("{:?}", manager);
        assert!(debug_str.contains("DocumentSyncManager"));
    }

    #[test]
    fn test_document_sync_manager_default() {
        let manager = DocumentSyncManager::default();
        assert!(!manager.is_document_open(&Url::parse("file:///test.rs").unwrap()));
    }

    #[test]
    fn test_update_document_no_changes() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        manager.open_document(
            uri.clone(),
            "rust".to_string(),
            1,
            "fn main() {}".to_string(),
        );

        let change = manager.update_document(&uri, 2, "fn main() {}".to_string());

        assert!(change.is_none());
        let doc = manager.get_document(&uri).unwrap();
        assert_eq!(doc.version, 1);
        assert_eq!(doc.text, "fn main() {}");
    }

    #[test]
    fn test_update_document_nonexistent() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        let change = manager.update_document(&uri, 1, "new content".to_string());

        assert!(change.is_none());
    }

    #[test]
    fn test_close_document_nonexistent() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        let closed = manager.close_document(&uri);
        assert!(closed.is_none());
    }

    #[test]
    fn test_get_document_nonexistent() {
        let manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        let doc = manager.get_document(&uri);
        assert!(doc.is_none());
    }

    #[test]
    fn test_is_document_open_nonexistent() {
        let manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        assert!(!manager.is_document_open(&uri));
    }

    #[test]
    fn test_get_all_documents_empty() {
        let manager = DocumentSyncManager::new();
        let docs = manager.get_all_documents();
        assert!(docs.is_empty());
    }

    #[test]
    fn test_get_all_documents_multiple() {
        let mut manager = DocumentSyncManager::new();
        let uri1 = Url::parse("file:///test1.rs").unwrap();
        let uri2 = Url::parse("file:///test2.rs").unwrap();

        manager.open_document(uri1.clone(), "rust".to_string(), 1, "content1".to_string());
        manager.open_document(uri2.clone(), "rust".to_string(), 1, "content2".to_string());

        let docs = manager.get_all_documents();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_compute_incremental_change_empty_to_content() {
        let old_text = "";
        let new_text = "new content";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_none());
    }

    #[test]
    fn test_compute_incremental_change_content_to_empty() {
        let old_text = "some content";
        let new_text = "";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_none());
    }

    #[test]
    fn test_compute_incremental_change_no_changes() {
        let text = "same content";
        let change = DocumentSyncManager::compute_incremental_change(text, text);

        // Should return a full update since no differences found
        assert!(change.range.is_none() || change.text.is_empty());
    }

    #[test]
    fn test_compute_incremental_change_multiple_line_changes() {
        let old_text = "line1\nline2\nline3\nline4\nline5";
        let new_text = "line1\nmodified2\nmodified3\nline4\nline5";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_some());
        let range = change.range.unwrap();
        assert!(range.start.line <= range.end.line);
    }

    #[test]
    fn test_compute_incremental_change_single_line_to_multiple() {
        let old_text = "single line";
        let new_text = "line1\nline2\nline3";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_some());
    }

    #[test]
    fn test_compute_incremental_change_multiple_to_single_line() {
        let old_text = "line1\nline2\nline3";
        let new_text = "single line";

        let change = DocumentSyncManager::compute_incremental_change(old_text, new_text);

        assert!(change.range.is_some());
    }

    #[test]
    fn test_document_sync_type_alias() {
        let sync = DocumentSync::new();
        assert!(!sync.is_document_open(&Url::parse("file:///test.rs").unwrap()));
    }

    #[test]
    fn test_text_document_content_change_event_structure() {
        let uri = Url::parse("file:///test.rs").unwrap();
        let mut manager = DocumentSyncManager::new();

        manager.open_document(uri.clone(), "rust".to_string(), 1, "original".to_string());
        let change = manager.update_document(&uri, 2, "modified".to_string());

        assert!(change.is_some());
        let event = change.unwrap();

        match event.range {
            Some(range) => {
                assert!(range.start.line <= range.end.line);
            }
            None => {
                // Full text update
                assert_eq!(event.text, "modified");
            }
        }
    }

    #[test]
    fn test_multiple_documents_same_uri() {
        let mut manager = DocumentSyncManager::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        manager.open_document(uri.clone(), "rust".to_string(), 1, "first".to_string());
        manager.open_document(
            uri.clone(),
            "typescript".to_string(),
            2,
            "second".to_string(),
        );

        let doc = manager.get_document(&uri).unwrap();
        assert_eq!(doc.version, 2);
        assert_eq!(doc.language_id, "typescript");
        assert_eq!(doc.text, "second");
    }

    #[test]
    fn test_document_state_all_fields() {
        let uri = Url::parse("file:///test.py").unwrap();
        let state = DocumentState {
            uri: uri.clone(),
            version: 5,
            text: "def hello(): pass".to_string(),
            language_id: "python".to_string(),
        };

        assert_eq!(state.uri, uri);
        assert_eq!(state.version, 5);
        assert_eq!(state.text, "def hello(): pass");
        assert_eq!(state.language_id, "python");
    }

    #[test]
    fn test_document_sync_manager_concurrent_operations() {
        let mut manager = DocumentSyncManager::new();
        let uris: Vec<_> = (0..10)
            .map(|i| Url::parse(&format!("file:///test{}.rs", i)).unwrap())
            .collect();

        for (i, uri) in uris.iter().enumerate() {
            manager.open_document(
                uri.clone(),
                "rust".to_string(),
                i as i32,
                format!("content {}", i),
            );
        }

        assert_eq!(manager.get_all_documents().len(), 10);

        for (i, uri) in uris.iter().enumerate() {
            let doc = manager.get_document(uri).unwrap();
            assert_eq!(doc.version, i as i32);
            assert_eq!(doc.text, format!("content {}", i));
        }
    }
}
