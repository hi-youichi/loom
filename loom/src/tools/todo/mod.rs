//! Todo tools: todo_write, todo_read.
//!
//! Persist todo list as JSON:
//! - With thread_id: `~/.loom/thread/{thread_id}/todo.json`
//! - Without thread_id: `~/.loom/todo.json` (global fallback)

mod todo_read;
mod todo_write;

pub use todo_read::{TodoReadTool, TOOL_TODO_READ};
pub use todo_write::{TodoWriteTool, TOOL_TODO_WRITE};

const TODOS_FILENAME: &str = "todo.json";

/// Returns the path to the todo list file.
/// - With thread_id: `~/.loom/thread/{thread_id}/todo.json`
/// - Without thread_id: `~/.loom/todo.json` (global fallback)
pub fn todo_file_path(
    thread_id: Option<&str>,
) -> Result<std::path::PathBuf, crate::tool_source::ToolSourceError> {
    let base = env_config::home::loom_home();
    match thread_id {
        Some(tid) => Ok(env_config::home::thread_session_dir(tid).join(TODOS_FILENAME)),
        None => Ok(base.join(TODOS_FILENAME)),
    }
}

/// Single todo item.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoInfo {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[cfg(test)]
pub(crate) static XDG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{TodoInfo, TODOS_FILENAME};

    /// Given LOOM_HOME is set, todo_file_path returns loom_home/todo.json.
    #[test]
    fn todo_file_path_uses_loom_home() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = super::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let path = super::todo_file_path(None).unwrap();
        assert_eq!(path, dir.path().join("todo.json"));
        std::env::remove_var("LOOM_HOME");
    }

    /// Given LOOM_HOME and thread_id, todo_file_path returns loom_home/thread/{thread_id}/todo.json.
    #[test]
    fn todo_file_path_with_thread_id() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = super::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let path = super::todo_file_path(Some("session-123")).unwrap();
        assert_eq!(
            path,
            dir.path()
                .join("thread")
                .join("session-123")
                .join("todo.json")
        );
        std::env::remove_var("LOOM_HOME");
    }

    /// TodoInfo roundtrip: serialize to JSON and deserialize back.
    #[test]
    fn todo_info_serialize_deserialize_roundtrip() {
        let t = TodoInfo {
            id: "id1".to_string(),
            content: "content".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: TodoInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, t.id);
        assert_eq!(back.content, t.content);
        assert_eq!(back.status, t.status);
        assert_eq!(back.priority, t.priority);
    }

    #[test]
    fn constants_match_docs() {
        assert_eq!(TODOS_FILENAME, "todo.json");
    }
}
