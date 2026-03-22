//! Message handlers with dptree dispatching
//!
//! This module provides flexible message handling using teloxide's dptree system.

use crate::config::Settings;
use loom::{
    run_agent_with_options, RunOptions, RunCmd, RunCompletion, AnyStreamEvent,
};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use teloxide::prelude::*;
use teloxide::types::{Message, MessageKind, PhotoSize, Document, Video, ReplyParameters, MessageId};
use teloxide::net::Download;
use tokio::fs;
use serde::{Deserialize, Serialize};

/// File type enum for metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileType {
    Photo,
    Document,
    Video,
    Audio,
    Other,
}

/// Metadata for downloaded files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Telegram chat ID
    pub chat_id: i64,
    /// Telegram message ID
    pub message_id: i32,
    /// Telegram file ID
    pub file_id: String,
    /// Telegram file unique ID (for deduplication)
    pub file_unique_id: String,
    /// Type of file
    pub file_type: FileType,
    /// Original filename (if available)
    pub original_name: Option<String>,
    /// MIME type
    pub mime_type: Option<String>,
    /// File size in bytes
    pub file_size: Option<u64>,
    /// Sender's user ID
    pub user_id: Option<i64>,
    /// Download timestamp (ISO 8601)
    pub downloaded_at: String,
}

/// Download configuration
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// Directory to save downloaded files
    pub dir: PathBuf,
    /// Save metadata file alongside downloaded file
    pub save_metadata: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("downloads"),
            save_metadata: false,
        }
    }
}

impl DownloadConfig {
    /// Create new download config
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ..Default::default()
        }
    }
    
    /// Generate file path: downloads/{chat_id}/{message_id}_{file_id}.{ext}
    pub fn get_file_path(&self, chat_id: i64, message_id: i32, file_id: &str, ext: &str) -> PathBuf {
        let truncated_id = if file_id.len() > 24 { &file_id[..24] } else { file_id };
        let filename = format!("{}_{}.{}", message_id, truncated_id, ext);
        
        let mut path = self.dir.clone();
        path.push(format!("{}", chat_id));
        path.push(&filename);
        path
    }
    
    /// Get metadata file path for a given file path
    pub fn get_metadata_path(&self, file_path: &Path) -> PathBuf {
        file_path.with_extension("json")
    }
    
    /// Initialize download directory
    pub async fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }
}

/// Extract file extension from filename or MIME type
fn get_file_extension(filename: Option<&str>, mime_type: Option<&str>) -> String {
    // Try to get extension from filename first
    if let Some(name) = filename {
        if let Some(dot_pos) = name.rfind('.') {
            let ext = &name[dot_pos + 1..];
            if !ext.is_empty() && ext.len() <= 10 {
                return ext.to_lowercase();
            }
        }
    }
    
    // Fallback to MIME type
    if let Some(mime) = mime_type {
        match mime {
            "image/jpeg" | "image/jpg" => return "jpg".to_string(),
            "image/png" => return "png".to_string(),
            "image/gif" => return "gif".to_string(),
            "image/webp" => return "webp".to_string(),
            "video/mp4" => return "mp4".to_string(),
            "video/webm" => return "webm".to_string(),
            "audio/mpeg" | "audio/mp3" => return "mp3".to_string(),
            "audio/ogg" => return "ogg".to_string(),
            "application/pdf" => return "pdf".to_string(),
            "application/zip" => return "zip".to_string(),
            _ => {}
        }
    }
    
    // Default fallback
    "bin".to_string()
}

/// Save metadata to JSON file
async fn save_metadata(path: &Path, metadata: &FileMetadata) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(metadata)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json).await
}

/// Download a file from Telegram by file_id
/// 
/// # Arguments
/// * `bot` - Telegram bot instance
/// * `file_id` - Telegram file ID
/// * `path` - Full local path to save the file
/// 
/// # Returns
/// Path to the downloaded file
pub async fn download_file(
    bot: &Bot, 
    file_id: &str, 
    path: &Path
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    // Get file info from Telegram
    let file = bot.get_file(file_id).await?;
    
    // Create destination file
    let mut dst = fs::File::create(path).await?;
    
    // Download file
    bot.download_file(&file.path, &mut dst).await?;
    
    tracing::info!("Downloaded file to: {:?}", path);
    
    Ok(path.to_path_buf())
}

