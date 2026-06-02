//! Message types re-exported from loom-llm.
//!
//! This file is kept for backward compatibility.
//! All types are now defined in the `loom-llm` crate.

// Re-export all message types from loom-llm
pub use loom_llm::message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantPayload, AssistantToolCall, ToolCallContent,
    assistant_content_for_chat_api,
};

/// Convert a `ContentPart` to a `ModalityType` for model validation.
/// This is a loom-specific helper that depends on `model_spec_core`.
pub fn content_part_modality(part: &ContentPart) -> model_spec_core::spec::ModalityType {
    match part {
        ContentPart::Text { .. } | ContentPart::File { .. } => model_spec_core::spec::ModalityType::Text,
        ContentPart::ImageUrl { .. } | ContentPart::ImageBase64 { .. } => model_spec_core::spec::ModalityType::Image,
        ContentPart::AudioBase64 { .. } => model_spec_core::spec::ModalityType::Audio,
        ContentPart::VideoUrl { .. } | ContentPart::VideoBase64 { .. } => model_spec_core::spec::ModalityType::Video,
        ContentPart::PdfUrl { .. } | ContentPart::PdfBase64 { .. } => model_spec_core::spec::ModalityType::Pdf,
    }
}
