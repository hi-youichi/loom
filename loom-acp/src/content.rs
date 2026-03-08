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
//! | **Image** | Image content | Optional; needs promptCapabilities.image. If not declared, ignore or UnsupportedBlock. |
//! | **Audio** | Audio | Same; promptCapabilities.audio. |
//! | **Resource** | Embedded resource (full content in message) | Optional; needs embeddedContext. If not declared, skip. |
//!
//! ## Implementation notes
//!
//! - Iterate content_blocks: Text -> take text and concatenate in order; ResourceLink -> append URI/description to message.
//! - If Image/Audio/Resource appear and were not declared in capabilities, return [`ContentError::UnsupportedBlock`] or ignore and log.
//! - Result is a single user message string assigned to `RunOptions::message`.
//! - **Empty list**: may return `Ok(String::new())` or [`ContentError::EmptyMessage`] (invalid params) depending on policy.


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
///     fn as_text(&self) -> Option<&str> { Some(self.0.as_str()) }
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
            parts.push(text.to_string());
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
    fn as_text(&self) -> Option<&str>;

    /// If this block is a type not supported by current capabilities and cannot be ignored, return `true`; parsing will return `ContentError::UnsupportedBlock`. Default is `false`.
    fn is_unsupported(&self) -> bool {
        false
    }
}

impl ContentBlockLike for str {
    fn as_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl ContentBlockLike for String {
    fn as_text(&self) -> Option<&str> {
        Some(self.as_str())
    }
}

/// Adapter for ACP ContentBlock: only Text is extracted; ResourceLink skipped; Image/Audio/Resource treated as unsupported.
impl ContentBlockLike for agent_client_protocol::ContentBlock {
    fn as_text(&self) -> Option<&str> {
        match self {
            agent_client_protocol::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        }
    }

    fn is_unsupported(&self) -> bool {
        matches!(
            self,
            agent_client_protocol::ContentBlock::Image(_)
                | agent_client_protocol::ContentBlock::Audio(_)
                | agent_client_protocol::ContentBlock::Resource(_)
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
