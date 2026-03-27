#[cfg(test)]
mod tests {
    use loom_acp::content::{ToolCallContent, ContentBlock};
    
    #[test]
    fn test_text_content() {
        let content = ToolCallContent::from_text("Hello, World!".to_string());
        
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"content\""));
        assert!(json.contains("Hello, World"));
    }
    
    #[test]
    fn test_diff_content() {
        let content = ToolCallContent::from_diff(
            "test.txt".to_string(),
            Some("old text".to_string()),
            "new text".to_string(),
        );
        
        let json = serde_json::to_string(&content).unwrap();
        
        assert!(json.contains("\"type\":\"diff\""));
        assert!(json.contains("test.txt"));
        assert!(json.contains("old text"));
        assert!(json.contains("new text"));
    }
    
    #[test]
    fn test_terminal_content() {
        let content = ToolCallContent::from_terminal("term-123".to_string());
        
        let json = serde_json::to_string(&content).unwrap();
        
        assert!(json.contains("\"type\":\"terminal\""));
        assert!(json.contains("term-123"));
    }
    
    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::Text {
            text: "Test content".to_string(),
        };
        
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("Test content"));
    }
    
    #[test]
    fn test_content_block_resource() {
        let block = ContentBlock::Resource {
            uri: "file:///path/to/file.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: Some("File content".to_string()),
            blob: None,
        };
        
        let json = serde_json::to_string(&block).unwrap();
        
        assert!(json.contains("\"type\":\"resource\""));
        assert!(json.contains("file:///path/to/file.txt"));
        assert!(json.contains("text/plain"));
    }
    
    #[test]
    fn test_content_deserialization() {
        let json = r#"{"type":"content","content":{"type":"text","text":"Test content"}}"#;
        let content: ToolCallContent = serde_json::from_str(json).unwrap();
        
        match content {
            ToolCallContent::Content { content } => {
                match content {
                    ContentBlock::Text { text } => assert_eq!(text, "Test content"),
                    _ => panic!("Expected Text block"),
                }
            }
            _ => panic!("Expected Content type"),
        }
    }
}
