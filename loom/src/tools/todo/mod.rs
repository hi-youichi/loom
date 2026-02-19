//! Todo tools: todo_write, todo_read.
//!
//! Persist todo list as JSON under XDG state home (e.g. `~/.local/state/loom/todos.json` on Linux).
//! Uses the `cross-xdg` crate for cross-platform paths.

mod todo_read;
mod todo_write;

pub use todo_read::{TodoReadTool, TOOL_TODO_READ};
pub use todo_write::{TodoWriteTool, TOOL_TODO_WRITE};

/// Application name used under XDG state_home (e.g. state_home/loom/todos.json).
const XDG_APP_NAME: &str = "loom";
/// Filename for the todo list JSON under the app directory.
const TODOS_FILENAME: &str = "todos.json";

/// Returns the path to the todo list file using XDG state home.
///
/// Resolves to e.g. `$XDG_STATE_HOME/loom/todos.json` (Linux: `~/.local/state/loom/todos.json`).
/// Fails with [`ToolSourceError::InvalidInput`] if XDG base dirs cannot be determined (e.g. no home).
pub fn todo_file_path() -> Result<std::path::PathBuf, crate::tool_source::ToolSourceError> {
    let base = cross_xdg::BaseDirs::new().map_err(|e| {
        crate::tool_source::ToolSourceError::InvalidInput(format!(
            "XDG base dirs unavailable: {}",
            e
        ))
    })?;
    Ok(base.state_home().join(XDG_APP_NAME).join(TODOS_FILENAME))
}

/// Single todo item.
///
/// Used for JSON (de)serialization to/from the XDG todo file.
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
    use super::{TodoInfo, TODOS_FILENAME, XDG_APP_NAME};

    /// Given XDG_STATE_HOME is set, todo_file_path returns state_home/loom/todos.json.
    #[test]
    fn todo_file_path_uses_xdg_state_home() {
        let _g = super::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let path = super::todo_file_path().unwrap();
        assert!(path.ends_with(std::path::Path::new("loom").join("todos.json")));
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "loom");
        assert_eq!(path.file_name().unwrap(), "todos.json");
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
        assert_eq!(XDG_APP_NAME, "loom");
        assert_eq!(TODOS_FILENAME, "todos.json");
    }
}
