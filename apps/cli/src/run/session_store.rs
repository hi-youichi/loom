use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct FileSessionStore {
    base_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>,
}

impl FileSessionStore {
    pub fn new(base_dir: &Path) -> Self {
        let _ = fs::create_dir_all(base_dir);
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn default_path() -> PathBuf {
        config::home::loom_home().join("data").join("sessions")
    }

    pub fn save(&self, session: &StoredSession) -> Result<(), String> {
        fs::create_dir_all(&self.base_dir).map_err(|e| e.to_string())?;
        let path = self.session_path(&session.id);
        let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
        let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load(&self, session_id: &str) -> Result<StoredSession, String> {
        let path = self.session_path(session_id);
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }

    pub fn delete(&self, session_id: &str) -> Result<(), String> {
        let path = self.session_path(session_id);
        fs::remove_file(&path).map_err(|e| e.to_string())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<StoredSession>, String> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut sessions: Vec<StoredSession> = fs::read_dir(&self.base_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map(|ext| ext == "json").unwrap_or(false)
            })
            .filter_map(|e| {
                let content = fs::read_to_string(e.path()).ok()?;
                serde_json::from_str(&content).ok()
            })
            .collect();

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(limit);
        Ok(sessions)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<StoredSession>, String> {
        let query_lower = query.to_lowercase();
        let all = self.list(1000)?;
        let results: Vec<StoredSession> = all
            .into_iter()
            .filter(|s| {
                s.content.to_lowercase().contains(&query_lower)
                    || s.title.as_ref().map(|t| t.to_lowercase().contains(&query_lower)).unwrap_or(false)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .take(limit)
            .collect();
        Ok(results)
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", session_id))
    }
}

pub fn store_session_from_conversation(
    store: &FileSessionStore,
    session_id: &str,
    user_msg: &str,
    assistant_reply: &str,
    tags: Vec<String>,
) -> Result<(), String> {
    let existing = store.load(session_id).ok();

    let (content, title, created_at) = match existing {
        Some(mut s) => {
            s.content.push_str(&format!(
                "\n\n---\n**User**: {}\n\n**Assistant**: {}",
                user_msg, assistant_reply
            ));
            s.updated_at = Utc::now();
            s.tags.extend(tags);
            s.tags.sort();
            s.tags.dedup();
            store.save(&s)?;
            return Ok(());
        }
        None => {
            let content = format!("**User**: {}\n\n**Assistant**: {}", user_msg, assistant_reply);
            let title = if user_msg.chars().count() > 60 {
                format!("{}...", user_msg.chars().take(60).collect::<String>())
            } else {
                user_msg.to_string()
            };
            (content, Some(title), Utc::now())
        }
    };

    let session = StoredSession {
        id: session_id.to_string(),
        title,
        content,
        created_at,
        updated_at: Utc::now(),
        tags,
    };
    store.save(&session)
}
