//! Content part parsing and file I/O for LlmTool.
//!
//! Converts agent-supplied JSON `content[]` arrays into typed
//! [`ContentPart`] values that the upstream HTTP transport can serialize.
//!
//! Standard OpenAI types (`text`, `image_url`, `input_audio`) use OpenAI's
//! nested structure (`{ "type": "image_url", "image_url": { "url": ... } }`)
//! and are parsed manually. anureo extension types (`*_path`, `*_base64`,
//! `*_url` for non-image media) use a flat structure and follow the agent
//! convenience pattern.

use std::path::{Path, PathBuf};

use base64::Engine;
use anureo_llm::ToolSourceError;
use serde_json::Value;
use tool_basic::file::resolve_path_under;

use anureo_llm::message::ContentPart;

// ---------------------------------------------------------------------------
// Media type helpers
// ---------------------------------------------------------------------------

/// Best-effort MIME type inference from a file path's extension.
///
/// Returns `"application/octet-stream"` for unrecognized extensions; callers
/// can still try the request and let the provider decide.
pub(crate) fn infer_media_type(path: &Path) -> Result<String, ToolSourceError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = match ext.as_str() {
        // images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        // audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        // video
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        // documents
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };
    Ok(mime.to_string())
}

/// Convert OpenAI-style audio `format` short name to MIME type.
///
/// `format` is what the agent passes (`"mp3"`, `"wav"`, etc.); we map it to
/// the MIME type that the internal `ContentPart::AudioBase64` stores.
pub(crate) fn format_to_media_type(format: &str) -> String {
    let mime = match format.to_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        other => return format!("audio/{}", other),
    };
    mime.to_string()
}

// ---------------------------------------------------------------------------
// Path safety + file I/O
// ---------------------------------------------------------------------------

/// Resolve a user-supplied relative path against `working_folder`, rejecting
/// paths that escape via `..` or symlinks.
///
/// Reuses `tool_basic::file::path::resolve_path_under`, which canonicalizes
/// the result and validates it lives under the working folder.
fn resolve_file_path(
    path_param: &str,
    working_folder: Option<&Path>,
) -> Result<PathBuf, ToolSourceError> {
    let working_folder = working_folder.ok_or_else(|| {
        ToolSourceError::InvalidInput(
            "working folder not configured, _path content parts unavailable".to_string(),
        )
    })?;
    resolve_path_under(working_folder, path_param)
        .map_err(|e| ToolSourceError::InvalidInput(format!("path resolution failed: {}", e)))
}

