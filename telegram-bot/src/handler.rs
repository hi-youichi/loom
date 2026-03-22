//! Message handlers with dptree dispatching
//!
//! This module provides flexible message handling using teloxide's dptree system.

use crate::config::{Settings, StreamingConfig};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use teloxide::prelude::*;
use teloxide::types::{Message, MessageKind, PhotoSize, Document, Video, MessageId};
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

// ============================================================================
// Message Queue Architecture (Phase 3 - Long-term solution)
// ============================================================================

/// Commands for the message handler
enum StreamCommand {
    /// Start a new Think phase
    StartThink { think_count: u32 },
    /// Start a new Act phase  
    StartAct { act_count: u32 },
    /// Add content to current Think message
    ThinkContent { content: String },
    /// Tool started executing
    ToolStart { name: String },
    /// Tool finished executing
    ToolEnd { name: String, result: String, is_error: bool },
    /// Flush any remaining content (end of stream)
    Flush,
}

/// State for the message handler
struct MessageState {
    /// Current message ID
    msg_id: Option<i32>,
    /// Current phase ("think" or "act")
    phase: String,
    /// Accumulated text for Think phase
    think_text: String,
    /// Tool calls for Act phase
    tools: Vec<String>,
    /// Last sent length (for detecting pending content)
    last_sent_length: usize,
    /// Last update time (for throttling)
    last_update: Instant,
    /// Streaming settings
    settings: StreamingConfig,
}

impl MessageState {
    fn new(settings: StreamingConfig) -> Self {
        Self {
            msg_id: None,
            phase: String::new(),
            think_text: String::new(),
            tools: Vec::new(),
            last_sent_length: 0,
            last_update: Instant::now(),
            settings,
        }
    }
    
    /// Check if we should update the message (throttle)
    fn should_update(&self, min_interval_ms: u64) -> bool {
        self.last_update.elapsed() >= Duration::from_millis(min_interval_ms)
    }
    
    /// Flush remaining content and return true if there was anything to flush
    fn has_pending_content(&self) -> bool {
        if self.phase == "think" {
            self.think_text.len() > self.last_sent_length
        } else {
            !self.tools.is_empty()
        }
    }
}

