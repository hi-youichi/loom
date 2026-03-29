//! Parse ACP ContentBlock list into a single user message
//!
//! ACP `session/prompt` carries `content_blocks: Vec<ContentBlock>`, where each block can be
//! Text, ResourceLink, Image, Audio, Resource, etc. This module merges such a list into
//! a **single user message string** for Loom. Protocol and type support are described in [`crate::protocol`].
//!
//! ## ContentBlock types and Loom support
//!
//! | Variant | Description | Loom support |
//! |---------|-------------|--------------|
//! | **Text** | Plain text or Markdown | **Required**: concatenate in order with `\n\n` between blocks. |
//! | **ResourceLink** | Reference (URI) to a resource the agent can fetch | **Required**: e.g. "Reference: …" or append context; if Loom cannot fetch, put URI in message only. |
//! | **Resource** | Embedded resource (full content in message) | **Supported** (requires `embeddedContext` capability): text resources formatted with metadata; binary resources skipped. |
//! | **Image** | Image content | Optional; needs promptCapabilities.image. If not declared, UnsupportedBlock. |
//! | **Audio** | Audio | Same; promptCapabilities.audio. |
//!
//! ## Implementation notes
//!
//! - Iterate content_blocks: Text -> take text and concatenate in order; ResourceLink -> append URI/description to message.
//! - If Image/Audio/Resource appear and were not declared in capabilities, return [`ContentError::UnsupportedBlock`] or ignore and log.
//! - Result is a single user message string assigned to `RunOptions::message`.
//! - **Empty list**: may return `Ok(String::new())` or [`ContentError::EmptyMessage`] (invalid params) depending on policy.

use serde::{Deserialize, Serialize};

/// Parse a slice of content blocks into a single user message string.
///
/// Only blocks implementing [`ContentBlockLike`] are processed; typically ACP's `ContentBlock::Text` implements it,
/// other types may implement it or return `None` to mean "skip".
///
/// # Arguments
///
/// - `blocks`: Slice of content blocks; order preserved; adjacent Text blocks joined with `\n\n`.
///
/// # Returns
///
/// - `Ok(s)`: Concatenated message string; may be empty.
/// - `Err(ContentError::UnsupportedBlock)`: A block type is not supported and cannot be skipped.
/// - `Err(ContentError::EmptyMessage)`: Policy requires at least one block and blocks is empty.
///
/// # Example
///
/// ```
/// use loom_acp::content::{content_blocks_to_message, ContentBlockLike, ContentError};
///
/// struct TextBlock(pub String);
/// impl ContentBlockLike for TextBlock {
///     fn as_text(&self) -> Option<String> { Some(self.0.clone()) }
/// }
///
/// let blocks = [TextBlock("Hello".into()), TextBlock("world".into())];
/// let msg = content_blocks_to_message(&blocks).unwrap();
/// assert_eq!(msg, "Hello\n\nworld");
/// ```
pub fn content_blocks_to_message<B>(blocks: &[B]) -> Result<String, ContentError>
where
    B: ContentBlockLike,
{
    let mut parts = Vec::new();
    for block in blocks {
        if let Some(text) = block.as_text() {
            parts.push(text);
        }
        if block.is_unsupported() {
            return Err(ContentError::UnsupportedBlock);
        }
    }
    Ok(parts.join("\n\n"))
}

/// Trait for content blocks that can be interpreted as a "text fragment".
///
/// ACP's `ContentBlock` can implement this trait when using the `agent-client-protocol` crate;
/// for tests or placeholders a simple struct (e.g. with only `text: String`) can implement it.
pub trait ContentBlockLike {
    /// If this block is plain text (or can be extracted as text), return the text; otherwise `None` (skip).
    fn as_text(&self) -> Option<String>;

    /// If this block is a type not supported by current capabilities and cannot be ignored, return `true`; parsing will return `ContentError::UnsupportedBlock`. Default is `false`.
    fn is_unsupported(&self) -> bool {
        false
    }
}

impl ContentBlockLike for str {
    fn as_text(&self) -> Option<String> {
        Some(self.to_string())
    }
}

impl ContentBlockLike for String {
    fn as_text(&self) -> Option<String> {
        Some(self.clone())
    }
}

