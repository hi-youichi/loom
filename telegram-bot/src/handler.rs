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
        current_msg_id: None,
        current_text: String::new(),
        current_tools: Vec::new(),
        last_update: Instant::now(),
        last_sent_length: 0,
        settings: settings.streaming.clone(),
    }));
    
    // Phase state uses std::sync::RwLock for synchronous updates in on_event callback
    let phase_state = Arc::new(std::sync::RwLock::new(PhaseState {
        current_phase: String::new(),
        think_count: 0,
        act_count: 0,
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
    let phase_state_clone = phase_state.clone();
    let bot_clone = bot.clone();
    let chat_id_clone = chat_id;
    
    let on_event = move |ev: AnyStreamEvent| {
        let state = state_clone.clone();
        let phase_state = phase_state_clone.clone();
        let bot = bot_clone.clone();
        let chat_id = chat_id_clone;
        let settings = streaming_settings.clone();
        
        match &ev {
            // ThinkNode 开始
            AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                if node_id == "think" && settings.show_think_phase => 
            {
                // 同步更新阶段状态
                let think_count = {
                    let mut ps = phase_state.write().unwrap();
                    ps.think_count += 1;
                    ps.current_phase = "think".to_string();
                    ps.think_count
                };
                
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 发送新的 Think 消息
                    let emoji = &s.settings.think_emoji;
                    let header = format!("{} Think #{}\n\n", emoji, think_count);
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
                // 同步更新阶段状态
                let (act_count, prev_phase) = {
                    let mut ps = phase_state.write().unwrap();
                    let prev = ps.current_phase.clone();
                    ps.act_count += 1;
                    ps.current_phase = "act".to_string();
                    (ps.act_count, prev)
                };
                
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 先更新 Think 阶段的最终内容（如果之前是 Think 阶段）
                    if prev_phase == "think" && s.settings.show_think_phase {
                        if let Some(msg_id) = s.current_msg_id {
                            let display_text = truncate_text(&s.current_text, s.settings.max_think_chars);
                            let _ = bot.edit_message_text(
                                chat_id, 
                                teloxide::types::MessageId(msg_id), 
                                &display_text
                            ).await;
                        }
                    }
                    
                    // 清空工具列表，准备新的 Act 阶段
                    s.current_tools.clear();
                    
                    // 发送新的 Act 消息
                    let emoji = &s.settings.act_emoji;
                    let header = format!("{} Act #{}\n", emoji, act_count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        s.current_msg_id = Some(msg.id.0);
                        s.current_text = header;
                        s.last_update = Instant::now();
                    }
                });
            }
            
            // Think 阶段：显示思考内容（包括 Thinking 和 Message 类型的 chunk）
            // 注意：某些 LLM 只返回 content，不返回 reasoning_content
            AnyStreamEvent::React(loom::StreamEvent::Messages { chunk, .. }) => {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                // 克隆需要的数据以满足 'static 生命周期要求
                let content = chunk.content.clone();
                
                // 在 spawn 之前读取阶段状态
                let current_phase = phase_state.read().unwrap().current_phase.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Think 阶段处理
                    if current_phase != "think" || !s.settings.show_think_phase {
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
                    s.last_sent_length = s.current_text.len();
                });
            }
            
            // 工具开始执行（实时）
            AnyStreamEvent::React(loom::StreamEvent::ToolStart { name, .. }) 
                if settings.show_act_phase =>
            {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                let tool_name = name.clone();
                
                // 在 spawn 之前读取阶段状态
                let current_phase = phase_state.read().unwrap().current_phase.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Act 阶段处理
                    if current_phase != "act" {
                        return;
                    }
                    
                    // 添加工具调用到显示
                    s.current_tools.push(format!("🔧 {}...", tool_name));
                    
                    // Throttle: at most every 300ms
                    if s.last_update.elapsed() < Duration::from_millis(300) {
                        return;
                    }
                    
                    // 构建显示文本
                    let display_text = s.current_tools.join("\n");
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
                    s.last_sent_length = display_text.len();
                });
            }
            
            // 工具执行完成（实时）
            AnyStreamEvent::React(loom::StreamEvent::ToolEnd { name, result, is_error, .. }) 
                if settings.show_act_phase =>
            {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                let tool_name = name.clone();
                let tool_result = result.clone();
                let error = *is_error;
                
                // 在 spawn 之前读取阶段状态
                let current_phase = phase_state.read().unwrap().current_phase.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Act 阶段处理
                    if current_phase != "act" {
                        return;
                    }
                    
                    // 更新工具状态：将 "🔧 xxx..." 替换为完整结果
                    let status = if error { "❌" } else { "✅" };
                    
                    // 截断结果显示（最多200字符）
                    let truncated_result = if tool_result.len() > 200 {
                        format!("{}...", &tool_result[..200])
                    } else {
                        tool_result.clone()
                    };
                    
                    // 转义换行符，保持单行显示
                    let single_line_result = truncated_result
                        .replace('\n', "\\n")
                        .replace('\r', "");
                    
                    let completed = format!("{} {}: {}", status, tool_name, single_line_result);
                    
                    // 查找并替换对应的 "🔧 xxx..." 条目
                    if let Some(pos) = s.current_tools.iter().position(|t| t.starts_with(&format!("🔧 {}...", tool_name))) {
                        s.current_tools[pos] = completed;
                    } else {
                        // 如果没找到，直接添加
                        s.current_tools.push(completed);
                    }
                    
                    // Throttle: at most every 300ms
                    if s.last_update.elapsed() < Duration::from_millis(300) {
                        return;
                    }
                    
                    // 构建显示文本
                    let display_text = s.current_tools.join("\n");
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
            
            // Act 阶段：显示工具调用和结果（通过 Updates 事件，作为备用）
            AnyStreamEvent::React(loom::StreamEvent::Updates { state: react_state, .. }) => {
                let state = state.clone();
                let bot = bot.clone();
                let chat_id = chat_id;
                
                // 克隆需要的数据以满足 'static 生命周期要求
                let tool_calls = react_state.tool_calls.clone();
                let tool_results = react_state.tool_results.clone();
                
                // 在 spawn 之前读取阶段状态
                let current_phase = phase_state.read().unwrap().current_phase.clone();
                
                tokio::spawn(async move {
                    let mut s = state.lock().await;
                    
                    // 只在 Act 阶段处理，且 current_tools 为空时才使用 Updates 数据
                    if current_phase != "act" || !s.settings.show_act_phase || !s.current_tools.is_empty() {
                        return;
                    }
                    
                    // Throttle: at most every 500ms
                    if s.last_update.elapsed() < Duration::from_millis(500) {
                        return;
                    }
                    
                    // 构建工具调用显示文本（仅在没有实时事件时使用）
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
    
    // Send any remaining unsent content (fix for throttle-induced content loss)
    {
        let mut s = state.lock().await;
        let has_unsent = s.current_text.len() > s.last_sent_length;
        
        if has_unsent {
            // Determine which phase we're in and get appropriate settings
            let current_phase = phase_state.read().unwrap().current_phase.clone();
            let (max_chars, show_phase) = if current_phase == "think" {
                (s.settings.max_think_chars, s.settings.show_think_phase)
            } else {
                (s.settings.max_act_chars, s.settings.show_act_phase)
            };
            
            if show_phase {
                let display_text = truncate_text(&s.current_text, max_chars);
                
                if let Some(msg_id) = s.current_msg_id {
                    let _ = bot.edit_message_text(
                        chat_id, 
                        teloxide::types::MessageId(msg_id), 
                        &display_text
                    ).await;
                    tracing::debug!("Sent final unsent content: {} chars", 
                        s.current_text.len() - s.last_sent_length);
                }
            }
            
            s.last_sent_length = s.current_text.len();
        }
    }
    
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