/// Single-consumer message handler that processes stream commands sequentially.
/// This eliminates race conditions and ensures all messages are sent in order.
async fn stream_message_handler(
    mut rx: mpsc::Receiver<StreamCommand>,
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    settings: StreamingConfig,
) {
    let mut state = MessageState::new(settings);
    
    while let Some(cmd) = rx.recv().await {
        match cmd {
            StreamCommand::StartThink { think_count } => {
                // Flush any previous phase content first
                if state.has_pending_content() && state.msg_id.is_some() {
                    if state.phase == "act" {
                        let display = state.tools.join("\n");
                        let text = truncate_text(&display, state.settings.max_act_chars);
                        if let Some(msg_id) = state.msg_id {
                            let _ = bot.edit_message_text(
                                chat_id,
                                MessageId(msg_id),
                                &text,
                            ).await;
                        }
                    } else if state.phase == "think" {
                        let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                        if let Some(msg_id) = state.msg_id {
                            let _ = bot.edit_message_text(
                                chat_id,
                                MessageId(msg_id),
                                &text,
                            ).await;
                        }
                    }
                }
                
                // Start new Think phase
                if state.settings.show_think_phase {
                    let header = format!("{} Think #{}\n\n", state.settings.think_emoji, think_count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        state.msg_id = Some(msg.id.0);
                        state.phase = "think".to_string();
                        state.think_text = header.clone();
                        state.last_sent_length = header.len();
                        state.last_update = Instant::now();
                    }
                }
            }
            
            StreamCommand::StartAct { act_count } => {
                // Flush Think phase content
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                    }
                }
                
                // Start new Act phase
                if state.settings.show_act_phase {
                    let header = format!("{} Act #{}\n\n", state.settings.act_emoji, act_count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        state.msg_id = Some(msg.id.0);
                        state.phase = "act".to_string();
                        state.tools.clear();
                        state.last_sent_length = 0;
                        state.last_update = Instant::now();
                    }
                }
            }
            
            StreamCommand::ThinkContent { content } => {
                if state.phase != "think" || !state.settings.show_think_phase {
                    continue;
                }
                
                state.think_text.push_str(&content);
                
                // Throttle: update at most every 500ms
                if state.should_update(500) {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                    }
                    state.last_sent_length = state.think_text.len();
                    state.last_update = Instant::now();
                }
            }
            
            StreamCommand::ToolStart { name } => {
                if state.phase != "act" || !state.settings.show_act_phase {
                    continue;
                }
                
                state.tools.push(format!("🔧 {}...", name));
                
                // Throttle: update at most every 300ms
                if state.should_update(300) {
                    let display = state.tools.join("\n");
                    let text = truncate_text(&display, state.settings.max_act_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                    }
                    state.last_update = Instant::now();
                }
            }
            
            StreamCommand::ToolEnd { name, result, is_error } => {
                if state.phase != "act" || !state.settings.show_act_phase {
                    continue;
                }
                
                // Update tool status
                let status = if is_error { "❌" } else { "✅" };
                let truncated_result = if result.len() > 200 {
                    format!("{}...", &result[..200])
                } else {
                    result.clone()
                };
                let single_line = truncated_result.replace('\n', "\\n").replace('\r', "");
                let completed = format!("{} {}: {}", status, name, single_line);
                
                // Replace or add
                if let Some(pos) = state.tools.iter().position(|t| t.starts_with(&format!("🔧 {}...", name))) {
                    state.tools[pos] = completed;
                } else {
                    state.tools.push(completed);
                }
                
                // Throttle: update at most every 300ms
                if state.should_update(300) {
                    let display = state.tools.join("\n");
                    let text = truncate_text(&display, state.settings.max_act_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                    }
                    state.last_update = Instant::now();
                }
            }
            
            StreamCommand::Flush => {
                // Send any remaining content
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                        tracing::debug!("Flushed {} chars of think content", 
                            state.think_text.len() - state.last_sent_length);
                    }
                } else if state.phase == "act" && !state.tools.is_empty() {
                    let display = state.tools.join("\n");
                    let text = truncate_text(&display, state.settings.max_act_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(
                            chat_id,
                            MessageId(msg_id),
                            &text,
                        ).await;
                        tracing::debug!("Flushed {} tools", state.tools.len());
                    }
                }
            }
        }
    }
    
    tracing::debug!("Message handler stopped");
}

// ============================================================================
// Legacy structures (will be removed after migration)
// ============================================================================

/// Phase state that needs synchronous updates (uses std::sync::RwLock for sync access)
struct PhaseState {
    /// Current phase: "think" or "act"
    current_phase: String,
    /// Think round counter
    think_count: u32,
    /// Act round counter
    act_count: u32,
}

/// Streaming state for multi-phase typewriter effect
struct StreamingState {
    /// Message ID for current phase (Think or Act)
    current_msg_id: Option<i32>,
    /// Text accumulated for current phase
    current_text: String,
    /// Last update time for throttling
    last_update: Instant,
    /// Streaming display settings
    settings: StreamingConfig,
    /// Current tool calls for Act phase display
    current_tools: Vec<String>,
    /// Length of text that has been sent (to detect pending content)
    last_sent_length: usize,
}

/// Truncate text to max characters, adding "..." if truncated
/// Note: Uses char count (not bytes) for proper UTF-8 handling
fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}


// ============================================================================
// Streaming function - imported from handler_new module
// Uses message queue to ensure updates are processed in order (no race conditions)
// ============================================================================
use crate::handler_new::run_loom_agent_streaming;

