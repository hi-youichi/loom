//! Synthetic [`teloxide::types::Message`] values for integration tests (mock inbound).

use serde_json::json;
use teloxide::types::Message;

fn user_human(id: i64) -> serde_json::Value {
    json!({
        "id": id,
        "is_bot": false,
        "first_name": "User"
    })
}

fn user_bot(username: &str) -> serde_json::Value {
    json!({
        "id": 999,
        "is_bot": true,
        "first_name": "Bot",
        "username": username
    })
}

fn chat_private(id: i64) -> serde_json::Value {
    json!({
        "id": id,
        "type": "private",
        "first_name": "Chat"
    })
}

fn chat_supergroup(id: i64) -> serde_json::Value {
    json!({
        "id": id,
        "type": "supergroup",
        "title": "Test Group"
    })
}

/// Private chat, plain text (maps: E2E-TG-001, E2E-TG-002, …).
pub fn message_private_text(chat_id: i64, message_id: i32, text: &str) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "text": text,
    });
    serde_json::from_value(value).expect("private text Message")
}

/// Supergroup text (maps: E2E-TG-008, E2E-TG-009).
pub fn message_group_text(chat_id: i64, message_id: i32, text: &str) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_supergroup(chat_id),
        "from": user_human(42),
        "text": text,
    });
    serde_json::from_value(value).expect("group text Message")
}

/// Reply thread: user replies to a prior human message (maps: E2E-TG-004).
pub fn message_private_reply_to_text(
    chat_id: i64,
    message_id: i32,
    replied_message_id: i32,
    replied_text: &str,
    text: &str,
) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000001,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "reply_to_message": {
            "message_id": replied_message_id,
            "date": 1000000,
            "chat": chat_private(chat_id),
            "from": user_human(42),
            "text": replied_text,
        },
        "text": text,
    });
    serde_json::from_value(value).expect("reply Message")
}

/// Reply to a message from the bot account (maps: E2E-TG-013).
pub fn message_group_reply_to_bot(
    chat_id: i64,
    message_id: i32,
    bot_username: &str,
    text: &str,
) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000001,
        "chat": chat_supergroup(chat_id),
        "from": user_human(42),
        "reply_to_message": {
            "message_id": message_id - 1,
            "date": 1000000,
            "chat": chat_supergroup(chat_id),
            "from": user_bot(bot_username),
            "text": "Prior bot line",
        },
        "text": text,
    });
    serde_json::from_value(value).expect("reply-to-bot Message")
}

/// Photo only (no caption field).
pub fn message_private_photo_only(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "photo": [
            {
                "file_id": "photo_small",
                "file_unique_id": "pu_small",
                "width": 90,
                "height": 90,
                "file_size": 1000
            },
            {
                "file_id": "photo_big",
                "file_unique_id": "pu_big",
                "width": 320,
                "height": 320,
                "file_size": 5000
            }
        ],
    });
    serde_json::from_value(value).expect("photo-only Message")
}

/// Photo with caption — `text()` is `None`; caption is not the text-message path (maps: E2E-TG-014).
pub fn message_private_photo_with_caption(chat_id: i64, message_id: i32, caption: &str) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "photo": [
            {
                "file_id": "photo_small",
                "file_unique_id": "pu_small",
                "width": 90,
                "height": 90,
                "file_size": 1000
            },
            {
                "file_id": "photo_big",
                "file_unique_id": "pu_big",
                "width": 320,
                "height": 320,
                "file_size": 5000
            }
        ],
        "caption": caption,
    });
    serde_json::from_value(value).expect("photo+caption Message")
}

/// Small document (maps: E2E-TG-006).
pub fn message_private_document(chat_id: i64, message_id: i32, file_name: &str) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "document": {
            "file_id": "doc_file",
            "file_unique_id": "doc_u",
            "file_name": file_name,
            "mime_type": "text/plain",
            "file_size": 12
        },
    });
    serde_json::from_value(value).expect("document Message")
}

/// Short video (maps: E2E-TG-007).
pub fn message_private_video(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "video": {
            "file_id": "vid_file",
            "file_unique_id": "vid_u",
            "width": 320,
            "height": 240,
            "duration": 2,
            "mime_type": "video/mp4",
            "file_size": 4096
        },
    });
    serde_json::from_value(value).expect("video Message")
}

/// Sticker-only common message (maps: E2E-TG-020).
pub fn message_private_sticker(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "sticker": {
            "file_id": "stk_file",
            "file_unique_id": "stk_u",
            "type": "regular",
            "width": 128,
            "height": 128,
            "is_animated": false,
            "is_video": false
        },
    });
    serde_json::from_value(value).expect("sticker Message")
}

/// Shared location (maps: E2E-TG-020).
pub fn message_private_location(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "location": {
            "longitude": -122.4194,
            "latitude": 37.7749
        },
    });
    serde_json::from_value(value).expect("location Message")
}

/// Voice note (maps: E2E-TG-020).
pub fn message_private_voice(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "voice": {
            "file_id": "voice_file",
            "file_unique_id": "voice_u",
            "duration": 3,
            "mime_type": "audio/ogg",
            "file_size": 1024
        },
    });
    serde_json::from_value(value).expect("voice Message")
}

/// Dice — non-`Common` [`MessageKind`](teloxide::types::MessageKind) (maps: E2E-TG-020 unhandled branch).
pub fn message_private_dice(chat_id: i64, message_id: i32) -> Message {
    let value = json!({
        "message_id": message_id,
        "date": 1000000,
        "chat": chat_private(chat_id),
        "from": user_human(42),
        "dice": {
            "emoji": "🎲",
            "value": 4
        },
    });
    serde_json::from_value(value).expect("dice Message")
}