/// Download a photo (automatically selects the largest size)
/// 
/// # Arguments
/// * `bot` - Telegram bot instance
/// * `photos` - Photo sizes array (Telegram sends multiple sizes)
/// * `config` - Download configuration
/// * `chat_id` - Chat ID
/// * `message_id` - Message ID
pub async fn download_photo(
    bot: &Bot,
    photos: &[PhotoSize],
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), Box<dyn std::error::Error + Send + Sync>> {
    let largest = photos.last().ok_or("No photo sizes available")?;
    let file_id = &largest.file.id;
    let file_unique_id = &largest.file.unique_id;
    
    let path = config.get_file_path(chat_id, message_id, file_id, "jpg");
    
    download_file(bot, file_id, &path).await?;
    
    let metadata = FileMetadata {
        chat_id,
        message_id,
        file_id: file_id.clone(),
        file_unique_id: file_unique_id.clone(),
        file_type: FileType::Photo,
        mime_type: Some("image/jpeg".to_string()),
        file_size: Some(largest.file.size as u64),
        original_name: None,
        user_id: None,
        downloaded_at: chrono::Utc::now().to_rfc3339(),
    };
    
    if config.save_metadata {
        let meta_path = config.get_metadata_path(&path);
        if let Err(e) = save_metadata(&meta_path, &metadata).await {
            tracing::warn!("Failed to save metadata: {}", e);
        }
    }
    
    Ok((path, metadata))
}

/// Download a document
/// 
/// # Arguments
/// * `bot` - Telegram bot instance
/// * `doc` - Document object
/// * `config` - Download configuration
/// * `chat_id` - Chat ID
/// * `message_id` - Message ID
pub async fn download_document(
    bot: &Bot,
    doc: &Document,
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), Box<dyn std::error::Error + Send + Sync>> {
    let file_id = &doc.file.id;
    let file_unique_id = &doc.file.unique_id;
    
    let mime_str = doc.mime_type.as_ref().map(|m| m.to_string());
    let ext = get_file_extension(doc.file_name.as_deref(), mime_str.as_deref());
    
    let path = config.get_file_path(chat_id, message_id, file_id, &ext);
    
    download_file(bot, file_id, &path).await?;
    
    let metadata = FileMetadata {
        chat_id,
        message_id,
        file_id: file_id.clone(),
        file_unique_id: file_unique_id.clone(),
        file_type: FileType::Document,
        mime_type: doc.mime_type.as_ref().map(|m| m.to_string()),
        file_size: Some(doc.file.size as u64),
        original_name: doc.file_name.clone(),
        user_id: None,
        downloaded_at: chrono::Utc::now().to_rfc3339(),
    };
    
    if config.save_metadata {
        let meta_path = config.get_metadata_path(&path);
        if let Err(e) = save_metadata(&meta_path, &metadata).await {
            tracing::warn!("Failed to save metadata: {}", e);
        }
    }
    
    Ok((path, metadata))
}

/// Download a video
/// 
/// # Arguments
/// * `bot` - Telegram bot instance
/// * `video` - Video object
/// * `config` - Download configuration
/// * `chat_id` - Chat ID
/// * `message_id` - Message ID
pub async fn download_video(
    bot: &Bot,
    video: &Video,
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), Box<dyn std::error::Error + Send + Sync>> {
    let file_id = &video.file.id;
    let file_unique_id = &video.file.unique_id;
    
    let mime_str = video.mime_type.as_ref().map(|m| m.to_string());
    let ext = get_file_extension(None, mime_str.as_deref());
    let ext = if ext == "bin" { "mp4" } else { &ext };
    
    let path = config.get_file_path(chat_id, message_id, file_id, ext);
    
    download_file(bot, file_id, &path).await?;
    
    let metadata = FileMetadata {
        chat_id,
        message_id,
        file_id: file_id.clone(),
        file_unique_id: file_unique_id.clone(),
        file_type: FileType::Video,
        mime_type: video.mime_type.as_ref().map(|m| m.to_string()),
        file_size: Some(video.file.size as u64),
        original_name: None,
        user_id: None,
        downloaded_at: chrono::Utc::now().to_rfc3339(),
    };
    
    if config.save_metadata {
        let meta_path = config.get_metadata_path(&path);
        if let Err(e) = save_metadata(&meta_path, &metadata).await {
            tracing::warn!("Failed to save metadata: {}", e);
        }
    }
    
    Ok((path, metadata))
}

