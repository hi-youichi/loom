//! User message store: append and list messages per thread.
//!
//! Used so that clients can read full, ordered message history by `thread_id`
//! from a dedicated store instead of short-term memory or checkpoint.

use async_trait::async_trait;

use crate::message::Message;

/// Error from [`UserMessageStore`] operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UserMessageStoreError {
    #[error("user message store error: {0}")]
    Other(String),
}

/// Store for user-facing messages per thread.
///
/// - `append`: add one message; caller ensures order and thread consistency.
/// - `list`: return messages for the thread in order; `before` is a pagination cursor (e.g. seq or id), `limit` caps the count.
#[async_trait]
pub trait UserMessageStore: Send + Sync {
    /// Appends one message for the given thread.
    async fn append(
        &self,
        thread_id: &str,
        message: &Message,
    ) -> Result<(), UserMessageStoreError>;

    /// Lists messages for the thread in order.
    ///
    /// - `before`: if set, return only messages with seq (or id) less than this (cursor-based pagination).
    /// - `limit`: max number of messages to return.
    async fn list(
        &self,
        thread_id: &str,
        before: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Vec<Message>, UserMessageStoreError>;
}

/// No-op implementation: append does nothing, list always returns an empty vec.
#[derive(Debug, Default)]
pub struct NoOpUserMessageStore;

#[async_trait]
impl UserMessageStore for NoOpUserMessageStore {
    async fn append(
        &self,
        _thread_id: &str,
        _message: &Message,
    ) -> Result<(), UserMessageStoreError> {
        Ok(())
    }

    async fn list(
        &self,
        _thread_id: &str,
        _before: Option<u64>,
        _limit: Option<u32>,
    ) -> Result<Vec<Message>, UserMessageStoreError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_append_does_not_error() {
        let store = NoOpUserMessageStore;
        store
            .append("t1", &Message::user("hi"))
            .await
            .expect("append should succeed");
    }

    #[tokio::test]
    async fn noop_list_returns_empty() {
        let store = NoOpUserMessageStore;
        let msgs = store
            .list("t1", None, Some(10))
            .await
            .expect("list should succeed");
        assert!(msgs.is_empty());
    }
}
