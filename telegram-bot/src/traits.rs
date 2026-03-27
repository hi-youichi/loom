//! Trait definitions for dependency injection
//!
//! This module defines the core traits that abstract external dependencies,
//! enabling testable and decoupled code.

use async_trait::async_trait;
use teloxide::types::{ParseMode, PhotoSize, Document, Video};
use std::path::PathBuf;

use crate::config::InteractionMode;
use crate::download::FileMetadata;
use crate::error::BotError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentRunContext {
    pub user_message_id: Option<i32>,
    pub ack_message_id: Option<i32>,
    pub interaction_mode: InteractionMode,
}

/// Message sending interface
#[async_trait]
pub trait MessageSender: Send + Sync {
    /// Send plain text and return the message id for subsequent [`Self::edit_message`] calls.
    async fn send_text_returning_id(&self, chat_id: i64, text: &str) -> Result<i32, BotError>;

    /// Send a plain text message
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<(), BotError> {
        let _ = self.send_text_returning_id(chat_id, text).await?;
        Ok(())
    }

    /// Send a message with formatting (Markdown/HTML)
    async fn send_text_with_parse_mode(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: ParseMode,
    ) -> Result<(), BotError>;

    /// Reply to a specific message
    async fn reply_to(
        &self,
        chat_id: i64,
        reply_to_message_id: i32,
        text: &str,
    ) -> Result<(), BotError>;

    /// Edit an existing message
    async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i32,
        text: &str,
    ) -> Result<(), BotError>;

    /// Send a reaction to a message
    async fn send_reaction(
        &self,
        chat_id: i64,
        message_id: i32,
        emoji: &str,
    ) -> Result<(), BotError>;
}

/// Agent running interface
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// Run the agent and return the response
    async fn run(
        &self,
        prompt: &str,
        chat_id: i64,
        context: AgentRunContext,
    ) -> Result<String, BotError>;
}

/// Session management interface
#[async_trait]
pub trait SessionManager: Send + Sync {
    /// Reset a session, returning the number of deleted checkpoints
    async fn reset(&self, thread_id: &str) -> Result<usize, BotError>;

    /// Check if a session exists
    async fn exists(&self, thread_id: &str) -> Result<bool, BotError>;
}

/// File downloading interface
#[async_trait]
pub trait FileDownloader: Send + Sync {
    /// Download a photo
    async fn download_photo(
        &self,
        chat_id: i64,
        message_id: i32,
        photos: &[PhotoSize],
    ) -> Result<(PathBuf, FileMetadata), BotError>;

    /// Download a document
    async fn download_document(
        &self,
        chat_id: i64,
        message_id: i32,
        document: &Document,
    ) -> Result<(PathBuf, FileMetadata), BotError>;

    /// Download a video
    async fn download_video(
        &self,
        chat_id: i64,
        message_id: i32,
        video: &Video,
    ) -> Result<(PathBuf, FileMetadata), BotError>;
}
