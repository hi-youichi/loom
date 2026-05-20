use std::path::Path;
use std::sync::Mutex;

use chrono::Local;
use rusqlite::{params, Connection};
use serde::Serialize;
use uuid::Uuid;

use crate::models::{Task, TaskStatus};
use crate::params::{CreateParams, ListParams, UpdateParams};

const INIT_SQL: &str = "\
CREATE TABLE IF NOT EXISTS tasks (\
    id          TEXT PRIMARY KEY,\
    name        TEXT NOT NULL,\
    description TEXT NOT NULL DEFAULT '',\
    assignee    TEXT NOT NULL DEFAULT '',\
    start_time  TEXT NOT NULL,\
    created_at  TEXT NOT NULL,\
    status      TEXT NOT NULL DEFAULT 'pending'\
                CHECK(status IN ('pending','in_progress','completed','cancelled'))\
)";

pub struct TaskDb {
    conn: Mutex<Connection>,
}

impl TaskDb {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(INIT_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_task(&self, p: &CreateParams) -> Result<Task, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = Local::now().to_rfc3339();
        let start_time = p
            .start_time
            .as_deref()
            .map(parse_time_input)
            .transpose()?
            .unwrap_or_else(|| now.clone());
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO tasks (id, name, description, assignee, start_time, created_at, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, p.name, p.description, p.assignee, start_time, now, p.status.as_str()],
        )?;

