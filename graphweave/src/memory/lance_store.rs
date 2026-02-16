//! LanceDB-backed Store (LanceStore). Persistent with vector search.
//!
//! Requires feature `lance`. put/get/list; put embeds value text; search with query uses vector similarity.

use std::path::Path;
use std::sync::Arc;

use arrow_array::types::Float32Type;
use arrow_array::FixedSizeListArray;
use arrow_array::{Array, Float32Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use futures::TryStreamExt;
use lancedb::connection::Connection;
use lancedb::query::ExecutableQuery;
use lancedb::query::QueryBase;

use crate::memory::embedder::Embedder;
use crate::memory::store::{Namespace, Store, StoreError, StoreSearchHit};

const TABLE_NAME: &str = "store";

fn ns_to_key(ns: &Namespace) -> String {
    serde_json::to_string(ns).unwrap_or_else(|_| "[]".to_string())
}

/// Escape single quotes for use in LanceDB SQL predicate (e.g. only_if).
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// Extracts embeddable text from a JSON value: prefer "text" field, else stringify.
fn text_from_value(value: &serde_json::Value) -> String {
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| value.to_string())
}

/// LanceDB-backed Store. Key: (namespace, key). Value stored as JSON text; vector column for semantic search.
///
/// **Interaction**: Used as `Arc<dyn Store>`; nodes use it for cross-thread memory with semantic search.
/// Put embeds value text (or full JSON string) via [`Embedder`]; search with query uses vector similarity.
pub struct LanceStore {
    conn: Connection,
    table_name: String,
    embedder: Arc<dyn Embedder>,
    dimension: usize,
}

impl LanceStore {
    /// Creates or opens a LanceDB store at `path`. If the table does not exist, creates it with schema (ns, key, value, vector).
    pub async fn new(
        path: impl AsRef<Path>,
        embedder: Arc<dyn Embedder>,
    ) -> Result<Self, StoreError> {
        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| StoreError::Storage("path must be valid UTF-8".into()))?;
        let conn = lancedb::connect(path_str)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;

        let dimension = embedder.dimension();
        let table_name = TABLE_NAME.to_string();

        let has_table = conn.open_table(TABLE_NAME).execute().await.is_ok();

        if !has_table {
            let schema = Arc::new(Schema::new(vec![
                Field::new("ns", DataType::Utf8, false),
                Field::new("key", DataType::Utf8, false),
                Field::new("value", DataType::Utf8, false),
                Field::new(
                    "vector",
                    DataType::FixedSizeList(
                        Arc::new(Field::new("item", DataType::Float32, true)),
                        dimension as i32,
                    ),
                    false,
                ),
            ]));
            conn.create_empty_table(&table_name, schema)
                .execute()
                .await
                .map_err(|e| StoreError::Storage(e.to_string()))?;
        }

        Ok(Self {
            conn,
            table_name,
            embedder,
            dimension,
        })
    }

    async fn open_table(&self) -> Result<lancedb::Table, StoreError> {
        self.conn
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))
    }
}

#[async_trait]
impl Store for LanceStore {
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
        let vectors = self.embedder.embed(&[text.as_str()]).await?;
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

