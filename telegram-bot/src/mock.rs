//! Mock implementations for testing
//!
//! Simple mock implementations for unit testing

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use teloxide::types::{Document, PhotoSize, Video};

use crate::download::{FileMetadata, FileType};
use crate::error::BotError;
use crate::formatting::FormattedMessage;
use crate::traits::FileDownloader;
use crate::traits::{AgentRunContext, MessageSender};


/// Mock Message Sender
pub struct MockSender {
    messages: Arc<RwLock<Vec<(i64, String)>>>,
    next_message_id: Arc<AtomicI32>,
    /// Next N calls to [`MessageSender::send_text_returning_id`] return [`BotError::Unknown`].
    fail_send_remaining: Arc<AtomicU32>,
}

impl MockSender {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(Vec::new())),
            next_message_id: Arc::new(AtomicI32::new(1)),
            fail_send_remaining: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Simulate send failures for the next `count` `send_text_returning_id` calls (tests only).
    pub fn fail_next_n_sends(&self, count: u32) {
        self.fail_send_remaining.store(count, Ordering::SeqCst);
    }

    pub fn get_messages(&self) -> Vec<(i64, String)> {
        self.messages.read().expect("mock messages lock poisoned").clone()
    }

    pub fn clear(&self) {
        self.messages.write().expect("mock messages lock poisoned").clear();
    }
}

impl Default for MockSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::MessageSender for MockSender {
    async fn send_text_returning_id(&self, chat_id: i64, text: &str) -> Result<i32, BotError> {
        loop {
            let remaining = self.fail_send_remaining.load(Ordering::SeqCst);
            if remaining == 0 {
                break;
            }
            match self.fail_send_remaining.compare_exchange(
                remaining,
                remaining - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Err(BotError::Unknown(
                        "mock send_text_returning_id failure".to_string(),
                    ));
                }
                Err(_) => continue,
            }
        }
        let id = self.next_message_id.fetch_add(1, Ordering::SeqCst);
        self.messages.write().expect("mock messages lock poisoned").push((chat_id, text.to_string()));
        Ok(id)
    }

    async fn send_text_with_parse_mode(
        &self,
        chat_id: i64,
        text: &str,
        _parse_mode: teloxide::types::ParseMode,
    ) -> Result<(), BotError> {
        self.messages.write().expect("mock messages lock poisoned").push((chat_id, text.to_string()));
        Ok(())
    }

    async fn send_formatted_returning_id(
        &self,
        chat_id: i64,
        msg: &FormattedMessage,
    ) -> Result<i32, BotError> {
        let id = self.next_message_id.fetch_add(1, Ordering::SeqCst);
        self.messages
            .write()
            .expect("mock messages lock poisoned")
            .push((chat_id, msg.plain_text_fallback.clone()));
        Ok(id)
    }

    async fn send_formatted(
        &self,
        chat_id: i64,
        msg: &FormattedMessage,
    ) -> Result<(), BotError> {
        self.messages
            .write()
            .expect("mock messages lock poisoned")
            .push((chat_id, msg.plain_text_fallback.clone()));
        Ok(())
    }

    async fn reply_to(
        &self,
        chat_id: i64,
        _reply_to_message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        self.messages.write().expect("mock messages lock poisoned").push((chat_id, text.to_string()));
        Ok(())
    }


    async fn edit_message(
        &self,
        chat_id: i64,
        _message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        self.messages.write().expect("mock messages lock poisoned").push((chat_id, text.to_string()));
        Ok(())
    }

    async fn edit_formatted(
        &self,
        chat_id: i64,
        _message_id: i32,
        msg: &FormattedMessage,
    ) -> Result<(), BotError> {
        self.messages
            .write()
            .expect("mock messages lock poisoned")
            .push((chat_id, msg.plain_text_fallback.clone()));
        Ok(())
    }

    async fn send_reaction(
        &self,
        chat_id: i64,
        _message_id: i32,
        emoji: &str,
    ) -> Result<(), BotError> {
        self.messages.write().expect("mock messages lock poisoned").push((chat_id, emoji.to_string()));
        Ok(())
    }
}

/// Mock Agent Runner
pub struct MockAgentRunner {
    response: String,
    should_fail: bool,
    calls: Arc<RwLock<Vec<String>>>,
    contexts: Arc<RwLock<Vec<AgentRunContext>>>,
    delay: Duration,
    /// When set, [`AgentRunner::run`](crate::traits::AgentRunner::run) sends `response` via this
    /// sender (mirrors streaming delivering user-visible text through Telegram APIs).
    deliver_via_sender: Option<Arc<dyn MessageSender>>,
}

impl MockAgentRunner {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            should_fail: false,
            calls: Arc::new(RwLock::new(Vec::new())),
            contexts: Arc::new(RwLock::new(Vec::new())),
            delay: Duration::ZERO,
            deliver_via_sender: None,
        }
    }

    /// Agent replies are recorded on `sender` via [`MessageSender::send_text`], like a mocked E2E path.
    pub fn with_sender(sender: Arc<dyn MessageSender>, response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            should_fail: false,
            calls: Arc::new(RwLock::new(Vec::new())),
            contexts: Arc::new(RwLock::new(Vec::new())),
            delay: Duration::ZERO,
            deliver_via_sender: Some(sender),
        }
    }

    pub fn with_delay(response: impl Into<String>, delay: Duration) -> Self {
        Self {
            response: response.into(),
            should_fail: false,
            calls: Arc::new(RwLock::new(Vec::new())),
            contexts: Arc::new(RwLock::new(Vec::new())),
            delay,
            deliver_via_sender: None,
        }
    }

    pub fn failing() -> Self {
        Self {
            response: String::new(),
            should_fail: true,
            calls: Arc::new(RwLock::new(Vec::new())),
            contexts: Arc::new(RwLock::new(Vec::new())),
            delay: Duration::ZERO,
            deliver_via_sender: None,
        }
    }

    pub fn get_calls(&self) -> Vec<String> {
        self.calls.read().expect("mock messages lock poisoned").clone()
    }

    pub fn get_contexts(&self) -> Vec<AgentRunContext> {
        self.contexts.read().expect("mock messages lock poisoned").clone()
    }
}