        Ok(Task {
            id,
            name: p.name.clone(),
            description: p.description.clone(),
            assignee: p.assignee.clone(),
            start_time,
            created_at: now,
            status: p.status,
        })
    }

    pub fn show_task(&self, id_prefix: &str) -> Result<Task, ShowError> {
        let tasks = self.find_by_id_prefix(id_prefix)?;
        match tasks.len() {
            0 => Err(ShowError::NotFound(id_prefix.to_string())),
            1 => Ok(tasks.into_iter().next().unwrap()),
            _ => Err(ShowError::Ambiguous {
                prefix: id_prefix.to_string(),
                matches: tasks.into_iter().map(|t| (t.id.clone(), t.name.clone())).collect(),
            }),
        }
    }

    pub fn list_tasks(&self, p: &ListParams) -> Result<TaskList, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut where_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref status) = p.status {
            where_clauses.push(format!("status = ?{}", param_values.len() + 1));
            param_values.push(Box::new(status.as_str().to_string()));
        }

        if let Some(ref assignee) = p.assignee {
            where_clauses.push(format!("assignee = ?{}", param_values.len() + 1));
            param_values.push(Box::new(assignee.clone()));
        }

        if let Some(ref name) = p.name {
            where_clauses.push(format!("name LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(format!("%{}%", name)));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM tasks {}", where_sql);
        let total: u32 = conn.query_row(
            &count_sql,
            param_values
                .as_slice()
                .iter()
                .map(|p| p.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
            |row| row.get(0),
        )?;

        let sort_field = match p.sort_by.as_str() {
            "start_time" => "start_time",
            "name" => "name",
            "status" => "status",
            _ => "created_at",
        };
        let sort_dir = match p.sort_order.as_str() {
            "asc" => "ASC",
            _ => "DESC",
        };

        let offset = (p.page.saturating_sub(1)) * p.limit;
        let data_sql = format!(
            "SELECT id, name, description, assignee, start_time, created_at, status FROM tasks {} ORDER BY {} {} LIMIT ?{} OFFSET ?{}",
            where_sql, sort_field, sort_dir, param_values.len() + 1, param_values.len() + 2
        );

        let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = param_values;
        all_params.push(Box::new(p.limit));
        all_params.push(Box::new(offset));

        let mut stmt = conn.prepare(&data_sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        let tasks = stmt
            .query_map(param_refs.as_slice(), |row| Ok(row_to_task(row)))?
            .filter_map(|t| t.ok())
            .collect();

        Ok(TaskList {
            tasks,
            total,
            limit: p.limit,
            page: p.page,
            has_more: offset + p.limit < total,
        })
    }

    pub fn update_task(&self, p: &UpdateParams) -> Result<Task, Box<dyn std::error::Error>> {
        let existing = self.show_task(&p.id)?;
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref name) = p.name {
            set_clauses.push(format!("name = ?{}", param_values.len() + 1));
            param_values.push(Box::new(name.clone()));
        }

        if let Some(ref description) = p.description {
            set_clauses.push(format!("description = ?{}", param_values.len() + 1));
            param_values.push(Box::new(description.clone()));
        }

        if let Some(ref assignee) = p.assignee {
            set_clauses.push(format!("assignee = ?{}", param_values.len() + 1));
            param_values.push(Box::new(assignee.clone()));
        }

        if let Some(ref start_time) = p.start_time {
            let parsed = parse_time_input(start_time)?;
            set_clauses.push(format!("start_time = ?{}", param_values.len() + 1));
            param_values.push(Box::new(parsed));
        }

        if let Some(ref status) = p.status {
            set_clauses.push(format!("status = ?{}", param_values.len() + 1));
            param_values.push(Box::new(status.as_str().to_string()));
        }

        if set_clauses.is_empty() {
            return Ok(existing);
        }

        let sql = format!(
            "UPDATE tasks SET {} WHERE id = ?{}",
            set_clauses.join(", "),
            param_values.len() + 1
        );
        param_values.push(Box::new(existing.id.clone()));

        conn.execute(
            &sql,
            param_values
                .as_slice()
                .iter()
                .map(|p| p.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;

        drop(conn);
        self.show_task(&existing.id)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    pub fn delete_task(&self, id_prefix: &str) -> Result<Task, ShowError> {
        let task = self.show_task(id_prefix)?;
        let conn = self.conn.lock().map_err(|e| ShowError::DbError(e.to_string()))?;
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![task.id])
            .map_err(|e| ShowError::DbError(e.to_string()))?;
        Ok(task)
    }

    fn find_by_id_prefix(&self, prefix: &str) -> Result<Vec<Task>, ShowError> {
        let conn = self.conn.lock().map_err(|e| ShowError::DbError(e.to_string()))?;
        let like_pattern = format!("{}%", prefix);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, assignee, start_time, created_at, status FROM tasks WHERE id LIKE ?1",
            )
            .map_err(|e| ShowError::DbError(e.to_string()))?;
        let tasks = stmt
            .query_map(params![like_pattern], |row| Ok(row_to_task(row)))
            .map_err(|e| ShowError::DbError(e.to_string()))?
            .filter_map(|t| t.ok())
            .collect();
        Ok(tasks)
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> Task {
    let status_str: String = row.get(6).unwrap_or_default();
    Task {
        id: row.get(0).unwrap_or_default(),
        name: row.get(1).unwrap_or_default(),
        description: row.get(2).unwrap_or_default(),
        assignee: row.get(3).unwrap_or_default(),
        start_time: row.get(4).unwrap_or_default(),
        created_at: row.get(5).unwrap_or_default(),
        status: TaskStatus::from_str(&status_str).unwrap_or(TaskStatus::Pending),
    }
}

#[derive(Debug, Serialize)]
pub struct TaskList {
    pub tasks: Vec<Task>,
    pub total: u32,
    pub limit: u32,
    pub page: u32,
    pub has_more: bool,
}

#[derive(Debug)]
pub enum ShowError {
    NotFound(String),
    Ambiguous {
        prefix: String,
        matches: Vec<(String, String)>,
    },
    DbError(String),
}

impl std::fmt::Display for ShowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShowError::NotFound(id) => write!(f, "task not found: {}", id),
            ShowError::Ambiguous { prefix, matches } => {
                writeln!(
                    f,
                    "ambiguous id '{}', matched {} tasks:",
                    prefix,
                    matches.len()
                )?;
                for (id, name) in matches {
                    writeln!(f, "  {} ... {}", &id[..8.min(id.len())], name)?;
                }
                Ok(())
            }
            ShowError::DbError(e) => write!(f, "database error: {}", e),
        }
    }
}

impl std::error::Error for ShowError {}

fn parse_time_input(input: &str) -> Result<String, String> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(input) {
        return Ok(dt.to_rfc3339());
    }

    let attempts = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d",
    ];

    for fmt in &attempts {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(input, fmt) {
            let local = naive
                .and_local_timezone(Local)
                .single()
                .ok_or_else(|| format!("ambiguous local time: {}", input))?;
            return Ok(local.to_rfc3339());
        }
        if fmt == &"%Y-%m-%d" {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(input, fmt) {
                let local = date
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_local_timezone(Local)
                    .single()
                    .ok_or_else(|| format!("ambiguous local time: {}", input))?;
                return Ok(local.to_rfc3339());
            }
        }
    }

    Err(format!(
        "invalid time format: '{}'. Expected formats: 2025-08-20T10:00:00, 2025-08-20 10:00:00, 2025-08-20",
        input
    ))
}