/// Read a file and base64-encode its contents, enforcing `max_file_size`.
fn read_and_base64_encode(path: &Path, max_file_size: usize) -> Result<String, ToolSourceError> {
    let bytes = std::fs::read(path).map_err(|e| {
        ToolSourceError::InvalidInput(format!("failed to read {}: {}", path.display(), e))
    })?;
    if bytes.len() > max_file_size {
        return Err(ToolSourceError::InvalidInput(format!(
            "file {} is {} bytes, exceeds max_file_size {}",
            path.display(),
            bytes.len(),
            max_file_size
        )));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

// ---------------------------------------------------------------------------
// content[] → ContentPart
// ---------------------------------------------------------------------------

/// Convert a single agent-supplied content part JSON object to a [`ContentPart`].
///
/// `text`, `image_url`, and `input_audio` are the OpenAI-standard types and
/// use OpenAI's nested structure. `_path` variants read the file from disk
/// and inline it as base64. Other `_url`/`_base64` types use a flat structure
/// since OpenAI has no equivalent standard for video/PDF.
pub(crate) fn resolve_content_part(
    v: &Value,
    working_folder: Option<&Path>,
    max_file_size: usize,
) -> Result<ContentPart, ToolSourceError> {
    let part_type = v
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("content part 缺少 type 字段".into()))?;

    match part_type {
        // ── Standard OpenAI types: nested structure manually parsed ──
        "text" => {
            let text = v
                .get("text")
                .and_then(|t| t.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("text part 缺少 text".into()))?;
            Ok(ContentPart::Text {
                text: text.to_string(),
            })
        }
        "image_url" => {
            let img = v.get("image_url").ok_or_else(|| {
                ToolSourceError::InvalidInput("image_url part 缺少 image_url 对象".into())
            })?;
            let url = img
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("image_url.url 缺失".into()))?;
            let detail = img.get("detail").and_then(|d| d.as_str()).map(String::from);

            // data: URI → ImageBase64; HTTP URL → ImageUrl
            if let Some(rest) = url.strip_prefix("data:") {
                let (header, data) = rest
                    .split_once(',')
                    .ok_or_else(|| ToolSourceError::InvalidInput("data URI 格式错误".into()))?;
                let media_type = header.split(';').next().unwrap_or("image/png");
                Ok(ContentPart::ImageBase64 {
                    media_type: media_type.to_string(),
                    data: data.to_string(),
                })
            } else {
                Ok(ContentPart::ImageUrl {
                    url: url.to_string(),
                    detail,
                })
            }
        }
        "input_audio" => {
            let audio = v.get("input_audio").ok_or_else(|| {
                ToolSourceError::InvalidInput("input_audio part 缺少 input_audio 对象".into())
            })?;
            let data = audio
                .get("data")
                .and_then(|d| d.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("input_audio.data 缺失".into()))?;
            let format = audio
                .get("format")
                .and_then(|f| f.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("input_audio.format 缺失".into()))?;
            let media_type = format_to_media_type(format);
            Ok(ContentPart::AudioBase64 {
                media_type,
                data: data.to_string(),
            })
        }

        // ── anureo extensions: video/pdf flat types pass through ──
        "video_url" | "pdf_url" => from_value(v),
        "video_base64" | "pdf_base64" => from_value(v),

        // ── _path types: read file → base64 → corresponding *Base64 variant ──
        "image_path" => {
            let path_str = v
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("image_path 缺少 path".into()))?;
            let path = resolve_file_path(path_str, working_folder)?;
            let media_type = infer_media_type(&path)?;
            let data = read_and_base64_encode(&path, max_file_size)?;
            Ok(ContentPart::ImageBase64 {
                media_type: media_type.to_string(),
                data,
            })
        }
        "audio_path" => {
            let path_str = v
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("audio_path 缺少 path".into()))?;
            let path = resolve_file_path(path_str, working_folder)?;
            let media_type = infer_media_type(&path)?;
            let data = read_and_base64_encode(&path, max_file_size)?;
            Ok(ContentPart::AudioBase64 { media_type, data })
        }
        "video_path" => {
            let path_str = v
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("video_path 缺少 path".into()))?;
            let path = resolve_file_path(path_str, working_folder)?;
            let media_type = infer_media_type(&path)?;
            let data = read_and_base64_encode(&path, max_file_size)?;
            Ok(ContentPart::VideoBase64 { media_type, data })
        }
        "pdf_path" => {
            let path_str = v
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput("pdf_path 缺少 path".into()))?;
            let path = resolve_file_path(path_str, working_folder)?;
            let data = read_and_base64_encode(&path, max_file_size)?;
            Ok(ContentPart::PdfBase64 { data })
        }

        other => Err(ToolSourceError::InvalidInput(format!(
            "未知 content part type: {}",
            other
        ))),
    }
}

/// Deserialize a `ContentPart` from JSON using its existing serde representation.
fn from_value(v: &Value) -> Result<ContentPart, ToolSourceError> {
    serde_json::from_value(v.clone())
        .map_err(|e| ToolSourceError::InvalidInput(format!("invalid content part: {}", e)))
}

// ---------------------------------------------------------------------------
// content[] → Message
// ---------------------------------------------------------------------------

/// Convert a single agent-supplied message JSON to a [`anureo_llm::Message`].
pub(crate) fn parse_message(
    v: &Value,
    working_folder: Option<&Path>,
    max_file_size: usize,
) -> Result<anureo_llm::Message, ToolSourceError> {
    use anureo_llm::message::{AssistantPayload, Message, UserContent};

    let role = v
        .get("role")
        .and_then(|r| r.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("message 缺少 role".into()))?;

    match role {
        "system" => {
            let content = extract_text_content(v)?;
            Ok(Message::System(content))
        }
        "user" => {
            let content = v
                .get("content")
                .ok_or_else(|| ToolSourceError::InvalidInput("user 消息缺少 content".into()))?;
            let user_content = match content {
                Value::String(s) => UserContent::Text(s.clone()),
                Value::Array(parts) => {
                    let resolved: Result<Vec<_>, _> = parts
                        .iter()
                        .map(|p| resolve_content_part(p, working_folder, max_file_size))
                        .collect();
                    UserContent::Multimodal(resolved?)
                }
                _ => {
                    return Err(ToolSourceError::InvalidInput(
                        "user content 必须是 string 或 array".into(),
                    ))
                }
            };
            Ok(Message::User(user_content))
        }
        "assistant" => {
            let content = extract_text_content(v)?;
            Ok(Message::Assistant(AssistantPayload {
                content,
                reasoning_content: None,
                tool_calls: vec![],
            }))
        }
        other => Err(ToolSourceError::InvalidInput(format!(
            "未知 message role: {}",
            other
        ))),
    }
}