/// Reset a session by deleting all checkpoints
fn reset_session(thread_id: &str) -> Result<usize, String> {
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

/// Check if the message is a reply to the bot
fn is_reply_to_bot(msg: &Message, bot_username: &str) -> bool {
    if let Some(replied_msg) = msg.reply_to_message() {
        // Check if replied message is from the bot
        if let Some(from) = &replied_msg.from {
            if let Some(username) = &from.username {
                return username.to_lowercase() == bot_username.to_lowercase();
            }
        }
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
                
                // Handle /reset command
                if text.trim() == "/reset" || text.trim().starts_with("/reset ") {
                    let thread_id = format!("telegram_{}", chat_id.0);
                    match reset_session(&thread_id) {
                        Ok(count) => {
                            bot.send_message(chat_id, format!("🔄 Session reset! Deleted {} checkpoints.", count))
                                .await?;
                        }
                        Err(e) => {
                            tracing::error!("Failed to reset session: {}", e);
                            bot.send_message(chat_id, format!("❌ Reset failed: {}", e))
                                .await?;
                        }
                    }
                    return Ok(());
                }
                
                // Handle /status command
                if text.trim() == "/status" {
                    bot.send_message(chat_id, "✅ Bot is running!")
                        .await?;
                    return Ok(());
                }
                
                // Check if we should respond: mention OR reply to bot
                let should_respond = is_bot_mentioned(&msg, &bot_username) || is_reply_to_bot(&msg, &bot_username);
                
                if settings.only_respond_when_mentioned && !should_respond {
                    tracing::debug!("Ignoring message (bot not mentioned and not a reply)");
                    return Ok(());
                }

                let clean_text = if !bot_username.is_empty() {
                    let mention = format!("@{} ", bot_username);
                    text.replace(&mention, "").replace(&format!("@{}", bot_username), "")
                } else {
                    text.to_string()
                };
                
                match run_loom_agent_streaming(&clean_text, chat_id.0, bot.clone(), Some(message_id.0), &settings).await {
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
    
    // ========== truncate_text tests ==========
    
    #[test]
    fn test_truncate_text_no_limit() {
        let text = "Hello, world!";
        let result = truncate_text(text, 0);
        assert_eq!(result, text);
    }
    
    #[test]
    fn test_truncate_text_short_enough() {
        let text = "Hello";
        let result = truncate_text(text, 10);
        assert_eq!(result, text);
    }
    
    #[test]
    fn test_truncate_text_needs_truncation() {
        let text = "Hello, world!";
        let result = truncate_text(text, 8);
        assert_eq!(result, "Hello...");
        assert!(result.len() <= 8);
    }
    
    #[test]
    fn test_truncate_text_exact_length() {
        let text = "Hello";
        let result = truncate_text(text, 5);
        assert_eq!(result, "Hello");
    }
    
    #[test]
    fn test_truncate_text_utf8_ascii() {
        let text = "Hello, world!";
        let result = truncate_text(text, 5);
        assert_eq!(result, "He...");
    }
    
    #[test]
    fn test_truncate_text_utf8_chinese() {
        let text = "你好世界测试";
        let result = truncate_text(text, 3);
        assert_eq!(result, "..."); // Only room for "..." when max_chars is 3
    }
    
    #[test]
    fn test_truncate_text_utf8_chinese_more() {
        let text = "你好世界测试";
        let result = truncate_text(text, 5);
        assert_eq!(result, "你好..."); // "你好" is 2 chars, + "..." = 5
    }
    
    #[test]
    fn test_truncate_text_utf8_mixed() {
        let text = "Hello世界"; // 7 chars
        let result = truncate_text(text, 6);
        // max_chars=6, so we take 6-3=3 chars and add "..." = "Hel..."
        assert_eq!(result, "Hel...");
    }
    
    #[test]
    fn test_truncate_text_empty() {
        let text = "";
        let result = truncate_text(text, 10);
        assert_eq!(result, "");
    }
    
    // ========== StreamingConfig tests ==========
    
    #[test]
    fn test_streaming_config_default() {
        use crate::config::StreamingConfig;
        
        let config = StreamingConfig::default();
        assert_eq!(config.max_think_chars, 500);
        assert_eq!(config.max_act_chars, 500);
        assert!(config.show_think_phase);
        assert!(config.show_act_phase);
        assert_eq!(config.think_emoji, "🤔");
        assert_eq!(config.act_emoji, "⚡");
    }
    
    #[test]
    fn test_settings_default_streaming() {
        let settings = Settings::default();
        assert_eq!(settings.streaming.max_think_chars, 500);
        assert!(settings.streaming.show_think_phase);
    }
}
