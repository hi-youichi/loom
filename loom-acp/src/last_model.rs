//! Persist the last-used model to `{loom_home}/last-model`.
//!
//! Written when the user selects a model via `set_session_config_option("model")`.
//! Read as fallback when `new_session` has no `MODEL` / `OPENAI_MODEL` env var.

use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "last-model";

fn file_path() -> PathBuf {
    config::home::loom_home().join(FILE_NAME)
}

pub fn save(model: &str) {
    let path = file_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, model);
}

pub fn load() -> Option<String> {
    let s = fs::read_to_string(file_path()).ok()?;
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