        let table = self.open_table().await?;
        let pred_ns = escape_sql(&ns);
        let pred_key = escape_sql(&key);
        let predicate = format!("ns = '{}' AND key = '{}'", pred_ns, pred_key);
        table
            .delete(&predicate)
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("ns", DataType::Utf8, false),
            Field::new("key", DataType::Utf8, false),
            Field::new("value", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    self.dimension as i32,
                ),
                false,
            ),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![ns.as_str()])),
                Arc::new(StringArray::from(vec![key.as_str()])),
                Arc::new(StringArray::from(vec![value_str.as_str()])),
                Arc::new(
                    FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                        std::iter::once(Some(vector.into_iter().map(Some).collect::<Vec<_>>())),
                        self.dimension as i32,
                    ),
                ),
            ],
        )
        .map_err(|e| StoreError::Storage(e.to_string()))?;

        let batch_iter = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);

        table
            .add(batch_iter)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get(
        &self,
        namespace: &Namespace,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        let ns = ns_to_key(namespace);
        let pred_ns = escape_sql(&ns);
        let pred_key = escape_sql(key);
        let predicate = format!("ns = '{}' AND key = '{}'", pred_ns, pred_key);
        let table = self.open_table().await?;
        let stream = table
            .query()
            .only_if(predicate)
            .limit(1)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let batch = match batches.first() {
            Some(b) if b.num_rows() > 0 => b,
            _ => return Ok(None),
        };
        let value_col = batch
            .column_by_name("value")
            .ok_or_else(|| StoreError::Storage("missing value column".into()))?;
        let arr = value_col
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| StoreError::Storage("value column not string".into()))?;
        let s = arr.value(0);
        let value = serde_json::from_str(s).map_err(StoreError::from)?;
        Ok(Some(value))
    }

    async fn list(&self, namespace: &Namespace) -> Result<Vec<String>, StoreError> {
        let ns = ns_to_key(namespace);
        let pred_ns = escape_sql(&ns);
        let predicate = format!("ns = '{}'", pred_ns);
        let table = self.open_table().await?;
        let stream = table
            .query()
            .only_if(predicate)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let mut keys = Vec::new();
        for batch in batches {
            let key_col = batch
                .column_by_name("key")
                .ok_or_else(|| StoreError::Storage("missing key column".into()))?;
            let arr = key_col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| StoreError::Storage("key column not string".into()))?;
            for i in 0..arr.len() {
                keys.push(arr.value(i).to_string());
            }
        }
        Ok(keys)
    }

    async fn search(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreSearchHit>, StoreError> {
        let ns = ns_to_key(namespace);
        let pred_ns = escape_sql(&ns);
        let predicate = format!("ns = '{}'", pred_ns);
        let limit = limit.unwrap_or(100).min(1000);
        let table = self.open_table().await?;

        if let Some(q) = query {
            if !q.is_empty() {
                let vectors = self.embedder.embed(&[q]).await?;
                let query_vec = vectors
                    .into_iter()
                    .next()
                    .ok_or_else(|| StoreError::Storage("embedder returned no vector".into()))?;
                if query_vec.len() != self.dimension {
                    return Err(StoreError::Storage(format!(
                        "embedder dimension {} != expected {}",
                        query_vec.len(),
                        self.dimension
                    )));
                }
                let stream = table
                    .query()
                    .nearest_to(query_vec.as_slice())
                    .map_err(|e| StoreError::Storage(e.to_string()))?
                    .only_if(predicate)
                    .limit(limit)
                    .execute()
                    .await
                    .map_err(|e| StoreError::Storage(e.to_string()))?;
                let batches: Vec<RecordBatch> = stream
                    .try_collect()
                    .await
                    .map_err(|e| StoreError::Storage(e.to_string()))?;
                let mut hits = Vec::new();
                for batch in batches {
                    let key_col = batch
                        .column_by_name("key")
                        .ok_or_else(|| StoreError::Storage("missing key column".into()))?;
                    let value_col = batch
                        .column_by_name("value")
                        .ok_or_else(|| StoreError::Storage("missing value column".into()))?;
                    let key_arr = key_col
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| StoreError::Storage("key column not string".into()))?;
                    let value_arr = value_col
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| StoreError::Storage("value column not string".into()))?;
                    let score_col = batch.column_by_name("_distance");
                    for i in 0..batch.num_rows() {
                        let key = key_arr.value(i).to_string();
                        let value =
                            serde_json::from_str(value_arr.value(i)).map_err(StoreError::from)?;
                        let score = score_col.and_then(|col| {
                            col.as_any()
                                .downcast_ref::<Float32Array>()
                                .map(|arr| arr.value(i) as f64)
                        });
                        hits.push(StoreSearchHit { key, value, score });
                    }
                }
                return Ok(hits);
            }
        }

        let stream = table
            .query()
            .only_if(predicate)
            .limit(limit)
            .execute()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| StoreError::Storage(e.to_string()))?;
        let mut hits = Vec::new();
        for batch in batches {
            let key_col = batch
                .column_by_name("key")
                .ok_or_else(|| StoreError::Storage("missing key column".into()))?;
            let value_col = batch
                .column_by_name("value")
                .ok_or_else(|| StoreError::Storage("missing value column".into()))?;
            let key_arr = key_col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| StoreError::Storage("key column not string".into()))?;
            let value_arr = value_col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| StoreError::Storage("value column not string".into()))?;
            for i in 0..batch.num_rows() {
                let key = key_arr.value(i).to_string();
                let value = serde_json::from_str(value_arr.value(i)).map_err(StoreError::from)?;
                hits.push(StoreSearchHit {
                    key,
                    value,
                    score: None,
                });
            }
        }
        Ok(hits)
    }
}