/// Streaming state for typewriter effect
struct StreamingState {
    text: String,
    last_update: Instant,
    msg_id: Option<i32>,
}

/// Run Loom agent with streaming support
async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    bot: Bot,
    reply_to: Option<i32>,
) -> Result<String, String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);
    
    let thread_id = format!("telegram_{}", chat_id);
    let chat_id = teloxide::types::ChatId(chat_id);
    
    // Send initial message (reply to original if available)
    let mut send_msg = bot.send_message(chat_id, "...");
    if let Some(msg_id) = reply_to {
        send_msg = send_msg.reply_parameters(ReplyParameters::new(MessageId(msg_id)));
    }
    let initial_msg = send_msg.await
        .map_err(|e| format!("Failed to send initial message: {}", e))?;
    
    // Create shared state
    let state = Arc::new(Mutex::new(StreamingState {
        text: String::new(),
        last_update: Instant::now(),
        msg_id: Some(initial_msg.id.0),
    }));
    
    let opts = RunOptions {
        message: message.to_string(),
        thread_id: Some(thread_id),
        working_folder: Some(PathBuf::from(".")),
        session_id: None,
        role_file: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
        model: None,
        mcp_config_path: None,
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
    };
    
    // Create event callback
    let state_clone = state.clone();
    let bot_clone = bot.clone();
    let chat_id_clone = chat_id;
    
    let on_event = move |ev: AnyStreamEvent| {
        let state = state_clone.clone();
        let bot = bot_clone.clone();
        let chat_id = chat_id_clone;
        
        // Extract text from streaming events
        let text_delta = match &ev {
            AnyStreamEvent::React(loom::StreamEvent::Messages { chunk, .. }) => {
                Some(chunk.content.clone())
            }
            _ => None,
        };
        
        if let Some(delta) = text_delta {
            let state = state.clone();
            let bot = bot.clone();
            let chat_id = chat_id;
            
            // Use blocking task to update state and message
            tokio::spawn(async move {
                let mut s = state.lock().await;
                s.text.push_str(&delta);
                
                // Throttle updates: at most every 300ms
                let should_update = s.last_update.elapsed() > Duration::from_millis(300);
                
                if should_update && s.text.len() <= 4000 {
                    // Truncate if too long (Telegram limit is 4096)
                    let display_text = if s.text.len() > 4000 {
                        format!("{}...", &s.text[..3950])
                    } else {
                        s.text.clone()
                    };
                    
                    if let Some(msg_id) = s.msg_id {
                        let _ = bot.edit_message_text(chat_id, teloxide::types::MessageId(msg_id), &display_text).await;
                    }
                    s.last_update = Instant::now();
                }
            });
        }
    };
    
    let result = run_agent_with_options(&opts, &RunCmd::React, Some(Box::new(on_event))).await;
    
    // Get final text
    let final_state = state.lock().await;
    let final_text = final_state.text.clone();
    let msg_id = final_state.msg_id;
    drop(final_state);
    
    // Update with final message
    if let Some(msg_id) = msg_id {
        let display_text = if final_text.len() > 4000 {
            format!("{}...", &final_text[..3950])
        } else if final_text.is_empty() {
            "(empty response)".to_string()
        } else {
            final_text.clone()
        };
        
        let _ = bot.edit_message_text(chat_id, teloxide::types::MessageId(msg_id), &display_text).await;
    }
    
    match result {
        Ok(RunCompletion::Finished(_)) => Ok(final_text),
        Ok(RunCompletion::Cancelled) => Err("Agent run was cancelled".to_string()),
        Err(e) => Err(format!("Agent error: {}", e)),
    }
}

