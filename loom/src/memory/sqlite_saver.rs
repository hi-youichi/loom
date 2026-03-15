//! SQLite-backed checkpointer (SqliteSaver). Persistent across process restarts.
//!
//! Aligns with SQLite checkpoint pattern (cf. loom-checkpoint-sqlite).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::params;

use crate::memory::checkpoint::{
    ChannelVersions, Checkpoint, CheckpointListItem, CheckpointMetadata, CheckpointSource,
    CHECKPOINT_VERSION,
};
use crate::memory::checkpointer::{CheckpointError, Checkpointer};
use crate::memory::config::RunnableConfig;
use crate::memory::serializer::Serializer;
use std::collections::HashMap;

fn source_to_str(s: &CheckpointSource) -> &'static str {
    match s {
        CheckpointSource::Input => "Input",
        CheckpointSource::Loop => "Loop",
        CheckpointSource::Update => "Update",
        CheckpointSource::Fork => "Fork",
    }
}

fn str_to_source(s: &str) -> CheckpointSource {
    match s {
        "Input" => CheckpointSource::Input,
        "Loop" => CheckpointSource::Loop,
        "Update" => CheckpointSource::Update,
        "Fork" => CheckpointSource::Fork,
        _ => CheckpointSource::Update,
    }
}

fn created_at_to_i64(t: &Option<std::time::SystemTime>) -> Option<i64> {
    t.as_ref().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64)
    })
}

fn i64_to_created_at(v: Option<i64>) -> Option<std::time::SystemTime> {
    v.and_then(|ms| std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(ms as u64)))
}

/// SQLite-backed checkpointer. Key: (thread_id, checkpoint_ns, checkpoint_id).
///
/// Persistent; for single-node and dev. Uses spawn_blocking for async.
///
/// **Interaction**: Used as `Arc<dyn Checkpointer<S>>` in StateGraph::compile_with_checkpointer.
pub struct SqliteSaver<S> {
    db_path: std::path::PathBuf,
    serializer: Arc<dyn Serializer<S>>,
}

impl<S> SqliteSaver<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a new SQLite checkpointer and ensures the table exists.
    pub fn new(
        path: impl AsRef<Path>,
        serializer: Arc<dyn Serializer<S>>,
    ) -> Result<Self, CheckpointError> {
        let db_path = path.as_ref().to_path_buf();
        let conn = crate::memory::sqlite_util::open_sqlite_with_wal(&db_path)
            .map_err(|e| CheckpointError::Storage(e))?;
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS checkpoints (
                thread_id TEXT NOT NULL,
                checkpoint_ns TEXT NOT NULL,
                checkpoint_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                payload BLOB NOT NULL,
                channel_versions TEXT NOT NULL,
                metadata_source TEXT NOT NULL,
                metadata_step INTEGER NOT NULL,
                metadata_created_at INTEGER,
                PRIMARY KEY (thread_id, checkpoint_ns, checkpoint_id)
            )
            "#,
            [],
        )
        .map_err(|e| CheckpointError::Storage(e.to_string()))?;
        Ok(Self {
            db_path,
            serializer,
        })
    }

    fn thread_id_required(config: &RunnableConfig) -> Result<String, CheckpointError> {
        config
            .thread_id
            .as_deref()
            .ok_or(CheckpointError::ThreadIdRequired)
            .map(String::from)
    }
}

