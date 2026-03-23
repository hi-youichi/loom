//! File download functionality
//!
//! Provides functions for downloading files from Telegram.

use crate::error::BotError;
use std::path::{Path, PathBuf};
use teloxide::prelude::*;
use teloxide::types::{PhotoSize, Document, Video};
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
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ..Default::default()
        }
    }
    
    pub fn get_file_path(&self, chat_id: i64, message_id: i32, file_id: &str, ext: &str) -> PathBuf {
        let truncated_id = if file_id.len() > 24 { &file_id[..24] } else { file_id };
        let filename = format!("{}_{}.{}", message_id, truncated_id, ext);
        
        let mut path = self.dir.clone();
        path.push(format!("{}", chat_id));
        path.push(&filename);
        path
    }
    
    pub fn get_metadata_path(&self, file_path: &Path) -> PathBuf {
        file_path.with_extension("json")
    }
    
    pub async fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }
}

/// Extract file extension from filename or MIME type
fn get_file_extension(filename: Option<&str>, mime_type: Option<&str>) -> String {
    if let Some(name) = filename {
        if let Some(dot_pos) = name.rfind('.') {
            let ext = &name[dot_pos + 1..];
            if !ext.is_empty() && ext.len() <= 10 {
                return ext.to_lowercase();
            }
        }
    }
    
    if let Some(mime) = mime_type {
        return match mime {
            "image/jpeg" | "image/jpg" => "jpg".to_string(),
            "image/png" => "png".to_string(),
            "image/gif" => "gif".to_string(),
            "image/webp" => "webp".to_string(),
            "video/mp4" => "mp4".to_string(),
            "video/webm" => "webm".to_string(),
            "audio/mpeg" | "audio/mp3" => "mp3".to_string(),
            "audio/ogg" => "ogg".to_string(),
            "application/pdf" => "pdf".to_string(),
            "application/zip" => "zip".to_string(),
            _ => "bin".to_string()
        }
    }
    
    "bin".to_string()
}

/// Save metadata to JSON file
async fn save_metadata(path: &Path, metadata: &FileMetadata) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(metadata)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json).await
}

/// Download a file from Telegram by file_id
pub async fn download_file(
    bot: &Bot, 
    file_id: &str, 
    path: &Path
) -> Result<PathBuf, BotError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    let file = bot.get_file(file_id).await?;
    let mut dst = fs::File::create(path).await?;
    bot.download_file(&file.path, &mut dst).await?;
    
    tracing::info!("Downloaded file to: {:?}", path);
    
    Ok(path.to_path_buf())
}

/// Download a photo
pub async fn download_photo(
    bot: &Bot,
    photos: &[PhotoSize],
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), BotError> {
    let largest = photos.last().ok_or_else(|| {
        BotError::Unknown("No photo sizes available".to_string())
    })?;
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
pub async fn download_document(
    bot: &Bot,
    doc: &Document,
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), BotError> {
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
pub async fn download_video(
    bot: &Bot,
    video: &Video,
    config: &DownloadConfig,
    chat_id: i64,
    message_id: i32
) -> Result<(PathBuf, FileMetadata), BotError> {
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

/// Reset a session by deleting all checkpoints
pub fn reset_session(thread_id: &str) -> Result<usize, String> {
    let db_path = loom::memory::default_memory_db_path();
    
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;
    
    let count = conn
        .execute(
            "DELETE FROM checkpoints WHERE thread_id = ?1",
            [thread_id],
        )
        .map_err(|e| format!("Failed to delete session: {}", e))?;
    
    Ok(count)
}

/// Check if message mentions bot
pub fn is_bot_mentioned(msg: &teloxide::types::Message, bot_username: &str) -> bool {
    if bot_username.is_empty() {
        return false;
    }

    if let Some(text) = msg.text() {
        let mention = format!("@{}", bot_username.to_lowercase());
        return text.to_lowercase().contains(&mention);
    }

    false
}

/// Check if message is a reply to bot
pub fn is_reply_to_bot(msg: &teloxide::types::Message, bot_username: &str) -> bool {
    if let Some(replied_msg) = msg.reply_to_message() {
        if let Some(from) = &replied_msg.from {
            if let Some(username) = &from.username {
                return username.to_lowercase() == bot_username.to_lowercase();
            }
        }
    }
    false
}
