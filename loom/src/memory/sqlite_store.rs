//! SQLite-backed Store (SqliteStore). Persistent across process restarts.
//!
//! Aligns with 16-memory-design §5.2.2. put/get/list; search is key/value filter (no semantic index).

use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rusqlite::params;

use crate::memory::store::{
    Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType, SearchItem,
    SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};

fn ns_to_key(ns: &Namespace) -> String {
    serde_json::to_string(ns).unwrap_or_else(|_| "[]".to_string())
}

fn key_to_ns(key: &str) -> Namespace {
    serde_json::from_str(key).unwrap_or_default()
}

fn millis_to_system_time(millis: i64) -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_millis(millis as u64)
}

fn system_time_to_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// SQLite-backed Store. Key: (namespace, key). Value stored as JSON text.
///
/// Persistent; for single-node and dev. Uses spawn_blocking for async.
///
/// **Interaction**: Used as `Arc<dyn Store>` when graph is compiled with store; nodes use it for cross-thread memory.
pub struct SqliteStore {
    db_path: std::path::PathBuf,
}

impl SqliteStore {
    /// Creates a new SQLite store and ensures the table exists.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let db_path = path.as_ref().to_path_buf();
        let conn =
            rusqlite::Connection::open(&db_path).map_err(|e| StoreError::Storage(e.to_string()))?;
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS store_kv (
                ns TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (ns, key)
            )
            "#,
            [],
        )
        .map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(Self { db_path })
    }

    /// Checks if a namespace matches a condition.
    fn matches_condition(namespace: &Namespace, condition: &MatchCondition) -> bool {
        let path = &condition.path;

        match condition.match_type {
            NamespaceMatchType::Prefix => {
                if namespace.len() < path.len() {
                    return false;
                }
                for (i, p) in path.iter().enumerate() {
                    if p != "*" && namespace.get(i) != Some(p) {
                        return false;
                    }
                }
                true
            }
            NamespaceMatchType::Suffix => {
                if namespace.len() < path.len() {
                    return false;
                }
                let start = namespace.len() - path.len();
                for (i, p) in path.iter().enumerate() {
                    if p != "*" && namespace.get(start + i) != Some(p) {
                        return false;
                    }
                }
                true
            }
        }
    }
}

