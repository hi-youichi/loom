//! Session management commands: list, show, delete.
//!
//! Uses the unified memory.db to manage all sessions.

use chrono::{DateTime, Local, Utc};
use clap::Subcommand;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Session information returned by list command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID (thread_id)
    pub session_id: String,
    /// Number of checkpoints in this session
    pub checkpoint_count: usize,
    /// Creation time of first checkpoint
    pub created_at: Option<DateTime<Utc>>,
    /// Last updated time (most recent checkpoint)
    pub last_updated: Option<DateTime<Utc>>,
    /// Step number of the latest checkpoint
    pub latest_step: i64,
    /// Source of the latest checkpoint (Input/Loop/Update/Fork)
    pub latest_source: String,
}

/// Session detail with message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    /// Basic session info
    #[serde(flatten)]
    pub info: SessionInfo,
    /// Message count in the state
    pub message_count: usize,
    /// First user message (if any)
    pub first_user_message: Option<String>,
    /// Last assistant reply (if any)
    pub last_assistant_reply: Option<String>,
}

/// Session manager for unified memory.db.
pub struct SessionManager {
    db_path: PathBuf,
}

/// Session command line arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

/// Session subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum SessionCommand {
    /// List all sessions
    List,
    /// Show details of a specific session
    Show {
        /// Session ID to show
        session_id: String,
    },
    /// Delete a session and all its checkpoints
    Delete {
        /// Session ID to delete
        session_id: String,
    },
}

impl SessionManager {
    /// Creates a new session manager with the given database path.
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    /// Creates a session manager with the default path (~/.loom/memory.db).
    pub fn with_default_path() -> Self {
        let db_path = loom::memory::default_memory_db_path();
        Self::new(db_path)
    }

