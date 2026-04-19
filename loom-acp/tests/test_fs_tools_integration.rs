//! Integration tests for file system tools (fs/read_text_file, fs/write_text_file).
//!
//! Verifies that write operations produce correct `ToolCallContent::Diff` content
//! using a mock `ClientBridgeTrait` — no LLM involved.
//!
//! Tests run single-threaded because they share the global `CLIENT_BRIDGE` singleton.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use loom::tool_source::ToolCallContent;
use loom::tools::Tool;
use serde_json::json;
use loom_acp::tools::{clear_client_bridge, set_client_bridge, ClientBridgeTrait, ReadTextFileTool, TerminalOutput, WriteTextFileTool};

// ============================================================================
// Mock ClientBridge
// ============================================================================

#[derive(Debug, Default, Clone)]
struct BridgeCall {
    method: String,
    #[allow(dead_code)]
    path: String,
    content: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct MockBridgeInner {
    files: HashMap<String, String>,
    calls: Vec<BridgeCall>,
}

struct MockBridge {
    inner: Mutex<MockBridgeInner>,
}

impl MockBridge {
    fn new() -> Self {
        Self {
            inner: Mutex::new(MockBridgeInner::default()),
        }
    }

    fn with_file(path: &str, content: &str) -> Self {
        let mut files = HashMap::new();
        files.insert(path.to_string(), content.to_string());
        Self {
            inner: Mutex::new(MockBridgeInner {
                files,
                calls: Vec::new(),
            }),
        }
    }

    fn calls(&self) -> Vec<BridgeCall> {
        self.inner.lock().unwrap().calls.clone()
    }
}

#[async_trait]
impl ClientBridgeTrait for MockBridge {
    fn is_available(&self) -> bool {
        true
    }

    async fn read_text_file(
        &self,
        path: &str,
        _line: Option<u32>,
        _limit: Option<u32>,
    ) -> Result<String, String> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(BridgeCall {
            method: "read_text_file".to_string(),
            path: path.to_string(),
            content: None,
        });
        inner
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("File not found: {}", path))
    }

    async fn write_text_file(&self, path: &str, content: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(BridgeCall {
            method: "write_text_file".to_string(),
            path: path.to_string(),
            content: Some(content.to_string()),
        });
        inner.files.insert(path.to_string(), content.to_string());
        Ok(())
    }

    async fn create_terminal(
        &self,
        _command: &str,
        _args: Option<&[String]>,
        _cwd: Option<&str>,
        _env: Option<&HashMap<String, String>>,
        _name: Option<&str>,
    ) -> Result<String, String> {
        Err("not implemented".to_string())
    }

    async fn terminal_output(&self, _terminal_id: &str) -> Result<TerminalOutput, String> {
        Err("not implemented".to_string())
    }
}

// ============================================================================
// Helpers
// ============================================================================

async fn setup_bridge(bridge: Arc<MockBridge>) {
    clear_client_bridge().await;
    set_client_bridge(bridge as Arc<dyn ClientBridgeTrait>).await;
}

// ============================================================================
// Tests: WriteTextFileTool produces correct Diff content
// ============================================================================

#[tokio::test]
async fn test_write_new_file_returns_diff_with_no_old_text() {
    let bridge = Arc::new(MockBridge::new());
    setup_bridge(bridge.clone()).await;

    let tool = WriteTextFileTool::new();
    let result = tool
        .call(
            json!({
                "path": "src/new_module.rs",
                "content": "fn hello() {}\n"
            }),
            None,
        )
        .await
        .expect("write should succeed");

    match result {
        ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        } => {
            assert_eq!(path, "src/new_module.rs");
            assert!(
                old_text.is_none(),
                "new file should have no old_text"
            );
            assert_eq!(new_text, "fn hello() {}\n");
        }
        other => panic!("expected Diff, got: {:?}", other),
    }

    let calls = bridge.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "read_text_file");
    assert_eq!(calls[1].method, "write_text_file");
    assert_eq!(calls[1].content.as_deref(), Some("fn hello() {}\n"));
}

