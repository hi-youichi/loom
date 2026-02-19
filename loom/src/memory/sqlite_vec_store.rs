//! SQLite-backed Store with vector search (SqliteVecStore). Persistent with semantic search via sqlite-vec.
//!
//! Uses dual-table design: store_vec_meta for metadata (ns, key, value),
//! vec0 virtual table for embeddings. Search with query uses KNN vector similarity.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rusqlite::params;

use crate::memory::embedder::Embedder;
use crate::memory::store::{
    Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType, SearchItem,
    SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};

static SQLITE_VEC_INIT: Once = Once::new();

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

/// Formats a Vec<f32> as JSON for sqlite-vec (e.g. "[0.1, 0.2, 0.3]").
fn vector_to_json(v: &[f32]) -> String {
    let parts: Vec<String> = v.iter().map(|f| f.to_string()).collect();
    format!("[{}]", parts.join(","))
}

/// Extracts embeddable text from a JSON value: prefer "text" field, else stringify.
fn text_from_value(value: &serde_json::Value) -> String {
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| value.to_string())
}

/// SQLite-backed Store with vector search. Key: (namespace, key). Value stored as JSON; embeddings in vec0.
///
/// **Interaction**: Used as `Arc<dyn Store>`; nodes use it for cross-thread memory with semantic search.
/// Put embeds value text via [`Embedder`]; search with query uses KNN vector similarity.
pub struct SqliteVecStore {
    db_path: std::path::PathBuf,
    embedder: std::sync::Arc<dyn Embedder>,
    dimension: usize,
    vec_table: String,
}

impl SqliteVecStore {
    /// Creates a new SQLite vector store. Registers sqlite-vec extension and creates tables if needed.
    pub fn new(
        path: impl AsRef<Path>,
        embedder: std::sync::Arc<dyn Embedder>,
    ) -> Result<Self, StoreError> {
        SQLITE_VEC_INIT.call_once(|| unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        });

        let db_path = path.as_ref().to_path_buf();
        let dimension = embedder.dimension();
        let vec_table = "store_vec_embeddings".to_string();

        let conn =
            rusqlite::Connection::open(&db_path).map_err(|e| StoreError::Storage(e.to_string()))?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS store_vec_meta (
                id INTEGER PRIMARY KEY,
                ns TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ns, key)
            )
            "#,
            [],
        )
        .map_err(|e| StoreError::Storage(e.to_string()))?;

        let create_vec_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {} USING vec0(embedding float[{}])",
            vec_table, dimension
        );
        conn.execute(&create_vec_sql, [])
            .map_err(|e| StoreError::Storage(e.to_string()))?;

        Ok(Self {
            db_path,
            embedder,
            dimension,
            vec_table,
        })
    }

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
impl Store for SqliteVecStore {
    async fn put(
        &self,
        namespace: &Namespace,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let ns = ns_to_key(namespace);
        let key = key.to_string();
        let value_str = serde_json::to_string(value)?;
        let text = text_from_value(value);
        let vectors = self.embedder.embed(&[&text]).await?;
        let vector = vectors
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::Storage("embedder returned no vector".into()))?;
        if vector.len() != self.dimension {
            return Err(StoreError::Storage(format!(
                "embedder dimension {} != expected {}",
                vector.len(),
                self.dimension
            )));
        }
        let vec_json = vector_to_json(&vector);
        let db_path = self.db_path.clone();
        let vec_table = self.vec_table.clone();
        let now = system_time_to_millis(SystemTime::now());

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;