#[async_trait]
impl Store for SqliteStore {
    async fn put(
        &self,
        namespace: &Namespace,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let ns = ns_to_key(namespace);
        let key = key.to_string();
        let value_str = serde_json::to_string(value)?;
        let db_path = self.db_path.clone();
        let now = system_time_to_millis(SystemTime::now());

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;

            // Check if exists to preserve created_at
            let mut stmt = conn
                .prepare("SELECT created_at FROM store_kv WHERE ns = ?1 AND key = ?2")
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let existing_created: Option<i64> = stmt
                .query_row(params![ns, key], |row| row.get(0))
                .ok();
            let created_at = existing_created.unwrap_or(now);

            conn.execute(
                "INSERT OR REPLACE INTO store_kv (ns, key, value, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ns, key, value_str, created_at, now],
            )
            .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok::<(), StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))?
    }

    async fn get(
        &self,
        namespace: &Namespace,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        let ns = ns_to_key(namespace);
        let key = key.to_string();
        let db_path = self.db_path.clone();

        let value_str_opt = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut stmt = conn
                .prepare("SELECT value FROM store_kv WHERE ns = ?1 AND key = ?2")
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut rows = stmt
                .query(params![ns, key])
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let row = match rows
                .next()
                .map_err(|e| StoreError::Storage(e.to_string()))?
            {
                Some(r) => r,
                None => return Ok::<_, StoreError>(None),
            };
            let value_str: String = row.get(0).map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok(Some(value_str))
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))??;

        let value_str = match value_str_opt {
            Some(s) => s,
            None => return Ok(None),
        };
        let value = serde_json::from_str(&value_str)?;
        Ok(Some(value))
    }

    async fn get_item(&self, namespace: &Namespace, key: &str) -> Result<Option<Item>, StoreError> {
        let ns_str = ns_to_key(namespace);
        let ns_clone = namespace.clone();
        let key = key.to_string();
        let db_path = self.db_path.clone();

        let result = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT value, created_at, updated_at FROM store_kv WHERE ns = ?1 AND key = ?2",
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut rows = stmt
                .query(params![ns_str, key])
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let row = match rows
                .next()
                .map_err(|e| StoreError::Storage(e.to_string()))?
            {
                Some(r) => r,
                None => return Ok::<_, StoreError>(None),
            };
            let value_str: String = row.get(0).map_err(|e| StoreError::Storage(e.to_string()))?;
            let created_at: i64 = row.get(1).map_err(|e| StoreError::Storage(e.to_string()))?;
            let updated_at: i64 = row.get(2).map_err(|e| StoreError::Storage(e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&value_str)?;

            Ok(Some(Item::with_timestamps(
                ns_clone,
                key,
                value,
                millis_to_system_time(created_at),
                millis_to_system_time(updated_at),
            )))
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))??;

        Ok(result)
    }

    async fn delete(&self, namespace: &Namespace, key: &str) -> Result<(), StoreError> {
        let ns = ns_to_key(namespace);
        let key = key.to_string();
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            conn.execute(
                "DELETE FROM store_kv WHERE ns = ?1 AND key = ?2",
                params![ns, key],
            )
            .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok::<(), StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))?
    }

    async fn list(&self, namespace: &Namespace) -> Result<Vec<String>, StoreError> {
        let ns = ns_to_key(namespace);
        let db_path = self.db_path.clone();

        let keys = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut stmt = conn
                .prepare("SELECT key FROM store_kv WHERE ns = ?1 ORDER BY key")
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map(params![ns], |row| row.get(0))
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let keys: Vec<String> = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok::<Vec<String>, StoreError>(keys)
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))??;

        Ok(keys)
    }

    async fn search(
        &self,
        namespace_prefix: &Namespace,
        options: SearchOptions,
    ) -> Result<Vec<SearchItem>, StoreError> {
        let ns_prefix = ns_to_key(namespace_prefix);
        let query = options.query.clone();
        let db_path = self.db_path.clone();

        let mut hits = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            // For prefix matching, we use LIKE with the JSON-serialized namespace prefix
            // This is a simplified approach; in production you might use a more sophisticated method
            let mut stmt = conn
                .prepare(
                    "SELECT ns, key, value, created_at, updated_at FROM store_kv WHERE ns LIKE ?1",
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let like_pattern = format!("{}%", ns_prefix.trim_end_matches(']'));
            let rows = stmt
                .query_map(params![like_pattern], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut hits: Vec<SearchItem> = Vec::new();
            for row in rows {
                let (ns_str, key, value_str, created_at, updated_at) =
                    row.map_err(|e| StoreError::Storage(e.to_string()))?;
                let value: serde_json::Value = serde_json::from_str(&value_str)?;
                let namespace = key_to_ns(&ns_str);
                let item = Item::with_timestamps(
                    namespace,
                    key,
                    value,
                    millis_to_system_time(created_at),
                    millis_to_system_time(updated_at),
                );
                hits.push(SearchItem::from_item(item));
            }
            Ok::<Vec<SearchItem>, StoreError>(hits)
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))??;

        // Apply query filter
        if let Some(q) = &query {
            if !q.is_empty() {
                let q_lower = q.to_lowercase();
                hits.retain(|h| {
                    h.item.key.to_lowercase().contains(&q_lower)
                        || h.item.value.to_string().to_lowercase().contains(&q_lower)
                });
            }
        }

        // Apply offset and limit
        if options.offset > 0 {
            if options.offset >= hits.len() {
                hits.clear();
            } else {
                hits = hits.into_iter().skip(options.offset).collect();
            }
        }
        hits.truncate(options.limit);

        Ok(hits)
    }

    async fn list_namespaces(
        &self,
        options: ListNamespacesOptions,
    ) -> Result<Vec<Namespace>, StoreError> {
        let db_path = self.db_path.clone();

        let all_ns = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut stmt = conn
                .prepare("SELECT DISTINCT ns FROM store_kv")
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let namespaces: Vec<Namespace> = rows
                .filter_map(|r| r.ok())
                .map(|ns_str| key_to_ns(&ns_str))
                .collect();
            Ok::<Vec<Namespace>, StoreError>(namespaces)
        })
        .await
        .map_err(|e| StoreError::Storage(e.to_string()))??;

        // Apply match conditions
        let mut namespaces: HashSet<Namespace> = all_ns.into_iter().collect();
        if !options.match_conditions.is_empty() {
            namespaces.retain(|ns| {
                options
                    .match_conditions
                    .iter()
                    .all(|cond| Self::matches_condition(ns, cond))
            });
        }

        // Apply max_depth
        let mut result: Vec<Namespace> = if let Some(max_depth) = options.max_depth {
            namespaces
                .into_iter()
                .map(|ns| {
                    if ns.len() > max_depth {
                        ns.into_iter().take(max_depth).collect()
                    } else {
                        ns
                    }
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect()
        } else {
            namespaces.into_iter().collect()
        };

        // Sort for deterministic output
        result.sort();

        // Apply offset and limit
        if options.offset > 0 {
            if options.offset >= result.len() {
                result.clear();
            } else {
                result = result.into_iter().skip(options.offset).collect();
            }
        }
        result.truncate(options.limit);

        Ok(result)
    }

    async fn batch(&self, ops: Vec<StoreOp>) -> Result<Vec<StoreOpResult>, StoreError> {
        let mut results = Vec::with_capacity(ops.len());

        for op in ops {
            let result = match op {
                StoreOp::Get { namespace, key } => {
                    let item = self.get_item(&namespace, &key).await?;
                    StoreOpResult::Get(item)
                }
                StoreOp::Put {
                    namespace,
                    key,
                    value,
                } => {
                    if let Some(v) = value {
                        self.put(&namespace, &key, &v).await?;
                    } else {
                        self.delete(&namespace, &key).await?;
                    }
                    StoreOpResult::Put
                }
                StoreOp::Search {
                    namespace_prefix,
                    options,
                } => {
                    let items = self.search(&namespace_prefix, options).await?;
                    StoreOpResult::Search(items)
                }
                StoreOp::ListNamespaces { options } => {
                    let ns = self.list_namespaces(options).await?;
                    StoreOpResult::ListNamespaces(ns)
                }
            };
            results.push(result);
        }

        Ok(results)
    }

    async fn search_simple(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreSearchHit>, StoreError> {
        let options = SearchOptions {
            query: query.map(String::from),
            filter: None,
            limit: limit.unwrap_or(10),
            offset: 0,
        };
        let results = self.search(namespace, options).await?;
        Ok(results
            .into_iter()
            .map(|si| StoreSearchHit {
                key: si.item.key,
                value: si.item.value,
                score: si.score,
            })
            .collect())
    }
}