#[tokio::test]
async fn test_write_existing_file_returns_diff_with_old_text() {
    let bridge = Arc::new(MockBridge::with_file(
        "src/lib.rs",
        "fn old() {}\n",
    ));
    setup_bridge(bridge.clone()).await;

    let tool = WriteTextFileTool::new();
    let result = tool
        .call(
            json!({
                "path": "src/lib.rs",
                "content": "fn updated() {}\n"
            }),
            None,
        )
        .await
        .expect("write should succeed");

    match result {
        ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        } => {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(old_text, Some("fn old() {}\n".to_string()));
            assert_eq!(new_text, "fn updated() {}\n");
        }
        other => panic!("expected Diff, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_write_preserves_multiline_content() {
    let old = "line 1\nline 2\nline 3\n";
    let new = "line 1\nline 2 modified\nline 3\nline 4\n";

    let bridge = Arc::new(MockBridge::with_file("multi.txt", old));
    setup_bridge(bridge.clone()).await;

    let tool = WriteTextFileTool::new();
    let result = tool
        .call(
            json!({
                "path": "multi.txt",
                "content": new
            }),
            None,
        )
        .await
        .expect("write should succeed");

    match result {
        ToolCallContent::Diff {
            old_text,
            new_text,
            ..
        } => {
            assert_eq!(old_text, Some(old.to_string()));
            assert_eq!(new_text, new);
        }
        other => panic!("expected Diff, got: {:?}", other),
    }
}

// ============================================================================
// Tests: ReadTextFileTool returns correct content
// ============================================================================

#[tokio::test]
async fn test_read_existing_file_returns_text_content() {
    let bridge = Arc::new(MockBridge::with_file(
        "README.md",
        "# Hello\nWorld\n",
    ));
    setup_bridge(bridge.clone()).await;

    let tool = ReadTextFileTool::new();
    let result = tool
        .call(
            json!({
                "path": "README.md"
            }),
            None,
        )
        .await
        .expect("read should succeed");

    match result {
        ToolCallContent::Text(content) => {
            assert_eq!(content, "# Hello\nWorld\n");
        }
        other => panic!("expected Text, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_read_missing_file_returns_error() {
    let bridge = Arc::new(MockBridge::new());
    setup_bridge(bridge.clone()).await;

    let tool = ReadTextFileTool::new();
    let result = tool
        .call(
            json!({
                "path": "nonexistent.rs"
            }),
            None,
        )
        .await;

    assert!(result.is_err(), "reading nonexistent file should fail");
}

// ============================================================================
// Tests: Write-then-read round-trip
// ============================================================================

#[tokio::test]
async fn test_write_then_read_roundtrip() {
    let bridge = Arc::new(MockBridge::new());
    setup_bridge(bridge.clone()).await;

    let write_tool = WriteTextFileTool::new();
    write_tool
        .call(
            json!({
                "path": "roundtrip.txt",
                "content": "hello world"
            }),
            None,
        )
        .await
        .expect("write should succeed");

    let read_tool = ReadTextFileTool::new();
    let result = read_tool
        .call(
            json!({
                "path": "roundtrip.txt"
            }),
            None,
        )
        .await
        .expect("read should succeed");

    match result {
        ToolCallContent::Text(content) => {
            assert_eq!(content, "hello world");
        }
        other => panic!("expected Text, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_write_then_update_diff_chain() {
    let bridge = Arc::new(MockBridge::new());
    setup_bridge(bridge.clone()).await;

    let tool = WriteTextFileTool::new();

    let r1 = tool
        .call(json!({"path": "chain.rs", "content": "v1"}), None)
        .await
        .expect("first write");
    match r1 {
        ToolCallContent::Diff { old_text, new_text, .. } => {
            assert!(old_text.is_none());
            assert_eq!(new_text, "v1");
        }
        _ => panic!("expected Diff"),
    }

    let r2 = tool
        .call(json!({"path": "chain.rs", "content": "v2"}), None)
        .await
        .expect("second write");
    match r2 {
        ToolCallContent::Diff { old_text, new_text, .. } => {
            assert_eq!(old_text, Some("v1".to_string()));
            assert_eq!(new_text, "v2");
        }
        _ => panic!("expected Diff"),
    }

    let r3: Result<ToolCallContent, _> = tool
        .call(json!({"path": "chain.rs", "content": "v3"}), None)
        .await;
    let r3 = r3.expect("third write");
    match r3 {
        ToolCallContent::Diff { old_text, new_text, .. } => {
            assert_eq!(old_text, Some("v2".to_string()));
            assert_eq!(new_text, "v3");
        }
        _ => panic!("expected Diff"),
    }
}
