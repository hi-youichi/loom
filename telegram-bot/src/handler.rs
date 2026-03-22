//! Message handlers with dptree dispatching
//!
//! This module provides flexible message handling using teloxide's dptree system.

use crate::config::{Settings, StreamingConfig};
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

/// Streaming state for multi-phase typewriter effect
struct StreamingState {
    /// Message ID for current phase (Think or Act)
    current_msg_id: Option<i32>,
    /// Text accumulated for current phase
    current_text: String,
    /// Current phase: "think" or "act"
    current_phase: String,
    /// Think round counter
    think_count: u32,
    /// Act round counter
    act_count: u32,
    /// Last update time for throttling
    last_update: Instant,
    /// Streaming display settings
    settings: StreamingConfig,
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

/// Run Loom agent with streaming support (multi-phase: Think/Act)
///
/// Note: `reply_to` is currently unused. In multi-phase mode, each phase sends
/// a new message. TODO: Consider using reply_to for the first Think message.
async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    bot: Bot,
    reply_to: Option<i32>,
    settings: &Settings,
) -> Result<String, String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);
    
    let thread_id = format!("telegram_{}", chat_id);
    let chat_id = teloxide::types::ChatId(chat_id);
    
    // Create shared state for multi-phase streaming
    let state = Arc::new(Mutex::new(StreamingState {
        current_phase: String::new(),
        think_count: 0,
        act_count: 0,
        current_msg_id: None,
        current_text: String::new(),
        last_update: Instant::now(),
        settings: settings.streaming.clone(),
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
    
    // Create event callback for multi-phase streaming
    let streaming_settings = settings.streaming.clone();
    let state_clone = state.clone();
    let bot_clone = bot.clone();
    let chat_id_clone = chat_id;
    
    let on_event = move |ev: AnyStreamEvent| {
        let state = state_clone.clone();
        let bot = bot_clone.clone();
        let chat_id = chat_id_clone;
        let settings = streaming_settings.clone();
        
        match &ev {
            // ThinkNode 开始
            AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                if node_id == "think" && settings.show_think_phase => 
            {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    s.think_count += 1;
                    s.current_phase = "think".to_string();
                    
                    // 发送新的 Think 消息
                    let emoji = &s.settings.think_emoji;
                    let header = format!("{} Think #{}", emoji, s.think_count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        s.current_msg_id = Some(msg.id.0);
                        s.current_text = header;
                        s.last_update = Instant::now();
                    }
                });
            }
            
            // ActNode 开始
            AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                if node_id == "act" && settings.show_act_phase =>
            {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    s.act_count += 1;
                    s.current_phase = "act".to_string();
                    
                    // 发送新的 Act 消息
                    let emoji = &s.settings.act_emoji;
                    let header = format!("{} Act #{}", emoji, s.act_count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        s.current_msg_id = Some(msg.id.0);
                        s.current_text = header;
                        s.last_update = Instant::now();
                    }
                });
            }
            
            // Think 阶段：显示思考内容
            AnyStreamEvent::React(loom::StreamEvent::Messages { chunk, .. }) => {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                // 克隆需要的数据以满足 'static 生命周期要求
                let content = chunk.content.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Think 阶段处理
                    if s.current_phase != "think" || !s.settings.show_think_phase {
                        return;
                    }
                    
                    // 添加内容
                    s.current_text.push_str(&content);
                    
                    // Throttle: at most every 500ms
                    if s.last_update.elapsed() < Duration::from_millis(500) {
                        return;
                    }
                    
                    // 截断显示
                    let display_text = truncate_text(&s.current_text, s.settings.max_think_chars);
                    
                    // 更新消息
                    if let Some(msg_id) = s.current_msg_id {
                        let _ = bot.edit_message_text(
                            chat_id, 
                            teloxide::types::MessageId(msg_id), 
                            &display_text
                        ).await;
                    }
                    s.last_update = Instant::now();
                });
            }
            
            // Act 阶段：显示工具调用和结果
            AnyStreamEvent::React(loom::StreamEvent::Updates { state: react_state, .. }) => {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                // 克隆需要的数据以满足 'static 生命周期要求
                let tool_calls = react_state.tool_calls.clone();
                let tool_results = react_state.tool_results.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Act 阶段处理
                    if s.current_phase != "act" || !s.settings.show_act_phase {
                        return;
                    }
                    
                    // Throttle: at most every 500ms
                    if s.last_update.elapsed() < Duration::from_millis(500) {
                        return;
                    }
                    
                    // 构建工具调用显示文本
                    let mut display_parts = Vec::new();
                    for tc in &tool_calls {
                        display_parts.push(format!("🔧 {}", tc.name));
                    }
                    for tr in &tool_results {
                        let status = if tr.is_error { "❌" } else { "✅" };
                        display_parts.push(format!("{} {}", status, tr.name.as_deref().unwrap_or("unknown")));
                    }
                    let display_text = display_parts.join("\n");
                    
                    // 截断总长度
                    let final_text = truncate_text(&display_text, s.settings.max_act_chars);
                    
                    // 更新消息
                    if let Some(msg_id) = s.current_msg_id {
                        let _ = bot.edit_message_text(
                            chat_id, 
                            teloxide::types::MessageId(msg_id), 
                            &final_text
                        ).await;
                    }
                    s.last_update = Instant::now();
                });
            }
            
            _ => {}
        }
    };
    
    // Run the agent with streaming callback
    let result = run_agent_with_options(&opts, &RunCmd::React, Some(Box::new(on_event))).await;
    
    // Get final text
    let final_state = state.lock().await;
    let final_text = final_state.current_text.clone();
    drop(final_state);
    
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