    /// Lists all sessions with summary information.
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let mut stmt = conn
            .prepare(
                r#"
            SELECT 
                thread_id,
                COUNT(*) as checkpoint_count,
                MIN(metadata_created_at) as created_at,
                MAX(metadata_created_at) as last_updated,
                (SELECT metadata_step FROM checkpoints c2 
                 WHERE c2.thread_id = c1.thread_id 
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_step,
                (SELECT metadata_source FROM checkpoints c2 
                 WHERE c2.thread_id = c1.thread_id 
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_source
            FROM checkpoints c1
            GROUP BY thread_id
            ORDER BY last_updated DESC
            "#,
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let sessions = stmt
            .query_map([], |row| {
                let session_id: String = row.get(0)?;
                let checkpoint_count: usize = row.get(1)?;
                let created_at_ms: Option<i64> = row.get(2)?;
                let last_updated_ms: Option<i64> = row.get(3)?;
                let latest_step: i64 = row.get(4)?;
                let latest_source: String = row.get(5)?;

                Ok(SessionInfo {
                    session_id,
                    checkpoint_count,
                    created_at: created_at_ms
                        .and_then(DateTime::from_timestamp_millis),
                    last_updated: last_updated_ms
                        .and_then(DateTime::from_timestamp_millis),
                    latest_step,
                    latest_source,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect sessions: {}", e))?;

        Ok(sessions)
    }

    /// Gets detailed information about a specific session.
    pub fn show_session(&self, session_id: &str) -> Result<Option<SessionDetail>, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        // Get basic session info
        let mut stmt = conn
            .prepare(
                r#"
            SELECT 
                COUNT(*) as checkpoint_count,
                MIN(metadata_created_at) as created_at,
                MAX(metadata_created_at) as last_updated,
                (SELECT metadata_step FROM checkpoints c2 
                 WHERE c2.thread_id = ?1 
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_step,
                (SELECT metadata_source FROM checkpoints c2 
                 WHERE c2.thread_id = ?1 
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_source
            FROM checkpoints
            WHERE thread_id = ?1
            "#,
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let info = stmt
            .query_row([session_id], |row| {
                let checkpoint_count: usize = row.get(0)?;
                let created_at_ms: Option<i64> = row.get(1)?;
                let last_updated_ms: Option<i64> = row.get(2)?;
                let latest_step: i64 = row.get(3)?;
                let latest_source: String = row.get(4)?;

                Ok(SessionInfo {
                    session_id: session_id.to_string(),
                    checkpoint_count,
                    created_at: created_at_ms
                        .and_then(DateTime::from_timestamp_millis),
                    last_updated: last_updated_ms
                        .and_then(DateTime::from_timestamp_millis),
                    latest_step,
                    latest_source,
                })
            })
            .optional()
            .map_err(|e| format!("Failed to query session: {}", e))?;

        let info = match info {
            Some(i) => i,
            None => return Ok(None),
        };

        // Get the latest checkpoint to extract message info
        let mut payload_stmt = conn
            .prepare(
                r#"
            SELECT payload
            FROM checkpoints
            WHERE thread_id = ?1
            ORDER BY metadata_created_at DESC
            LIMIT 1
            "#,
            )
            .map_err(|e| format!("Failed to prepare payload statement: {}", e))?;

        let payload: Option<Vec<u8>> = payload_stmt
            .query_row([session_id], |row| row.get(0))
            .optional()
            .map_err(|e| format!("Failed to query payload: {}", e))?;

        let (message_count, first_user_message, last_assistant_reply) = if let Some(data) = payload
        {
            // Try to deserialize as ReActState
            match serde_json::from_slice::<loom::state::ReActState>(&data) {
                Ok(state) => {
                    let first_user = state.messages.iter().find_map(|m| match m {
                        loom::message::Message::User(s) => Some(s.as_text().to_string()),
                        _ => None,
                    });
                    let last_assistant = state.last_assistant_reply();
                    (state.messages.len(), first_user, last_assistant)
                }
                Err(_) => (0, None, None),
            }
        } else {
            (0, None, None)
        };

        Ok(Some(SessionDetail {
            info,
            message_count,
            first_user_message,
            last_assistant_reply,
        }))
    }

    /// Deletes a session and all its checkpoints.
    pub fn delete_session(&self, session_id: &str) -> Result<usize, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let count = conn
            .execute("DELETE FROM checkpoints WHERE thread_id = ?1", [session_id])
            .map_err(|e| format!("Failed to delete session: {}", e))?;

        Ok(count)
    }

    /// Formats a timestamp for display.
    fn format_datetime(dt: &Option<DateTime<Utc>>) -> String {
        dt.map(|t| {
            let local: DateTime<Local> = t.into();
            local.format("%Y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_else(|| "N/A".to_string())
    }

    /// Prints session list in a formatted table.
    pub fn print_session_list(&self, sessions: &[SessionInfo], json: bool) -> Result<(), String> {
        if json {
            let json_output = serde_json::to_string_pretty(sessions)
                .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
            println!("{}", json_output);
        } else {
            if sessions.is_empty() {
                println!("No sessions found.");
                return Ok(());
            }

            println!(
                "{:<30} {:<8} {:<12} {:<20} {:<20}",
                "SESSION ID", "CHECKPOINTS", "LATEST STEP", "CREATED", "LAST UPDATED"
            );
            println!("{}", "-".repeat(90));

            for session in sessions {
                println!(
                    "{:<30} {:<8} {:<12} {:<20} {:<20}",
                    session.session_id,
                    session.checkpoint_count,
                    session.latest_step,
                    Self::format_datetime(&session.created_at),
                    Self::format_datetime(&session.last_updated)
                );
            }

            println!("\nTotal sessions: {}", sessions.len());
        }
        Ok(())
    }

    /// Prints session detail in a formatted way.
    pub fn print_session_detail(&self, detail: &SessionDetail, json: bool) -> Result<(), String> {
        if json {
            let json_output = serde_json::to_string_pretty(detail)
                .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
            println!("{}", json_output);
        } else {
            println!("Session: {}", detail.info.session_id);
            println!("{}", "=".repeat(60));
            println!("Checkpoints: {}", detail.info.checkpoint_count);
            println!("Messages: {}", detail.message_count);
            println!("Latest Step: {}", detail.info.latest_step);
            println!("Latest Source: {}", detail.info.latest_source);
            println!(
                "Created: {}",
                Self::format_datetime(&detail.info.created_at)
            );
            println!(
                "Last Updated: {}",
                Self::format_datetime(&detail.info.last_updated)
            );

            if let Some(ref msg) = detail.first_user_message {
                let truncated = if msg.chars().count() > 100 {
                    format!("{}...", msg.chars().take(100).collect::<String>())
                } else {
                    msg.clone()
                };
                println!("\nFirst User Message:\n  {}", truncated);
            }

            if let Some(ref reply) = detail.last_assistant_reply {
                let truncated = if reply.chars().count() > 200 {
                    format!("{}...", reply.chars().take(200).collect::<String>())
                } else {
                    reply.clone()
                };
                println!("\nLast Assistant Reply:\n  {}", truncated);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_manager_creation() {
        let manager = SessionManager::with_default_path();
        assert!(manager.db_path.to_string_lossy().contains("memory.db"));
    }

    #[test]
    fn test_format_datetime() {
        let dt = DateTime::from_timestamp_millis(1700000000000);
        let formatted = SessionManager::format_datetime(&dt);
        assert!(formatted.contains("2023"));
    }

    #[test]
    fn test_format_datetime_none() {
        let formatted = SessionManager::format_datetime(&None);
        assert_eq!(formatted, "N/A");
    }
}