/// Adapter for ACP ContentBlock: Text and Resource are extracted; ResourceLink skipped; Image/Audio treated as unsupported.
impl ContentBlockLike for agent_client_protocol::ContentBlock {
    fn as_text(&self) -> Option<String> {
        match self {
            agent_client_protocol::ContentBlock::Text(t) => Some(t.text.clone()),
            agent_client_protocol::ContentBlock::Resource(r) => {
                use agent_client_protocol::EmbeddedResourceResource;
                
                match &r.resource {
                    EmbeddedResourceResource::TextResourceContents(text_res) => {
                        let mime = text_res.mime_type.as_deref().unwrap_or("text/plain");
                        let uri = &text_res.uri;
                        let text = &text_res.text;
                        
                        Some(format!(
                            "--- Embedded Resource ---\nURI: {}\nMIME: {}\n\n{}\n--- End Resource ---",
                            uri, mime, text
                        ))
                    }
                    EmbeddedResourceResource::BlobResourceContents(blob_res) => {
                        tracing::debug!(
                            uri = %blob_res.uri,
                            mime = ?blob_res.mime_type,
                            "Skipping binary embedded resource"
                        );
                        None
                    }
                    _ => {
                        tracing::debug!("Unknown embedded resource type, skipping");
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn is_unsupported(&self) -> bool {
        matches!(
            self,
            agent_client_protocol::ContentBlock::Image(_)
                | agent_client_protocol::ContentBlock::Audio(_)
        )
    }
}

/// Content block parse or validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// Unsupported content block type (e.g. Image/Audio/Resource when capability not declared).
    #[error("unsupported content block type")]
    UnsupportedBlock,

    /// Content block list is empty and policy requires a non-empty message.
    #[error("empty message")]
    EmptyMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { 
        url: String, 
        #[serde(skip_serializing_if = "Option::is_none")] 
        mime_type: Option<String> 
    },
    Resource {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")] 
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")] 
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")] 
        blob: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCallContent {
    Content { content: ContentBlock },
    Diff {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")] 
        old_text: Option<String>,
        new_text: String,
    },
    Terminal { terminal_id: String },
}

impl ToolCallContent {
    pub fn from_text(text: String) -> Self {
        ToolCallContent::Content {
            content: ContentBlock::Text { text },
        }
    }
    
    pub fn from_diff(path: String, old_text: Option<String>, new_text: String) -> Self {
        ToolCallContent::Diff { path, old_text, new_text }
    }
    
    pub fn from_terminal(terminal_id: String) -> Self {
        ToolCallContent::Terminal { terminal_id }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallLocation {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

pub fn extract_locations(tool_name: &str, args: &serde_json::Value) -> Vec<ToolCallLocation> {
    match tool_name {
        "read" | "write_file" | "edit" | "delete_file" | "move_file" | "glob" | "grep" => {
            let mut locations = Vec::new();
            
            if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                locations.push(ToolCallLocation {
                    path: path.to_string(),
                    line: args.get("line").and_then(|l| l.as_u64()).map(|l| l as u32),
                });
            }
            
            if tool_name == "move_file" {
                if let Some(source) = args.get("source").and_then(|s| s.as_str()) {
                    locations.push(ToolCallLocation {
                        path: source.to_string(),
                        line: None,
                    });
                }
                if let Some(target) = args.get("target").and_then(|t| t.as_str()) {
                    locations.push(ToolCallLocation {
                        path: target.to_string(),
                        line: None,
                    });
                }
            }
            
            if tool_name == "grep" {
                if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                    locations.push(ToolCallLocation {
                        path: path.to_string(),
                        line: None,
                    });
                }
            }
            
            locations
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::{
        BlobResourceContents, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
        TextContent, TextResourceContents,
    };

    #[test]
    fn test_text_content_block() {
        let block = ContentBlock::Text(TextContent::new("Hello, world!"));
        let result = block.as_text();
        assert_eq!(result, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_text_resource_parsing() {
        let text_res = TextResourceContents::new("Hello, world!", "file:///test.txt")
            .mime_type(Some("text/plain".to_string()));
        let embedded = EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(
            text_res,
        ));
        let block = ContentBlock::Resource(embedded);

        let result = block.as_text().expect("Should extract text from resource");
        assert!(result.contains("Hello, world!"));
        assert!(result.contains("file:///test.txt"));
        assert!(result.contains("text/plain"));
        assert!(result.contains("--- Embedded Resource ---"));
        assert!(result.contains("--- End Resource ---"));
    }

    #[test]
    fn test_text_resource_without_mime() {
        let text_res = TextResourceContents::new("Content", "file:///example.txt");
        let embedded = EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(
            text_res,
        ));
        let block = ContentBlock::Resource(embedded);

        let result = block.as_text().expect("Should extract text");
        assert!(result.contains("Content"));
        assert!(result.contains("text/plain")); // default MIME
    }

    #[test]
    fn test_blob_resource_skipped() {
        let blob_res = BlobResourceContents::new("SGVsbG8=", "file:///binary.bin")
            .mime_type(Some("application/octet-stream".to_string()));
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::BlobResourceContents(blob_res));
        let block = ContentBlock::Resource(embedded);

        let result = block.as_text();
        assert!(
            result.is_none(),
            "Binary resources should be skipped and return None"
        );
    }

    #[test]
    fn test_resource_not_unsupported() {
        let text_res = TextResourceContents::new("Test", "file:///test.txt");
        let embedded = EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(
            text_res,
        ));
        let block = ContentBlock::Resource(embedded);

        assert!(
            !block.is_unsupported(),
            "Resource should not be marked as unsupported"
        );
    }

    #[test]
    fn test_image_still_unsupported() {
        use agent_client_protocol::ImageContent;
        let block = ContentBlock::Image(ImageContent::new("data", "image/png"));
        assert!(
            block.is_unsupported(),
            "Image should still be marked as unsupported"
        );
    }

    #[test]
    fn test_audio_still_unsupported() {
        use agent_client_protocol::AudioContent;
        let block = ContentBlock::Audio(AudioContent::new("data", "audio/mp3"));
        assert!(
            block.is_unsupported(),
            "Audio should still be marked as unsupported"
        );
    }

    #[test]
    fn test_content_blocks_to_message_with_resource() {
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Start")),
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(
                    TextResourceContents::new("Embedded content", "file:///test.txt"),
                ),
            )),
            ContentBlock::Text(TextContent::new("End")),
        ];

        let result = content_blocks_to_message(&blocks).expect("Should merge blocks");
        assert!(result.contains("Start"));
        assert!(result.contains("Embedded content"));
        assert!(result.contains("End"));
        assert!(result.contains("--- Embedded Resource ---"));
    }

    #[test]
    fn test_empty_blocks() {
        let blocks: Vec<ContentBlock> = vec![];
        let result = content_blocks_to_message(&blocks);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }
}