fn extract_text_content(v: &Value) -> Result<String, ToolSourceError> {
    match v.get("content") {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Null) | None => Ok(String::new()),
        _ => Err(ToolSourceError::InvalidInput(
            "content 必须是 string".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        std::env::temp_dir().join("llm-tool-content-tests")
    }

    #[test]
    fn infer_media_type_known_extensions() {
        assert_eq!(
            infer_media_type(&PathBuf::from("a.png")).unwrap(),
            "image/png"
        );
        assert_eq!(
            infer_media_type(&PathBuf::from("a.mp3")).unwrap(),
            "audio/mpeg"
        );
        assert_eq!(
            infer_media_type(&PathBuf::from("a.pdf")).unwrap(),
            "application/pdf"
        );
        assert_eq!(
            infer_media_type(&PathBuf::from("a.mp4")).unwrap(),
            "video/mp4"
        );
    }

    #[test]
    fn infer_media_type_unknown_falls_back() {
        assert_eq!(
            infer_media_type(&PathBuf::from("a.unknownext")).unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            infer_media_type(&PathBuf::from("noext")).unwrap(),
            "application/octet-stream"
        );
    }

    #[test]
    fn format_to_media_type_known() {
        assert_eq!(format_to_media_type("mp3"), "audio/mpeg");
        assert_eq!(format_to_media_type("WAV"), "audio/wav");
        assert_eq!(format_to_media_type("M4A"), "audio/mp4");
        assert_eq!(format_to_media_type("flac"), "audio/flac");
    }

    #[test]
    fn format_to_media_type_unknown_falls_back() {
        assert_eq!(format_to_media_type("opus"), "audio/opus");
    }

    #[test]
    fn resolve_content_part_text() {
        let v = serde_json::json!({"type": "text", "text": "hello"});
        let p = resolve_content_part(&v, None, 1024).unwrap();
        match p {
            ContentPart::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn resolve_content_part_image_url_http() {
        let v = serde_json::json!({
            "type": "image_url",
            "image_url": { "url": "https://example.com/a.png", "detail": "high" }
        });
        let p = resolve_content_part(&v, None, 1024).unwrap();
        match p {
            ContentPart::ImageUrl { url, detail } => {
                assert_eq!(url, "https://example.com/a.png");
                assert_eq!(detail.as_deref(), Some("high"));
            }
            _ => panic!("expected ImageUrl"),
        }
    }

    #[test]
    fn resolve_content_part_image_url_data_uri() {
        let v = serde_json::json!({
            "type": "image_url",
            "image_url": { "url": "data:image/jpeg;base64,XYZ" }
        });
        let p = resolve_content_part(&v, None, 1024).unwrap();
        match p {
            ContentPart::ImageBase64 { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "XYZ");
            }
            _ => panic!("expected ImageBase64"),
        }
    }

    #[test]
    fn resolve_content_part_input_audio() {
        let v = serde_json::json!({
            "type": "input_audio",
            "input_audio": { "data": "ABC", "format": "mp3" }
        });
        let p = resolve_content_part(&v, None, 1024).unwrap();
        match p {
            ContentPart::AudioBase64 { media_type, data } => {
                assert_eq!(media_type, "audio/mpeg");
                assert_eq!(data, "ABC");
            }
            _ => panic!("expected AudioBase64"),
        }
    }

    #[test]
    fn resolve_content_part_unknown_type_errors() {
        let v = serde_json::json!({"type": "wat"});
        let err = resolve_content_part(&v, None, 1024).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("未知 content part type"), "msg = {}", msg);
    }

    #[test]
    fn resolve_content_part_missing_type_errors() {
        let v = serde_json::json!({"text": "no type"});
        let err = resolve_content_part(&v, None, 1024).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("缺少 type"), "msg = {}", msg);
    }

    #[test]
    fn parse_message_user_text() {
        let v = serde_json::json!({"role": "user", "content": "hi"});
        let m = parse_message(&v, None, 1024).unwrap();
        match m {
            anureo_llm::Message::User(anureo_llm::UserContent::Text(s)) => assert_eq!(s, "hi"),
            _ => panic!("expected User Text"),
        }
    }

    #[test]
    fn parse_message_user_multimodal() {
        let v = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "https://x/a.png"}}
            ]
        });
        let m = parse_message(&v, None, 1024).unwrap();
        match m {
            anureo_llm::Message::User(anureo_llm::UserContent::Multimodal(parts)) => {
                assert_eq!(parts.len(), 2);
            }
            _ => panic!("expected User Multimodal"),
        }
    }

    #[test]
    fn parse_message_system() {
        let v = serde_json::json!({"role": "system", "content": "you are helpful"});
        let m = parse_message(&v, None, 1024).unwrap();
        match m {
            anureo_llm::Message::System(s) => assert_eq!(s, "you are helpful"),
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn parse_message_unknown_role_errors() {
        let v = serde_json::json!({"role": "tool", "content": "x"});
        let err = parse_message(&v, None, 1024).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("未知 message role"), "msg = {}", msg);
    }

    // Keep workspace helper alive for future path-based tests.
    #[allow(dead_code)]
    fn _ensure_workspace() -> PathBuf {
        let _ = workspace();
        workspace()
    }
}
