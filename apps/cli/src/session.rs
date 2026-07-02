//! Session data layer: CRUD operations on sessions in memory.db.
//!
//! Presentation logic lives in [`crate::session_view`].

use chrono::{DateTime, Local, Utc};
use clap::{Args, Subcommand};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Session information returned by list command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub checkpoint_count: usize,
    pub created_at: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
    pub latest_step: i64,
    pub latest_source: String,
    pub title: Option<String>,
}

/// Session detail with message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    #[serde(flatten)]
    pub info: SessionInfo,
    pub message_count: usize,
    pub first_user_message: Option<String>,
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
    List(ListArgs),
    Show { session_id: String },
    Delete { session_id: String },
    Rename { session_id: String, title: String },
    Cat { session_id: String },
}

/// Arguments for the `session list` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct ListArgs {
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    #[arg(long)]
    pub reverse: bool,
    #[arg(long)]
    pub oneline: bool,
    #[arg(long)]
    pub no_pager: bool,
    #[arg(long)]
    pub grep: Option<String>,
    #[arg(long)]
    pub format: Option<String>,
}

impl SessionManager {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    pub fn with_default_path() -> Self {
        Self::new(checkpoint_sqlite_store::default_memory_db_path())
    }

    /// Lists sessions with SQL-level filtering (limit, since, until, reverse).
    pub fn list_sessions_filtered(
        &self,
        limit: usize,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
        reverse: bool,
    ) -> Result<Vec<SessionInfo>, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let mut sql = String::from(
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
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_source,
                (SELECT metadata_summary FROM checkpoints c2
                 WHERE c2.thread_id = c1.thread_id
                 AND metadata_summary IS NOT NULL
                 ORDER BY metadata_created_at DESC LIMIT 1) as title
            FROM checkpoints c1
            GROUP BY thread_id
            "#,
        );

        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        let mut having_clauses: Vec<String> = Vec::new();
        let mut next_param = 1usize;
        if let Some(since) = since {
            having_clauses.push(format!("MAX(metadata_created_at) >= ?{}", next_param));
            params.push(since.timestamp_millis().into());
            next_param += 1;
        }
        if let Some(until) = until {
            having_clauses.push(format!("MAX(metadata_created_at) <= ?{}", next_param));
            params.push(until.timestamp_millis().into());
            next_param += 1;
        }
        if !having_clauses.is_empty() {
            sql.push_str(" HAVING ");
            sql.push_str(&having_clauses.join(" AND "));
        }

        sql.push_str(if reverse { " ORDER BY last_updated ASC" } else { " ORDER BY last_updated DESC" });

        if limit > 0 {
            sql.push_str(&format!(" LIMIT ?{}", next_param));
            params.push((limit as i64).into());
        }

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let sessions = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let session_id: String = row.get(0)?;
                let checkpoint_count: usize = row.get(1)?;
                let created_at_ms: Option<i64> = row.get(2)?;
                let last_updated_ms: Option<i64> = row.get(3)?;
                let latest_step: i64 = row.get(4)?;
                let latest_source: String = row.get(5)?;
                let title: Option<String> = row.get(6)?;

