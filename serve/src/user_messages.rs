//! Handle `UserMessages` request: list stored messages for a thread.

use loom_llm::message::Message;
use loom_protocol::{UserMessageItem, UserMessagesResponse};

/// Handles user_messages request: lists messages from the store for the given thread.
/// When store is None or NoOp, returns empty messages and has_more: false (no error).
/// When thread_id is missing (empty), returns an error response.
pub(crate) async fn handle_user_messages(
    r: loom_protocol::UserMessagesRequest,
    user_message_store: Option<std::sync::Arc<dyn loom_memory::user_message::UserMessageStore>>,
) -> loom_protocol::ServerResponse {
    if r.thread_id.is_empty() {
        return loom_protocol::ServerResponse::Error(loom_protocol::ErrorResponse {
            id: Some(r.id.clone()),
            error: "thread_id is required".to_string(),
        });
    }
    let Some(store) = user_message_store else {
        return loom_protocol::ServerResponse::UserMessages(UserMessagesResponse {
            id: r.id.clone(),
            thread_id: r.thread_id.clone(),
            messages: vec![],
            has_more: Some(false),
        });
    };
    match store.list(&r.thread_id, r.before, r.limit).await {
        Ok(messages) => {
            let items: Vec<UserMessageItem> =
                messages.into_iter().map(|m| message_to_item(&m)).collect();
            loom_protocol::ServerResponse::UserMessages(UserMessagesResponse {
                id: r.id.clone(),
                thread_id: r.thread_id.clone(),
                messages: items,
                has_more: Some(false),
            })
        }
        Err(e) => loom_protocol::ServerResponse::Error(loom_protocol::ErrorResponse {
            id: Some(r.id.clone()),
            error: e.to_string(),
        }),
    }
}

fn message_to_item(m: &Message) -> UserMessageItem {
    let (role, content) = m.to_role_content_pair();
    UserMessageItem {
        role: role.to_string(),
        content,
    }
}