/// Check if the message mentions the bot
fn is_bot_mentioned(msg: &Message, bot_username: &str) -> bool {
    if bot_username.is_empty() {
        return false;
    }

    if let Some(text) = msg.text() {
        let mention = format!("@{}", bot_username.to_lowercase());
        return text.to_lowercase().contains(&mention);
    }

    false
}

/// Default message handler with download support
/// 
/// This handler processes incoming messages and downloads media files.
pub async fn default_handler(
    bot: Bot, 
    msg: Message,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
) -> Result<(), teloxide::RequestError> {
    // Message basic info
    let message_id = msg.id;
    let chat_id = msg.chat.id;
    let _date = msg.date;  // Available for future use
    
    // Sender info (optional, may be None for channel posts)
    let _from_user = &msg.from;
    
    tracing::info!(
        "Message #{} in chat {}",
        message_id, chat_id
    );
    
    // Create default download config
    let config = DownloadConfig::default();
    
    // Initialize download directory
    if let Err(e) = config.init().await {
        tracing::error!("Failed to create download directory: {}", e);
    }
    
    // Handle different message types
    match &msg.kind {
        MessageKind::Common(_) => {
            // Handle text messages
            if let Some(text) = msg.text() {
                tracing::info!("Text: {}", text);
                
                if settings.only_respond_when_mentioned && !is_bot_mentioned(&msg, &bot_username) {
                    tracing::debug!("Ignoring message (bot not mentioned)");
                    return Ok(());
                }

                let clean_text = if !bot_username.is_empty() {
                    let mention = format!("@{} ", bot_username);
                    text.replace(&mention, "").replace(&format!("@{}", bot_username), "")
                } else {
                    text.to_string()
                };
                
                match run_loom_agent_streaming(&clean_text, chat_id.0, bot.clone(), Some(message_id.0)).await {
                    Ok(_reply) => {
                        // Message already updated in streaming function
                    }
                    Err(e) => {
                        tracing::error!("Agent error: {}", e);
                        let _ = bot.send_message(chat_id, format!("Error: {}", e)).await;
                    }
                }
            }
            
            // Handle photos
            if let Some(photos) = msg.photo() {
                match download_photo(&bot, photos, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Photo downloaded");
                        bot.send_message(chat_id, format!("📷 图片已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download photo: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
                            .await?;
                    }
                }
            }
            
            // Handle documents
            if let Some(doc) = msg.document() {
                match download_document(&bot, doc, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Document downloaded");
                        bot.send_message(chat_id, format!("📁 文件已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download document: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
                            .await?;
                    }
                }
            }
            
            // Handle videos
            if let Some(video) = msg.video() {
                match download_video(&bot, video, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Video downloaded");
                        bot.send_message(chat_id, format!("🎬 视频已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download video: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
                            .await?;
                    }
                }
            }
        }
        
        // Handle other message kinds
        _ => {
            tracing::debug!("Unhandled message kind: {:?}", msg.kind);
        }
    }
    
    Ok(())
}

/// Create a handler with custom download configuration
pub fn create_handler_with_config(config: Arc<DownloadConfig>) -> impl Fn(Bot, Message) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), teloxide::RequestError>> + Send>> {
    move |bot: Bot, msg: Message| {
        let config = config.clone();
        Box::pin(async move {
            let chat_id = msg.chat.id;
            let message_id = msg.id;
            
            // Initialize download directory
            if let Err(e) = config.init().await {
                tracing::error!("Failed to create download directory: {}", e);
            }
            
            // Handle photos
            if let Some(photos) = msg.photo() {
                match download_photo(&bot, photos, &config, chat_id.0, message_id.0).await {
                    Ok((path, _metadata)) => {
                        bot.send_message(chat_id, format!("📷 图片已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download photo: {}", e);
                    }
                }
            }
            
            // Handle documents
            if let Some(doc) = msg.document() {
                match download_document(&bot, doc, &config, chat_id.0, message_id.0).await {
                    Ok((path, _metadata)) => {
                        bot.send_message(chat_id, format!("📁 文件已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download document: {}", e);
                    }
                }
            }
            
            // Handle text messages
            if let Some(text) = msg.text() {
                bot.send_message(chat_id, format!("收到: {}", text)).await?;
            }
            
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_download_config_default() {
        let config = DownloadConfig::default();
        assert_eq!(config.dir, PathBuf::from("downloads"));
        assert!(!config.save_metadata);
    }
    
    #[test]
    fn test_download_config_new() {
        let config = DownloadConfig::new("/custom/path");
        assert_eq!(config.dir, PathBuf::from("/custom/path"));
        assert!(!config.save_metadata);
    }
    
    #[test]
    fn test_download_config_clone() {
        let config = DownloadConfig::new("/test");
        let cloned = config.clone();
        assert_eq!(config.dir, cloned.dir);
        assert_eq!(config.save_metadata, cloned.save_metadata);
    }
    
    #[test]
    fn test_download_config_file_path() {
        let config = DownloadConfig::new("/tmp/bot_downloads");
        let path = config.get_file_path(123456789, 42, "AgACAgIAAxkBAAI", "jpg");
        assert_eq!(path, PathBuf::from("/tmp/bot_downloads/123456789/42_AgACAgIAAxkBAAI.jpg"));
    }
    
    #[test]
    fn test_download_config_file_path_truncated() {
        let config = DownloadConfig::default();
        let long_file_id = "AgACAgIAAxkBAAIRbGQyAAMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let path = config.get_file_path(123, 1, long_file_id, "jpg");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("1_"));
        assert!(filename.ends_with(".jpg"));
        assert!(filename.len() < 50);
    }
    
    #[test]
    fn test_download_config_file_path_short_id() {
        let config = DownloadConfig::default();
        let short_id = "abc";
        let path = config.get_file_path(123, 1, short_id, "pdf");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "1_abc.pdf");
    }
    
    #[test]
    fn test_download_config_file_path_negative_ids() {
        let config = DownloadConfig::default();
        let path = config.get_file_path(-1001234567890, 999, "fid", "mp4");
        assert!(path.to_str().unwrap().contains("-1001234567890"));
        assert!(path.to_str().unwrap().contains("999_fid.mp4"));
    }
    
    #[test]
    fn test_download_config_metadata_path() {
        let config = DownloadConfig::default();
        let file_path = PathBuf::from("downloads/123/42_photo.jpg");
        let meta_path = config.get_metadata_path(&file_path);
        assert_eq!(meta_path, PathBuf::from("downloads/123/42_photo.json"));
    }
    
    #[test]
    fn test_download_config_metadata_path_nested() {
        let config = DownloadConfig::default();
        let file_path = PathBuf::from("downloads/123/456/78_video.mp4");
        let meta_path = config.get_metadata_path(&file_path);
        assert_eq!(meta_path, PathBuf::from("downloads/123/456/78_video.json"));
    }
    
    #[test]
    fn test_get_file_extension_from_filename() {
        assert_eq!(get_file_extension(Some("test.jpg"), None), "jpg");
        assert_eq!(get_file_extension(Some("document.PDF"), None), "pdf");
        assert_eq!(get_file_extension(Some("file.TAR.GZ"), None), "gz");
        assert_eq!(get_file_extension(Some("photo.jpeg"), None), "jpeg");
    }
    
    #[test]
    fn test_get_file_extension_from_mime() {
        assert_eq!(get_file_extension(None, Some("image/jpeg")), "jpg");
        assert_eq!(get_file_extension(None, Some("image/png")), "png");
        assert_eq!(get_file_extension(None, Some("image/gif")), "gif");
        assert_eq!(get_file_extension(None, Some("image/webp")), "webp");
        assert_eq!(get_file_extension(None, Some("video/mp4")), "mp4");
        assert_eq!(get_file_extension(None, Some("video/webm")), "webm");
        assert_eq!(get_file_extension(None, Some("audio/mpeg")), "mp3");
        assert_eq!(get_file_extension(None, Some("audio/mp3")), "mp3");
        assert_eq!(get_file_extension(None, Some("audio/ogg")), "ogg");
        assert_eq!(get_file_extension(None, Some("application/pdf")), "pdf");
        assert_eq!(get_file_extension(None, Some("application/zip")), "zip");
    }
    
    #[test]
    fn test_get_file_extension_unknown_mime() {
        assert_eq!(get_file_extension(None, Some("application/octet-stream")), "bin");
        assert_eq!(get_file_extension(None, Some("text/plain")), "bin");
    }
    
    #[test]
    fn test_get_file_extension_fallback() {
        assert_eq!(get_file_extension(None, None), "bin");
        assert_eq!(get_file_extension(Some("noextension"), None), "bin");
        assert_eq!(get_file_extension(Some("dot."), None), "bin");
    }
    
    #[test]
    fn test_get_file_extension_long_extension() {
        assert_eq!(get_file_extension(Some("file.verylongextension"), None), "bin");
    }
    
    #[test]
    fn test_get_file_extension_filename_priority() {
        assert_eq!(get_file_extension(Some("test.png"), Some("image/jpeg")), "png");
    }
    
    #[test]
    fn test_file_type_serialization() {
        let types = vec![
            FileType::Photo,
            FileType::Document,
            FileType::Video,
            FileType::Audio,
            FileType::Other,
        ];
        for ft in types {
            let json = serde_json::to_string(&ft).unwrap();
            let parsed: FileType = serde_json::from_str(&json).unwrap();
            assert_eq!(ft, parsed);
        }
    }
    
    #[test]
    fn test_file_metadata_serialization() {
        let metadata = FileMetadata {
            chat_id: 123456789,
            message_id: 42,
            file_id: "AgACAgIAAxkBAAI".to_string(),
            file_unique_id: "AQADeN1x".to_string(),
            file_type: FileType::Photo,
            mime_type: Some("image/jpeg".to_string()),
            file_size: Some(102400),
            original_name: None,
            user_id: Some(987654321),
            downloaded_at: "2026-03-21T09:00:00Z".to_string(),
        };
        
        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: FileMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata.chat_id, parsed.chat_id);
        assert_eq!(metadata.message_id, parsed.message_id);
        assert_eq!(metadata.file_id, parsed.file_id);
        assert_eq!(metadata.file_unique_id, parsed.file_unique_id);
        assert_eq!(metadata.file_type, parsed.file_type);
        assert_eq!(metadata.mime_type, parsed.mime_type);
        assert_eq!(metadata.file_size, parsed.file_size);
        assert_eq!(metadata.original_name, parsed.original_name);
        assert_eq!(metadata.user_id, parsed.user_id);
        assert_eq!(metadata.downloaded_at, parsed.downloaded_at);
    }
    
    #[test]
    fn test_file_metadata_json_format() {
        let metadata = FileMetadata {
            chat_id: 123,
            message_id: 1,
            file_id: "fid".to_string(),
            file_unique_id: "uid".to_string(),
            file_type: FileType::Document,
            mime_type: None,
            file_size: None,
            original_name: Some("test.pdf".to_string()),
            user_id: None,
            downloaded_at: "2026-03-21T09:00:00Z".to_string(),
        };
        
        let json = serde_json::to_string_pretty(&metadata).unwrap();
        assert!(json.contains("\"chat_id\": 123"));
        assert!(json.contains("\"message_id\": 1"));
        assert!(json.contains("\"file_id\": \"fid\""));
        assert!(json.contains("\"file_unique_id\": \"uid\""));
        assert!(json.contains("\"file_type\": \"Document\""));
    }
    
    #[test]
    fn test_check_text_for_bot_mention() {
        fn check_mention(text: &str, bot_username: &str) -> bool {
            if bot_username.is_empty() {
                return false;
            }
            let mention = format!("@{}", bot_username.to_lowercase());
            text.to_lowercase().contains(&mention)
        }
        
        assert!(check_mention("@testbot hello", "testbot"));
        assert!(check_mention("hello @TestBot world", "testbot"));
        assert!(check_mention("@TESTBOT", "testbot"));
        assert!(!check_mention("hello world", "testbot"));
        assert!(!check_mention("@otherbot hello", "testbot"));
        assert!(!check_mention("@testbot hello", ""));
    }
}