            let existing: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT id, created_at FROM store_vec_meta WHERE ns = ?1 AND key = ?2",
                    params![ns, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();

            let (id, _created_at) = match existing {
                Some((id, created)) => {
                    conn.execute("DELETE FROM store_vec_embeddings WHERE rowid = ?1", params![id])
                        .map_err(|e| StoreError::Storage(e.to_string()))?;
                    conn.execute(
                        "UPDATE store_vec_meta SET value = ?1, updated_at = ?2 WHERE id = ?3",
                        params![value_str, now, id],
                    )
                    .map_err(|e| StoreError::Storage(e.to_string()))?;
                    (id, created)
                }
                None => {
                    conn.execute(
                        "INSERT INTO store_vec_meta (ns, key, value, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![ns, key, value_str, now, now],
                    )
                    .map_err(|e| StoreError::Storage(e.to_string()))?;
                    let id = conn.last_insert_rowid();
                    (id, now)
                }
            };

            conn.execute(
                &format!("INSERT INTO {} (rowid, embedding) VALUES (?1, ?2)", vec_table),
                params![id, vec_json],
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
                .prepare("SELECT value FROM store_vec_meta WHERE ns = ?1 AND key = ?2")
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

        match value_str_opt {
            Some(s) => {
                let value = serde_json::from_str(&s)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
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
                    "SELECT value, created_at, updated_at FROM store_vec_meta WHERE ns = ?1 AND key = ?2",
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut rows = stmt
                .query(params![ns_str, key])
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let row = match rows.next().map_err(|e| StoreError::Storage(e.to_string()))? {
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
        let vec_table = self.vec_table.clone();

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM store_vec_meta WHERE ns = ?1 AND key = ?2",
                    params![ns, key],
                    |row| row.get(0),
                )
                .ok();
            if let Some(id) = id {
                conn.execute(
                    &format!("DELETE FROM {} WHERE rowid = ?1", vec_table),
                    params![id],
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
                conn.execute("DELETE FROM store_vec_meta WHERE id = ?1", params![id])
                    .map_err(|e| StoreError::Storage(e.to_string()))?;
            }
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
                .prepare("SELECT key FROM store_vec_meta WHERE ns = ?1 ORDER BY key")
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
        let limit = options.limit.min(1000);
        let ns_prefix = ns_to_key(namespace_prefix);
        let like_pattern = format!("{}%", ns_prefix.trim_end_matches(']'));
        let query = options.query.clone();
        let db_path = self.db_path.clone();
        let vec_table = self.vec_table.clone();
        let embedder = self.embedder.clone();
        let dimension = self.dimension;

        if let Some(ref q) = query {
            if !q.is_empty() {
                let vectors = embedder.embed(&[q]).await?;
                let query_vec = vectors
                    .into_iter()
                    .next()
                    .ok_or_else(|| StoreError::EmbeddingError("No vector returned".into()))?;
                if query_vec.len() != dimension {
                    return Err(StoreError::Storage(format!(
                        "embedder dimension {} != expected {}",
                        query_vec.len(),
                        dimension
                    )));
                }
                let vec_json = vector_to_json(&query_vec);
                let knn_limit = (limit + options.offset).max(50) * 3;

                let hits = tokio::task::spawn_blocking(move || {
                    let conn = rusqlite::Connection::open(&db_path)
                        .map_err(|e| StoreError::Storage(e.to_string()))?;

                    let knn_sql = format!(
                        "SELECT rowid, distance FROM {} WHERE embedding MATCH ?1 AND k = ?2",
                        vec_table
                    );
                    let mut stmt = conn
                        .prepare(&knn_sql)
                        .map_err(|e| StoreError::Storage(e.to_string()))?;
                    let rows = stmt
                        .query_map(params![vec_json, knn_limit as i64], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
                        })
                        .map_err(|e| StoreError::Storage(e.to_string()))?;

                    let rowids_with_dist: Vec<(i64, f64)> = rows
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| StoreError::Storage(e.to_string()))?;

                    if rowids_with_dist.is_empty() {
                        return Ok::<Vec<SearchItem>, StoreError>(Vec::new());
                    }

                    let ids: Vec<i64> = rowids_with_dist.iter().map(|(id, _)| *id).collect();
                    let dist_map: std::collections::HashMap<i64, f64> =
                        rowids_with_dist.into_iter().collect();
                    let ns_prefix_trimmed = like_pattern.trim_end_matches('%');

                    let metas: Vec<(i64, String, String, String, i64, i64)> = if ids.is_empty() {
                        Vec::new()
                    } else {
                        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                        let meta_sql = format!(
                            "SELECT id, ns, key, value, created_at, updated_at FROM store_vec_meta WHERE id IN ({})",
                            placeholders
                        );
                        let mut stmt = conn
                            .prepare(&meta_sql)
                            .map_err(|e| StoreError::Storage(e.to_string()))?;
                        let rows = stmt
                            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                                Ok((
                                    row.get(0)?,
                                    row.get(1)?,
                                    row.get(2)?,
                                    row.get(3)?,
                                    row.get(4)?,
                                    row.get(5)?,
                                ))
                            })
                            .map_err(|e| StoreError::Storage(e.to_string()))?;
                        rows.collect::<Result<Vec<_>, _>>()
                            .map_err(|e| StoreError::Storage(e.to_string()))?
                    };

                    let mut hits: Vec<SearchItem> = metas
                        .into_iter()
                        .filter(|(_, ns_str, ..)| ns_str.starts_with(ns_prefix_trimmed) || ns_str == ns_prefix_trimmed)
                        .filter_map(|(id, ns_str, key, value_str, created_at, updated_at)| {
                            let dist = dist_map.get(&id).copied()?;
                            let value: serde_json::Value =
                                serde_json::from_str(&value_str).ok()?;
                            let score = 1.0 / (1.0 + dist);
                            let item = Item::with_timestamps(
                                key_to_ns(&ns_str),
                                key,
                                value,
                                millis_to_system_time(created_at),
                                millis_to_system_time(updated_at),
                            );
                            Some(SearchItem::with_score(item, score))
                        })
                        .collect();

                    hits.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    Ok::<Vec<SearchItem>, StoreError>(hits)
                })
                .await
                .map_err(|e| StoreError::Storage(e.to_string()))??;

                let hits = hits.into_iter().skip(options.offset).take(limit).collect();
                return Ok(hits);
            }
        }

        let hits = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT ns, key, value, created_at, updated_at FROM store_vec_meta WHERE ns LIKE ?1 ORDER BY key LIMIT ?2 OFFSET ?3",
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map(
                    params![like_pattern, (limit + options.offset) as i64, options.offset as i64],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let mut hits: Vec<SearchItem> = Vec::new();
            for row in rows {
                let (ns_str, key, value_str, created_at, updated_at) =
                    row.map_err(|e| StoreError::Storage(e.to_string()))?;
                let value: serde_json::Value = serde_json::from_str(&value_str)?;
                let item = Item::with_timestamps(
                    key_to_ns(&ns_str),
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
                .prepare("SELECT DISTINCT ns FROM store_vec_meta")
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

        let mut namespaces: HashSet<Namespace> = all_ns.into_iter().collect();
        if !options.match_conditions.is_empty() {
            namespaces.retain(|ns| {
                options
                    .match_conditions
                    .iter()
                    .all(|cond| Self::matches_condition(ns, cond))
            });
        }

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
        result.sort();
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