#[async_trait]
impl<S> Checkpointer<S> for SqliteSaver<S>
where
    S: Clone + Send + Sync + 'static,
{
    async fn put(
        &self,
        config: &RunnableConfig,
        checkpoint: &Checkpoint<S>,
    ) -> Result<String, CheckpointError> {
        let thread_id = Self::thread_id_required(config)?;
        let checkpoint_ns = config.checkpoint_ns.clone();
        let payload = self.serializer.serialize(&checkpoint.channel_values)?;
        let channel_versions = serde_json::to_string(&checkpoint.channel_versions)
            .map_err(|e| CheckpointError::Serialization(e.to_string()))?;
        let metadata_source = source_to_str(&checkpoint.metadata.source).to_string();
        let metadata_step = checkpoint.metadata.step as i64;
        let metadata_created_at = created_at_to_i64(&checkpoint.metadata.created_at);
        let id = checkpoint.id.clone();
        let ts = checkpoint.ts.clone();

        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = crate::memory::sqlite_util::open_sqlite_with_wal(&db_path)
                .map_err(|e| CheckpointError::Storage(e))?;
            conn.execute(
                r#"
                INSERT OR REPLACE INTO checkpoints
                (thread_id, checkpoint_ns, checkpoint_id, ts, payload, channel_versions,
                 metadata_source, metadata_step, metadata_created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    thread_id,
                    checkpoint_ns,
                    id.clone(),
                    ts,
                    payload,
                    channel_versions,
                    metadata_source,
                    metadata_step,
                    metadata_created_at,
                ],
            )
            .map_err(|e| CheckpointError::Storage(e.to_string()))?;
            Ok::<String, CheckpointError>(id)
        })
        .await
        .map_err(|e| CheckpointError::Storage(e.to_string()))?
    }

    async fn get_tuple(
        &self,
        config: &RunnableConfig,
    ) -> Result<Option<(Checkpoint<S>, CheckpointMetadata)>, CheckpointError> {
        let thread_id = Self::thread_id_required(config)?;
        let checkpoint_ns = config.checkpoint_ns.clone();
        let want_id = config.checkpoint_id.clone();
        let db_path = self.db_path.clone();

        type RowData = (String, String, Vec<u8>, String, String, i64, Option<i64>);
        let row: Option<RowData> = tokio::task::spawn_blocking(move || -> Result<Option<RowData>, CheckpointError> {
            let conn = crate::memory::sqlite_util::open_sqlite_with_wal(&db_path)
                .map_err(|e| CheckpointError::Storage(e))?;
            let sql = if want_id.is_some() {
                "SELECT checkpoint_id, ts, payload, channel_versions, metadata_source, metadata_step, metadata_created_at
                 FROM checkpoints WHERE thread_id = ?1 AND checkpoint_ns = ?2 AND checkpoint_id = ?3"
            } else {
                "SELECT checkpoint_id, ts, payload, channel_versions, metadata_source, metadata_step, metadata_created_at
                 FROM checkpoints WHERE thread_id = ?1 AND checkpoint_ns = ?2
                 ORDER BY metadata_created_at DESC LIMIT 1"
            };
            let mut stmt = conn.prepare(sql).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let mut rows = if let Some(ref cid) = want_id {
                stmt.query(params![thread_id, checkpoint_ns, cid])
            } else {
                stmt.query(params![thread_id, checkpoint_ns])
            }
            .map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let row = match rows.next().map_err(|e| CheckpointError::Storage(e.to_string()))? {
                Some(r) => r,
                None => return Ok::<_, CheckpointError>(None),
            };
            let checkpoint_id: String = row.get(0).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let ts: String = row.get(1).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let payload: Vec<u8> = row.get(2).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let channel_versions_json: String = row.get(3).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let metadata_source: String = row.get(4).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let metadata_step: i64 = row.get(5).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let metadata_created_at: Option<i64> = row.get(6).map_err(|e| CheckpointError::Storage(e.to_string()))?;
            Ok(Some((
                checkpoint_id,
                ts,
                payload,
                channel_versions_json,
                metadata_source,
                metadata_step,
                metadata_created_at,
            )))
        })
        .await
        .map_err(|e| CheckpointError::Storage(e.to_string()))??;

        let (
            checkpoint_id,
            ts,
            payload,
            channel_versions_json,
            metadata_source,
            metadata_step,
            metadata_created_at,
        ): RowData = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let channel_values = self.serializer.deserialize(&payload)?;
        let channel_versions: ChannelVersions = serde_json::from_str(&channel_versions_json)
            .map_err(|e| CheckpointError::Serialization(e.to_string()))?;
        let metadata = CheckpointMetadata {
            source: str_to_source(&metadata_source),
            step: metadata_step,
            created_at: i64_to_created_at(metadata_created_at),
            parents: HashMap::new(),
        };
        let checkpoint = Checkpoint {
            v: CHECKPOINT_VERSION,
            id: checkpoint_id.clone(),
            ts,
            channel_values,
            channel_versions,
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            metadata: metadata.clone(),
        };
        Ok(Some((checkpoint, metadata)))
    }

    async fn list(
        &self,
        config: &RunnableConfig,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Vec<CheckpointListItem>, CheckpointError> {
        let thread_id = Self::thread_id_required(config)?;
        let checkpoint_ns = config.checkpoint_ns.clone();
        let db_path = self.db_path.clone();
        let before = before.map(String::from);
        let after = after.map(String::from);

        let items = tokio::task::spawn_blocking(move || {
            let conn = crate::memory::sqlite_util::open_sqlite_with_wal(&db_path)
                .map_err(|e| CheckpointError::Storage(e))?;
            let mut stmt = conn
                .prepare(
                    "SELECT checkpoint_id, metadata_source, metadata_step, metadata_created_at
                     FROM checkpoints WHERE thread_id = ?1 AND checkpoint_ns = ?2
                     ORDER BY metadata_created_at ASC",
                )
                .map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map(params![thread_id, checkpoint_ns], |row| {
                    Ok(CheckpointListItem {
                        checkpoint_id: row.get(0)?,
                        metadata: CheckpointMetadata {
                            source: str_to_source(&row.get::<_, String>(1)?),
                            step: row.get::<_, i64>(2)?,
                            created_at: i64_to_created_at(row.get(3)?),
                            parents: HashMap::new(),
                        },
                    })
                })
                .map_err(|e| CheckpointError::Storage(e.to_string()))?;
            let mut list: Vec<CheckpointListItem> = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| CheckpointError::Storage(e.to_string()))?;
            if let Some(a) = &after {
                if let Some(pos) = list.iter().position(|i| i.checkpoint_id.as_str() == a) {
                    list = list[pos + 1..].to_vec();
                }
            }
            if let Some(b) = &before {
                if let Some(pos) = list.iter().position(|i| i.checkpoint_id.as_str() == b) {
                    list = list[..pos].to_vec();
                }
            }
            if let Some(n) = limit {
                let len = list.len();
                if len > n {
                    list = list[len - n..].to_vec();
                }
            }
            Ok::<Vec<CheckpointListItem>, CheckpointError>(list)
        })
        .await
        .map_err(|e| CheckpointError::Storage(e.to_string()))??;

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn source_roundtrip() {
        let sources = [
            CheckpointSource::Input,
            CheckpointSource::Loop,
            CheckpointSource::Update,
            CheckpointSource::Fork,
        ];
        for s in &sources {
            let st = source_to_str(s);
            let back = str_to_source(st);
            assert_eq!(std::mem::discriminant(s), std::mem::discriminant(&back));
        }
    }

    #[test]
    fn str_to_source_unknown_defaults_to_update() {
        assert!(matches!(str_to_source("unknown"), CheckpointSource::Update));
    }

    #[test]
    fn created_at_roundtrip() {
        let t = UNIX_EPOCH + Duration::from_millis(1700000000000);
        let i = created_at_to_i64(&Some(t));
        assert_eq!(i, Some(1700000000000));
        let back = i64_to_created_at(i);
        assert_eq!(back, Some(t));
    }

    #[test]
    fn created_at_none_roundtrip() {
        assert!(created_at_to_i64(&None).is_none());
        assert!(i64_to_created_at(None).is_none());
    }

    #[test]
    fn thread_id_required_ok() {
        let config = RunnableConfig {
            thread_id: Some("t1".to_string()),
            ..RunnableConfig::default()
        };
        let tid = SqliteSaver::<Vec<u8>>::thread_id_required(&config).unwrap();
        assert_eq!(tid, "t1");
    }

    #[test]
    fn thread_id_required_missing() {
        let config = RunnableConfig {
            thread_id: None,
            ..RunnableConfig::default()
        };
        let result = SqliteSaver::<Vec<u8>>::thread_id_required(&config);
        assert!(result.is_err());
    }

    #[test]
    fn new_creates_table() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let serializer: Arc<dyn Serializer<serde_json::Value>> = Arc::new(crate::memory::serializer::JsonSerializer);
        let _saver = SqliteSaver::<serde_json::Value>::new(&db_path, serializer).unwrap();
        assert!(db_path.exists());
    }

    #[tokio::test]
    async fn put_and_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("rt.db");
        let serializer: Arc<dyn Serializer<serde_json::Value>> = Arc::new(crate::memory::serializer::JsonSerializer);
        let saver = SqliteSaver::<serde_json::Value>::new(&db_path, serializer).unwrap();

        let config = RunnableConfig {
            thread_id: Some("thread-1".to_string()),
            ..RunnableConfig::default()
        };
        let now = SystemTime::now();
        let checkpoint = Checkpoint {
            v: CHECKPOINT_VERSION,
            id: "ck-1".to_string(),
            ts: "2024-01-01T00:00:00Z".to_string(),
            channel_values: serde_json::json!({"key": "value"}),
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            metadata: CheckpointMetadata {
                source: CheckpointSource::Input,
                step: 1,
                created_at: Some(now),
                parents: HashMap::new(),
            },
        };

        let id = saver.put(&config, &checkpoint).await.unwrap();
        assert_eq!(id, "ck-1");

        let result = saver.get_tuple(&config).await.unwrap();
        assert!(result.is_some());
        let (ck, meta) = result.unwrap();
        assert_eq!(ck.id, "ck-1");
        assert_eq!(ck.channel_values, serde_json::json!({"key": "value"}));
        assert!(matches!(meta.source, CheckpointSource::Input));
        assert_eq!(meta.step, 1);
    }

    #[tokio::test]
    async fn list_returns_checkpoints() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("list.db");
        let serializer = Arc::new(crate::memory::serializer::JsonSerializer) as Arc<dyn Serializer<serde_json::Value>>;
        let saver = SqliteSaver::<serde_json::Value>::new(&db_path, serializer).unwrap();

        let config = RunnableConfig {
            thread_id: Some("thread-list".to_string()),
            ..RunnableConfig::default()
        };
        let base = UNIX_EPOCH + Duration::from_millis(1700000000000);
        for i in 0..3 {
            let checkpoint = Checkpoint {
                v: CHECKPOINT_VERSION,
                id: format!("ck-{}", i),
                ts: format!("2024-01-0{}T00:00:00Z", i + 1),
                channel_values: serde_json::json!({"step": i}),
                channel_versions: HashMap::new(),
                versions_seen: HashMap::new(),
                updated_channels: None,
                pending_sends: Vec::new(),
                metadata: CheckpointMetadata {
                    source: CheckpointSource::Loop,
                    step: i,
                    created_at: Some(base + Duration::from_secs(i as u64)),
                    parents: HashMap::new(),
                },
            };
            saver.put(&config, &checkpoint).await.unwrap();
        }

        let items = saver.list(&config, None, None, None).await.unwrap();
        assert_eq!(items.len(), 3);

        let limited = saver.list(&config, Some(2), None, None).await.unwrap();
        assert_eq!(limited.len(), 2);

        let after = saver.list(&config, None, None, Some("ck-0")).await.unwrap();
        assert_eq!(after.len(), 2);

        let before = saver.list(&config, None, Some("ck-2"), None).await.unwrap();
        assert_eq!(before.len(), 2);
    }

    #[tokio::test]
    async fn get_tuple_missing_thread_id() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("no_tid.db");
        let serializer = Arc::new(crate::memory::serializer::JsonSerializer) as Arc<dyn Serializer<serde_json::Value>>;
        let saver = SqliteSaver::<serde_json::Value>::new(&db_path, serializer).unwrap();
        let config = RunnableConfig::default();
        let result = saver.get_tuple(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_tuple_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("empty.db");
        let serializer = Arc::new(crate::memory::serializer::JsonSerializer) as Arc<dyn Serializer<serde_json::Value>>;
        let saver = SqliteSaver::<serde_json::Value>::new(&db_path, serializer).unwrap();
        let config = RunnableConfig {
            thread_id: Some("nonexistent".to_string()),
            ..RunnableConfig::default()
        };
        let result = saver.get_tuple(&config).await.unwrap();
        assert!(result.is_none());
    }
}
