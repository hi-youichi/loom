//! Mock implementations for testing
//!
//! Simple mock implementations for unit testing

use async_trait::async_trait;
use crate::error::BotError;
use std::sync::{Arc, RwLock};

/// Mock Message Sender
pub struct MockSender {
    messages: Arc<RwLock<Vec<(i64, String)>>>,
}

impl MockSender {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub fn get_messages(&self) -> Vec<(i64, String)> {
        self.messages.read().unwrap().clone()
    }
    
    pub fn clear(&self) {
        self.messages.write().unwrap().clear();
    }
}

impl Default for MockSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::MessageSender for MockSender {
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<(), BotError> {
        self.messages.write().unwrap().push((chat_id, text.to_string()));
        Ok(())
    }

    async fn send_text_with_parse_mode(
        &self,
        chat_id: i64,
        text: &str,
        _parse_mode: teloxide::types::ParseMode,
    ) -> Result<(), BotError> {
        self.messages.write().unwrap().push((chat_id, text.to_string()));
        Ok(())
    }

    async fn reply_to(
        &self,
        chat_id: i64,
        _reply_to_message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        self.messages.write().unwrap().push((chat_id, text.to_string()));
        Ok(())
    }

    async fn edit_message(
        &self,
        chat_id: i64,
        _message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        self.messages.write().unwrap().push((chat_id, text.to_string()));
        Ok(())
    }
}

/// Mock Agent Runner
pub struct MockAgentRunner {
    response: String,
    should_fail: bool,
    calls: Arc<RwLock<Vec<String>>>,
}

impl MockAgentRunner {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            should_fail: false,
            calls: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub fn failing() -> Self {
        Self {
            response: String::new(),
            should_fail: true,
            calls: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub fn get_calls(&self) -> Vec<String> {
        self.calls.read().unwrap().clone()
    }
}

#[async_trait]
impl crate::traits::AgentRunner for MockAgentRunner {
    async fn run(
        &self,
        prompt: &str,
        _chat_id: i64,
        _message_id: Option<i32>,
    ) -> Result<String, BotError> {
        self.calls.write().unwrap().push(prompt.to_string());
        
        if self.should_fail {
            Err(BotError::Agent("Mock error".to_string()))
        } else {
            Ok(self.response.clone())
        }
    }
}

/// Mock Session Manager
pub struct MockSessionManager {
    resets: Arc<RwLock<usize>>,
}

impl MockSessionManager {
    pub fn new() -> Self {
        Self {
            resets: Arc::new(RwLock::new(0)),
        }
    }
    
    pub fn reset_count(&self) -> usize {
        *self.resets.read().unwrap()
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
        let mut count = self.resets.write().unwrap();
        *count += 1;
        Ok(*count)
    }

    async fn exists(&self, _thread_id: &str) -> Result<bool, BotError> {
        Ok(false)
    }
}