                Ok(SessionInfo {
                    session_id,
                    checkpoint_count,
                    created_at: created_at_ms.and_then(DateTime::from_timestamp_millis),
                    last_updated: last_updated_ms.and_then(DateTime::from_timestamp_millis),
                    latest_step,
                    latest_source,
                    title,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect sessions: {}", e))?;

        Ok(sessions)
    }

    pub fn parse_date_arg(s: &str) -> Result<DateTime<Utc>, String> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(dt.with_timezone(&Utc));
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            let dt = d.and_hms_opt(0, 0, 0).ok_or_else(|| format!("Invalid date: {}", s))?;
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
        }
        Err(format!("Invalid date '{}': expected YYYY-MM-DD or RFC 3339", s))
    }

    pub fn show_session(&self, session_id: &str) -> Result<Option<SessionDetail>, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let mut stmt = conn.prepare(
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
                 ORDER BY metadata_created_at DESC LIMIT 1) as latest_source,
                (SELECT metadata_summary FROM checkpoints c2
                 WHERE c2.thread_id = ?1
                 AND metadata_summary IS NOT NULL
                 ORDER BY metadata_created_at DESC LIMIT 1) as title
            FROM checkpoints
            WHERE thread_id = ?1
            "#,
        ).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let info = stmt.query_row([session_id], |row| {
            let checkpoint_count: usize = row.get(0)?;
            let created_at_ms: Option<i64> = row.get(1)?;
            let last_updated_ms: Option<i64> = row.get(2)?;
            let latest_step: i64 = row.get(3)?;
            let latest_source: String = row.get(4)?;
            let title: Option<String> = row.get(5)?;
            Ok(SessionInfo {
                session_id: session_id.to_string(),
                checkpoint_count,
                created_at: created_at_ms.and_then(DateTime::from_timestamp_millis),
                last_updated: last_updated_ms.and_then(DateTime::from_timestamp_millis),
                latest_step,
                latest_source,
                title,
            })
        }).optional().map_err(|e| format!("Failed to query session: {}", e))?;

        let info = match info { Some(i) => i, None => return Ok(None) };

        let mut payload_stmt = conn.prepare(
            "SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at DESC LIMIT 1"
        ).map_err(|e| format!("Failed to prepare payload statement: {}", e))?;

        let payload: Option<Vec<u8>> = payload_stmt.query_row([session_id], |row| row.get(0))
            .optional().map_err(|e| format!("Failed to query payload: {}", e))?;

        let (message_count, first_user_message, last_assistant_reply) = if let Some(data) = payload {
            match serde_json::from_slice::<agent::state::ReActState>(&data) {
                Ok(state) => {
                    let first_user = state.messages.iter().find_map(|m| match m {
                        loom_llm::message::Message::User(s) => Some(s.as_text().to_string()),
                        _ => None,
                    });
                    (state.messages.len(), first_user, state.last_assistant_reply())
                }
                Err(_) => (0, None, None),
            }
        } else { (0, None, None) };

        Ok(Some(SessionDetail { info, message_count, first_user_message, last_assistant_reply }))
    }

    pub fn delete_session(&self, session_id: &str) -> Result<usize, String> {
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        conn.execute("DELETE FROM checkpoints WHERE thread_id = ?1", [session_id])
            .map_err(|e| format!("Failed to delete session: {}", e))
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> Result<(), String> {
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        let affected = conn.execute(
            "UPDATE checkpoints SET metadata_summary = ?1
             WHERE rowid = (SELECT rowid FROM checkpoints WHERE thread_id = ?2 ORDER BY metadata_created_at DESC LIMIT 1)",
            rusqlite::params![title, session_id],
        ).map_err(|e| format!("Failed to rename session: {}", e))?;
        if affected == 0 { return Err(format!("Session not found: {}", session_id)); }
        Ok(())
    }

    pub fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<SessionInfo>, String> {
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        let query_lower = query.to_lowercase();

        let thread_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT thread_id FROM checkpoints ORDER BY metadata_created_at DESC")
                .map_err(|e| format!("Failed to prepare statement: {}", e))?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| format!("Failed to query: {}", e))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let mut matched = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for thread_id in &thread_ids {
            if matched.len() >= limit { break; }
            if seen.contains(thread_id) { continue; }
            seen.insert(thread_id.clone());

            let payloads: Vec<Vec<u8>> = conn
                .prepare("SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at DESC LIMIT 3")
                .and_then(|mut s| s.query_map([thread_id], |row| row.get(0)).map(|rows| rows.filter_map(|r| r.ok()).collect()))
                .unwrap_or_default();

            let found = payloads.iter().any(|data| {
                serde_json::from_slice::<agent::state::ReActState>(data)
                    .map(|state| state.messages.iter().any(|m| match m {
                        loom_llm::message::Message::System(s) => s.to_lowercase().contains(&query_lower),
                        loom_llm::message::Message::User(uc) => uc.as_text().to_lowercase().contains(&query_lower),
                        loom_llm::message::Message::Assistant(a) => a.content.to_lowercase().contains(&query_lower),
                        loom_llm::message::Message::Tool { content, .. } => content.as_text().map(|t| t.to_lowercase().contains(&query_lower)).unwrap_or(false),
                    })).unwrap_or(false)
            });

            if found {
                if let Ok(Some(detail)) = self.show_session(thread_id) {
                    matched.push(detail.info);
                }
            }
        }
        Ok(matched)
    }

    pub fn cat_session(&self, session_id: &str) -> Result<Vec<stream_event::CodexEvent>, String> {
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        let mut stmt = conn.prepare("SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at ASC")
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;
        let payloads: Vec<Vec<u8>> = stmt.query_map([session_id], |row| row.get(0))
            .map_err(|e| format!("Failed to query checkpoints: {}", e))?
            .collect::<Result<Vec<_>, _>>().map_err(|e| format!("Failed to collect payloads: {}", e))?;
        if payloads.is_empty() { return Err(format!("Session not found: {}", session_id)); }
        let states: Vec<agent::state::ReActState> = payloads.iter().filter_map(|d| serde_json::from_slice(d).ok()).collect();
        Ok(crate::codex_event_builder::build_codex_events(session_id, &states))
    }

    pub fn extract_session_text(&self, session_id: &str) -> Result<String, String> {
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        let mut stmt = conn.prepare("SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at ASC")
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;
        let payloads: Vec<Vec<u8>> = stmt.query_map([session_id], |row| row.get(0))
            .map_err(|e| format!("Query failed: {}", e))?
            .filter_map(|r| r.ok()).collect();
        if payloads.is_empty() { return Err(format!("Session not found: {}", session_id)); }

        let mut parts = Vec::new();
        for data in &payloads {
            if let Ok(state) = serde_json::from_slice::<agent::state::ReActState>(data) {
                for msg in &state.messages {
                    match msg {
                        loom_llm::message::Message::User(u) => parts.push(format!("User: {}", u.as_text())),
                        loom_llm::message::Message::Assistant(a) => { if !a.content.is_empty() { parts.push(format!("Assistant: {}", a.content)); } }
                        loom_llm::message::Message::Tool { content, .. } => { if let Some(text) = content.as_text() { if !text.is_empty() { parts.push(format!("Tool: {}", text)); } } }
                        _ => {}
                    }
                }
            }
        }
        Ok(parts.join("\n"))
    }

    /// Prints session detail (used by `session show`, not `session list`).
    pub fn print_session_detail(&self, detail: &SessionDetail, json: bool) -> Result<(), String> {
        if json {
            let json_output = serde_json::to_string_pretty(detail)
                .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
            println!("{}", json_output);
        } else {
            println!("Session: {}", detail.info.session_id);
            println!("{}", "=".repeat(60));
            if let Some(ref title) = detail.info.title { println!("Title: {}", title); }
            println!("Checkpoints: {}", detail.info.checkpoint_count);
            println!("Messages: {}", detail.message_count);
            println!("Latest Step: {}", detail.info.latest_step);
            println!("Latest Source: {}", detail.info.latest_source);
            println!("Created: {}", Self::format_datetime(&detail.info.created_at));
            println!("Last Updated: {}", Self::format_datetime(&detail.info.last_updated));
            if let Some(ref msg) = detail.first_user_message {
                let truncated = if msg.chars().count() > 100 { format!("{}...", msg.chars().take(100).collect::<String>()) } else { msg.clone() };
                println!("\nFirst User Message:\n  {}", truncated);
            }
            if let Some(ref reply) = detail.last_assistant_reply {
                let truncated = if reply.chars().count() > 200 { format!("{}...", reply.chars().take(200).collect::<String>()) } else { reply.clone() };
                println!("\nLast Assistant Reply:\n  {}", truncated);
            }
        }
        Ok(())
    }

    fn format_datetime(dt: &Option<DateTime<Utc>>) -> String {
        dt.map(|t| { let local: DateTime<Local> = t.into(); local.format("%Y-%m-%d %H:%M:%S").to_string() })
            .unwrap_or_else(|| "N/A".to_string())
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
        assert!(SessionManager::format_datetime(&dt).contains("2023"));
    }

    #[test]
    fn test_format_datetime_none() {
        assert_eq!(SessionManager::format_datetime(&None), "N/A");
    }

    #[test]
    fn parse_date_arg_rfc3339() {
        let dt = SessionManager::parse_date_arg("2025-07-15T10:30:00Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-07-15T10:30:00+00:00");
    }

    #[test]
    fn parse_date_arg_ymd_date() {
        let dt = SessionManager::parse_date_arg("2025-07-15").unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-07-15T00:00:00+00:00");
    }

    #[test]
    fn parse_date_arg_invalid_returns_error() {
        let result = SessionManager::parse_date_arg("not-a-date");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid date"));
    }
}