#[async_trait]
impl crate::traits::AgentRunner for MockAgentRunner {
    async fn run(
        &self,
        prompt: &str,
        chat_id: i64,
        context: AgentRunContext,
    ) -> Result<String, BotError> {
        self.calls.write().expect("mock messages lock poisoned").push(prompt.to_string());
        self.contexts.write().expect("mock messages lock poisoned").push(context);

        if self.should_fail {
            return Err(BotError::Agent("Mock error".to_string()));
        }

        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }

        if let Some(sender) = &self.deliver_via_sender {
            sender.send_text(chat_id, &self.response).await?;
            return Ok(String::new());
        }

        Ok(self.response.clone())
    }
}

/// Mock Session Manager
pub struct MockSessionManager {
    reset_calls: Arc<RwLock<usize>>,
    deleted_per_reset: usize,
}

impl MockSessionManager {
    pub fn new() -> Self {
        Self::with_deleted_per_reset(1)
    }

    pub fn with_deleted_per_reset(deleted: usize) -> Self {
        Self {
            reset_calls: Arc::new(RwLock::new(0)),
            deleted_per_reset: deleted,
        }
    }

    pub fn reset_count(&self) -> usize {
        *self.reset_calls.read().expect("mock messages lock poisoned")
    }
}

impl Default for MockSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::SessionManager for MockSessionManager {
    async fn reset(&self, _thread_id: &str) -> Result<usize, BotError> {
        *self.reset_calls.write().expect("mock messages lock poisoned") += 1;
        Ok(self.deleted_per_reset)
    }

    async fn exists(&self, _thread_id: &str) -> Result<bool, BotError> {
        Ok(false)
    }
}

/// [`FileDownloader`](crate::traits::FileDownloader) that always errors; use when the test only exercises text paths.
pub struct StubFileDownloader;

impl StubFileDownloader {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubFileDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileDownloader for StubFileDownloader {
    async fn download_photo(
        &self,
        _chat_id: i64,
        _message_id: i32,
        _photos: &[PhotoSize],
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        Err(BotError::Unknown("stub file downloader".into()))
    }

    async fn download_document(
        &self,
        _chat_id: i64,
        _message_id: i32,
        _document: &Document,
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        Err(BotError::Unknown("stub file downloader".into()))
    }

    async fn download_video(
        &self,
        _chat_id: i64,
        _message_id: i32,
        _video: &Video,
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        Err(BotError::Unknown("stub file downloader".into()))
    }
}

/// Returns a fixed path and metadata for media tests (maps: E2E-TG-005, E2E-TG-006, E2E-TG-007).
pub struct FakeFileDownloader {
    path: PathBuf,
}

impl FakeFileDownloader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
        }
    }

    fn metadata(
        &self,
        chat_id: i64,
        message_id: i32,
        file_type: FileType,
        file_id: &str,
        mime_type: Option<String>,
        original_name: Option<String>,
    ) -> FileMetadata {
        FileMetadata {
            chat_id,
            message_id,
            file_id: file_id.to_string(),
            file_unique_id: format!("{}_uniq", file_id),
            file_type,
            original_name,
            mime_type,
            file_size: Some(1),
            user_id: None,
            downloaded_at: "1970-01-01T00:00:00Z".to_string(),
        }
    }
}

#[async_trait]
impl FileDownloader for FakeFileDownloader {
    async fn download_photo(
        &self,
        chat_id: i64,
        message_id: i32,
        photos: &[PhotoSize],
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        let file_id = photos
            .last()
            .map(|p| p.file.id.as_str())
            .unwrap_or("photo");
        let meta = self.metadata(
            chat_id,
            message_id,
            FileType::Photo,
            file_id,
            Some("image/jpeg".to_string()),
            None,
        );
        Ok((self.path.clone(), meta))
    }

    async fn download_document(
        &self,
        chat_id: i64,
        message_id: i32,
        document: &Document,
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        let meta = self.metadata(
            chat_id,
            message_id,
            FileType::Document,
            document.file.id.as_str(),
            document.mime_type.as_ref().map(|m| m.to_string()),
            document.file_name.clone(),
        );
        Ok((self.path.clone(), meta))
    }

    async fn download_video(
        &self,
        chat_id: i64,
        message_id: i32,
        video: &Video,
    ) -> Result<(PathBuf, FileMetadata), BotError> {
        let meta = self.metadata(
            chat_id,
            message_id,
            FileType::Video,
            video.file.id.as_str(),
            video.mime_type.as_ref().map(|m| m.to_string()),
            None,
        );
        Ok((self.path.clone(), meta))
    }
}

/// Session reset always fails (maps: reset failure user messaging).
pub struct ErrorSessionManager {
    message: String,
}

impl ErrorSessionManager {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
impl crate::traits::SessionManager for ErrorSessionManager {
    async fn reset(&self, _thread_id: &str) -> Result<usize, BotError> {
        Err(BotError::Database(self.message.clone()))
    }

    async fn exists(&self, _thread_id: &str) -> Result<bool, BotError> {
        Ok(false)
    }
}
