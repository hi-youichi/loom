//! Handle `UserMessages` request: list stored messages for a thread.

use loom::{Message, UserMessageItem, UserMessagesResponse};

/// Handles user_messages request: lists messages from the store for the given thread.
/// When store is None or NoOp, returns empty messages and has_more: false (no error).
/// When thread_id is missing (empty), returns an error response.
pub(crate) async fn handle_user_messages(
    r: loom::UserMessagesRequest,
    user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
) -> loom::ServerResponse {
    if r.thread_id.is_empty() {
        return loom::ServerResponse::Error(loom::ErrorResponse {
            id: Some(r.id.clone()),
            error: "thread_id is required".to_string(),
        });
    }
    let Some(store) = user_message_store else {
        return loom::ServerResponse::UserMessages(UserMessagesResponse {
            id: r.id.clone(),
            thread_id: r.thread_id.clone(),
            messages: vec![],
            has_more: Some(false),
        });
    };
    match store.list(&r.thread_id, r.before, r.limit).await {
        Ok(messages) => {
            let items: Vec<UserMessageItem> = messages
                .into_iter()
                .map(|m| message_to_item(&m))
                .collect();
            loom::ServerResponse::UserMessages(UserMessagesResponse {
                id: r.id.clone(),
                thread_id: r.thread_id.clone(),
                messages: items,
                has_more: Some(false),
            })
        }
        Err(e) => loom::ServerResponse::Error(loom::ErrorResponse {
            id: Some(r.id.clone()),
            error: e.to_string(),
        }),
    }
}

fn message_to_item(m: &Message) -> UserMessageItem {
    let (role, content) = match m {
        Message::System(c) => ("system".to_string(), c.clone()),
        Message::User(c) => ("user".to_string(), c.clone()),
        Message::Assistant(c) => ("assistant".to_string(), c.clone()),
    };
    UserMessageItem { role, content }
}
