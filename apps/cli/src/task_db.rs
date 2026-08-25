use std::path::PathBuf;

/// Ensure the task database directory exists and return the database path.
///
/// The database lives at `<anureo_home>/tasks/tasks.db`.
pub(crate) fn ensure_task_db() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let db_path = config::home::anureo_home().join("tasks").join("tasks.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(db_path)
}
