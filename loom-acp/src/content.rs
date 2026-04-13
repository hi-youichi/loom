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

// Re-export from loom for compatibility
pub use loom::message::{ContentPart, UserContent};

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

/// Adapter for ACP ContentBlock: Text, Resource, and ResourceLink are extracted; Image/Audio unsupported in pure-text mode.
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
                        let mime = blob_res
                            .mime_type
                            .as_deref()
                            .unwrap_or("application/octet-stream");
                        Some(format!(
                            "--- Binary Resource ---\nURI: {}\nMIME: {}\nSize: {} bytes\n--- End Resource ---",
                            blob_res.uri,
                            mime,
                            blob_res.blob.len()
                        ))
                    }
                    _ => {
                        tracing::debug!("Unknown embedded resource type, skipping");
                        None
                    }
                }
            }
            agent_client_protocol::ContentBlock::ResourceLink(rl) => {
                let mut parts = vec![format!("Reference: {} ({})", rl.name, rl.uri)];
                if let Some(desc) = &rl.description {
                    parts.push(format!("Description: {}", desc));
                }
                if let Some(mime) = &rl.mime_type {
                    parts.push(format!("MIME: {}", mime));
                }
                Some(parts.join("\n"))
            }
            _ => None,
        }
    }

    fn is_unsupported(&self) -> bool {
        false
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
    Text {
        text: String,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        uri: Option<String>,
    },
    Audio {
        data: String,
        mime_type: String,
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
    ResourceLink {
        uri: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCallContent {
    Content {
        content: ContentBlock,
    },
    Diff {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        old_text: Option<String>,
        new_text: String,
    },
    Terminal {
        terminal_id: String,
    },
}

impl ToolCallContent {
    pub fn from_text(text: String) -> Self {
        ToolCallContent::Content {
            content: ContentBlock::Text { text },
        }
    }

    pub fn from_diff(path: String, old_text: Option<String>, new_text: String) -> Self {
        ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        }
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

/// Convert ACP ContentBlock list to UserContent, supporting multimodal input.
///
/// This replaces the old `content_blocks_to_message` for cases where the caller
/// needs the full multimodal structure (images, audio) rather than a flattened string.
///
/// # Arguments
///
/// * `blocks` - A slice of ACP ContentBlock items
///
/// # Returns
///
/// * `Ok(UserContent)` - The converted content, either Text or Multimodal
/// * `Err(ContentError::EmptyMessage)` - If blocks is empty or contains no usable content
pub fn content_blocks_to_user_content(
    blocks: &[agent_client_protocol::ContentBlock],
) -> Result<UserContent, ContentError> {
    if blocks.is_empty() {
        return Err(ContentError::EmptyMessage);
    }

    let mut parts: Vec<ContentPart> = Vec::new();

    for block in blocks {
        match block {
            agent_client_protocol::ContentBlock::Text(t) => {
                parts.push(ContentPart::Text {
                    text: t.text.clone(),
                });
            }

            agent_client_protocol::ContentBlock::Image(img) => {
                if !img.data.is_empty() {
                    parts.push(ContentPart::ImageBase64 {
                        media_type: img.mime_type.clone(),
                        data: img.data.clone(),
                    });
                } else if let Some(uri) = &img.uri {
                    parts.push(ContentPart::ImageUrl {
                        url: uri.clone(),
                        detail: None,
                    });
                } else {
                    tracing::warn!(
                        mime_type = %img.mime_type,
                        "ACP Image with empty data and no URI, skipping"
                    );
                }
            }

            agent_client_protocol::ContentBlock::Audio(audio) => {
                if audio.data.is_empty() {
                    tracing::warn!(
                        mime_type = %audio.mime_type,
                        "ACP Audio with empty data, skipping"
                    );
                    continue;
                }
                parts.push(ContentPart::AudioBase64 {
                    media_type: audio.mime_type.clone(),
                    data: audio.data.clone(),
                });
            }

            agent_client_protocol::ContentBlock::Resource(r) => {
                use agent_client_protocol::EmbeddedResourceResource;
                match &r.resource {
                    EmbeddedResourceResource::TextResourceContents(text_res) => {
                        parts.push(ContentPart::Text {
                            text: format!(
                                "--- Embedded Resource ---\nURI: {}\nMIME: {}\n\n{}\n--- End Resource ---",
                                text_res.uri,
                                text_res.mime_type.as_deref().unwrap_or("text/plain"),
                                text_res.text
                            ),
                        });
                    }
                    EmbeddedResourceResource::BlobResourceContents(blob_res) => {
                        let mime = blob_res.mime_type.as_deref().unwrap_or("");
                        if mime.starts_with("image/") {
                            parts.push(ContentPart::ImageBase64 {
                                media_type: mime.to_string(),
                                data: blob_res.blob.clone(),
                            });
                        } else if mime.starts_with("audio/") {
                            parts.push(ContentPart::AudioBase64 {
                                media_type: mime.to_string(),
                                data: blob_res.blob.clone(),
                            });
                        } else {
                            let mime = blob_res
                                .mime_type
                                .as_deref()
                                .unwrap_or("application/octet-stream");
                            parts.push(ContentPart::Text {
                                text: format!(
                                    "--- Binary Resource ---\nURI: {}\nMIME: {}\nSize: {} bytes\n--- End Resource ---",
                                    blob_res.uri,
                                    mime,
                                    blob_res.blob.len()
                                ),
                            });
                        }
                    }
                    _ => {
                        tracing::debug!("Unknown embedded resource type, skipping");
                    }
                }
            }

            agent_client_protocol::ContentBlock::ResourceLink(rl) => {
                let mut text = format!("Reference: {} ({})", rl.name, rl.uri);
                if let Some(desc) = &rl.description {
                    text.push_str(&format!("\nDescription: {}", desc));
                }
                if let Some(mime) = &rl.mime_type {
                    text.push_str(&format!("\nMIME: {}", mime));
                }
                parts.push(ContentPart::Text { text });
            }
            _ => {
                tracing::debug!("Unhandled ContentBlock type, skipping");
            }
        }
    }

    if parts.is_empty() {
        return Err(ContentError::EmptyMessage);
    }

    // 纯文本快捷路径：如果所有部分都是文本，合并为单个文本
    if parts.iter().all(|p| matches!(p, ContentPart::Text { .. })) {
        let combined = parts
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.as_str(),
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return Ok(UserContent::Text(combined));
    }

    Ok(UserContent::Multimodal(parts))
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
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(text_res));
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
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(text_res));
        let block = ContentBlock::Resource(embedded);

        let result = block.as_text().expect("Should extract text");
        assert!(result.contains("Content"));
        assert!(result.contains("text/plain")); // default MIME
    }

    #[test]
    fn test_blob_resource_returns_reference() {
        let blob_res = BlobResourceContents::new("SGVsbG8=", "file:///binary.bin")
            .mime_type(Some("application/octet-stream".to_string()));
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::BlobResourceContents(blob_res));
        let block = ContentBlock::Resource(embedded);

        let result = block
            .as_text()
            .expect("Should return text reference for blob");
        assert!(result.contains("Binary Resource"));
        assert!(result.contains("file:///binary.bin"));
        assert!(result.contains("application/octet-stream"));
    }

    #[test]
    fn test_resource_not_unsupported() {
        let text_res = TextResourceContents::new("Test", "file:///test.txt");
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(text_res));
        let block = ContentBlock::Resource(embedded);

        assert!(
            !block.is_unsupported(),
            "Resource should not be marked as unsupported"
        );
    }

    #[test]
    fn test_image_no_longer_unsupported() {
        use agent_client_protocol::ImageContent;
        let block = ContentBlock::Image(ImageContent::new("data", "image/png"));
        assert!(
            !block.is_unsupported(),
            "Image should no longer be marked as unsupported"
        );
    }

    #[test]
    fn test_audio_no_longer_unsupported() {
        use agent_client_protocol::AudioContent;
        let block = ContentBlock::Audio(AudioContent::new("data", "audio/mp3"));
        assert!(
            !block.is_unsupported(),
            "Audio should no longer be marked as unsupported"
        );
    }

    #[test]
    fn test_content_blocks_to_user_converted() {
        use agent_client_protocol::{AudioContent, ImageContent};

        // 文本 + 图片 + 音频
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Look at this")),
            ContentBlock::Image(ImageContent::new("base64data", "image/png")),
            ContentBlock::Audio(AudioContent::new("audiodata", "audio/mp3")),
        ];

        let result = content_blocks_to_user_content(&blocks);
        assert!(result.is_ok());

        let UserContent::Multimodal(parts) = result.unwrap() else {
            panic!("Expected Multimodal content");
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[0], ContentPart::Text { .. }));
        assert!(matches!(parts[1], ContentPart::ImageBase64 { .. }));
        assert!(matches!(parts[2], ContentPart::AudioBase64 { .. }));
    }

    #[test]
    fn test_content_blocks_to_user_text_only() {
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Hello")),
            ContentBlock::Text(TextContent::new("World")),
        ];

        let result = content_blocks_to_user_content(&blocks);
        assert!(result.is_ok());

        // 纯文本应该合并为 UserContent::Text
        let UserContent::Text(text) = result.unwrap() else {
            panic!("Expected Text content");
        };
        assert_eq!(text, "Hello\n\nWorld");
    }

    #[test]
    fn test_content_blocks_to_user_empty() {
        let blocks: Vec<ContentBlock> = vec![];
        let result = content_blocks_to_user_content(&blocks);
        assert!(matches!(result, Err(ContentError::EmptyMessage)));
    }

    #[test]
    fn test_content_blocks_to_message_with_resource() {
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Start")),
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
                    "Embedded content",
                    "file:///test.txt",
                )),
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

    #[test]
    fn test_resource_link_as_text() {
        use agent_client_protocol::ResourceLink;
        let rl = ResourceLink::new("document.pdf", "file:///home/user/document.pdf")
            .mime_type(Some("application/pdf".to_string()))
            .description(Some("Important document".to_string()));
        let block = ContentBlock::ResourceLink(rl);
        let result = block
            .as_text()
            .expect("Should extract text from ResourceLink");
        assert!(result.contains("Reference: document.pdf"));
        assert!(result.contains("file:///home/user/document.pdf"));
        assert!(result.contains("application/pdf"));
        assert!(result.contains("Important document"));
    }

    #[test]
    fn test_resource_link_in_content_blocks_to_user_content() {
        use agent_client_protocol::ResourceLink;
        let rl = ResourceLink::new("readme.md", "file:///project/README.md");
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Check this file")),
            ContentBlock::ResourceLink(rl),
        ];
        let result = content_blocks_to_user_content(&blocks).expect("Should convert");
        let text = result.as_text();
        assert!(text.contains("Check this file"));
        assert!(text.contains("Reference: readme.md"));
        assert!(text.contains("file:///project/README.md"));
    }

    #[test]
    fn test_blob_resource_in_content_blocks_to_user_content() {
        let blob_res = BlobResourceContents::new("SGVsbG8=", "file:///data.bin")
            .mime_type(Some("application/octet-stream".to_string()));
        let embedded =
            EmbeddedResource::new(EmbeddedResourceResource::BlobResourceContents(blob_res));
        let blocks = vec![ContentBlock::Resource(embedded)];
        let result = content_blocks_to_user_content(&blocks).expect("Should convert");
        let text = result.as_text();
        assert!(text.contains("Binary Resource"));
        assert!(text.contains("file:///data.bin"));
    }
}
