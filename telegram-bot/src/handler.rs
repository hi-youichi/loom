//! Message handlers with dptree dispatching
//!
//! This module provides flexible message handling using teloxide's dptree system.

use teloxide::prelude::*;
use teloxide::types::{Message, MessageKind, PhotoSize, Document, Video};
use teloxide::net::Download;
use tokio::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Download configuration
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// Directory to save downloaded files
    pub dir: PathBuf,
    /// Create subdirectories by date (YYYY-MM-DD)
    pub organize_by_date: bool,
    /// Create subdirectories by chat ID
    pub organize_by_chat: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("downloads"),
            organize_by_date: false,
            organize_by_chat: false,
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
    
    /// Get the full path for a file
    pub fn get_path(&self, filename: &str, chat_id: Option<i64>) -> PathBuf {
        let mut path = self.dir.clone();
        
        // Add chat subdirectory if enabled
        if self.organize_by_chat {
            if let Some(id) = chat_id {
                path.push(format!("chat_{}", id));
            }
        }
        
        // Add date subdirectory if enabled
        if self.organize_by_date {
            let now = std::time::SystemTime::now();
            let datetime: chrono::DateTime<chrono::Utc> = now.into();
            let date = datetime.format("%Y-%m-%d").to_string();
            path.push(&date);
        }
        
        path.push(filename);
        path
    }
    
    /// Initialize download directory
    pub async fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }
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
/// * `chat_id` - Optional chat ID for organizing files
/// * `prefix` - Filename prefix
pub async fn download_photo(
    bot: &Bot,
    photos: &[PhotoSize],
    config: &DownloadConfig,
    chat_id: Option<i64>,
    prefix: &str
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    // Select the largest photo (last in array)
    let largest = photos.last().ok_or("No photo sizes available")?;
    
    let filename = format!("{}_{}x{}.jpg", prefix, largest.width, largest.height);
    let path = config.get_path(&filename, chat_id);
    
    download_file(bot, &largest.file.id, &path).await
}

/// Download a document
/// 
/// # Arguments
/// * `bot` - Telegram bot instance
/// * `doc` - Document object
/// * `config` - Download configuration
/// * `chat_id` - Optional chat ID for organizing files
pub async fn download_document(
    bot: &Bot,
    doc: &Document,
    config: &DownloadConfig,
    chat_id: Option<i64>
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let filename = doc.file_name.as_deref().unwrap_or("unknown_file");
    let path = config.get_path(filename, chat_id);
    
    download_file(bot, &doc.file.id, &path).await
}

/// Download a video
pub async fn download_video(
    bot: &Bot,
    video: &Video,
    config: &DownloadConfig,
    chat_id: Option<i64>,
    prefix: &str
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let filename = format!("{}_{}.mp4", prefix, video.width);
    let path = config.get_path(&filename, chat_id);
    
    download_file(bot, &video.file.id, &path).await
}

/// Default message handler with download support
/// 
/// This handler processes incoming messages and downloads media files.
pub async fn default_handler(bot: Bot, msg: Message) -> Result<(), teloxide::RequestError> {
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
                
                // Echo the message back
                bot.send_message(chat_id, format!("收到: {}", text)).await?;
            }
            
            // Handle photos
            if let Some(photos) = msg.photo() {
                match download_photo(&bot, photos, &config, Some(chat_id.0), &format!("photo_{}", message_id.0)).await {
                    Ok(path) => {
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
                match download_document(&bot, doc, &config, Some(chat_id.0)).await {
                    Ok(path) => {
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
                match download_video(&bot, video, &config, Some(chat_id.0), &format!("video_{}", message_id.0)).await {
                    Ok(path) => {
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
                match download_photo(&bot, photos, &config, Some(chat_id.0), &format!("photo_{}", message_id.0)).await {
                    Ok(path) => {
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
                match download_document(&bot, doc, &config, Some(chat_id.0)).await {
                    Ok(path) => {
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
        assert!(!config.organize_by_date);
        assert!(!config.organize_by_chat);
    }
    
    #[test]
    fn test_download_config_path() {
        let config = DownloadConfig::new("/tmp/bot_downloads");
        let path = config.get_path("test.jpg", None);
        assert_eq!(path, PathBuf::from("/tmp/bot_downloads/test.jpg"));
    }
    
    #[test]
    fn test_download_config_path_with_chat() {
        let mut config = DownloadConfig::new("/tmp/bot_downloads");
        config.organize_by_chat = true;
        let path = config.get_path("test.jpg", Some(12345));
        assert_eq!(path, PathBuf::from("/tmp/bot_downloads/chat_12345/test.jpg"));
    }
    
    #[test]
    fn test_download_config_path_with_date() {
        let mut config = DownloadConfig::new("/tmp/bot_downloads");
        config.organize_by_date = true;
        let path = config.get_path("test.jpg", None);
        assert!(path.to_str().unwrap().contains("/tmp/bot_downloads/"));
    }
}
