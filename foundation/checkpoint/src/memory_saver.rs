//! In-memory checkpointer (MemorySaver).
//!
//! In-memory checkpointer. Not persistent; for dev and tests.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::RunnableConfig;
use crate::{Checkpoint, CheckpointListItem, CheckpointMetadata};
use crate::{CheckpointError, Checkpointer, PendingWrite};

/// One stored task write entry.
#[derive(Debug, Clone)]
struct WriteEntry {
    task_id: String,
    idx: i64,
    channel: String,
    value: serde_json::Value,
}

/// In-memory checkpointer. Key: (thread_id, checkpoint_ns); each thread has a list of checkpoints.
///
/// In-memory checkpointer. Not persistent; for dev and tests.
///
/// **Interaction**: Used as `Arc<dyn Checkpointer<S>>` in StateGraph::compile_with_checkpointer.
pub struct MemorySaver<S> {
    inner: Arc<RwLock<MemorySaverInner<S>>>,
}

struct MemorySaverInner<S> {
    /// Key: format!("{}:{}", thread_id, checkpoint_ns). Value: list of (checkpoint_id, checkpoint) newest last.
    by_thread: HashMap<String, Vec<(String, Checkpoint<S>)>>,
    /// Key: format!("{}:{}:{}", thread_id, checkpoint_ns, checkpoint_id).
    /// Value: pending writes accumulated for that checkpoint.
    writes: HashMap<String, Vec<WriteEntry>>,
    next_id: u64,
}

impl<S> MemorySaver<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a new in-memory checkpointer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemorySaverInner {
                by_thread: HashMap::new(),
                writes: HashMap::new(),
                next_id: 0,
            })),
        }
    }

    fn thread_key(config: &RunnableConfig) -> Result<String, CheckpointError> {
        let thread_id = config
            .thread_id
            .as_deref()
            .ok_or(CheckpointError::ThreadIdRequired)?;
        Ok(format!("{}:{}", thread_id, config.checkpoint_ns))
    }

    fn writes_key(config: &RunnableConfig, checkpoint_id: &str) -> Result<String, CheckpointError> {
        let thread_id = config
            .thread_id
            .as_deref()
            .ok_or(CheckpointError::ThreadIdRequired)?;
        Ok(format!(
            "{}:{}:{}",
            thread_id, config.checkpoint_ns, checkpoint_id
        ))
    }
}

impl<S> Default for MemorySaver<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<S> Checkpointer<S> for MemorySaver<S>
where
    S: Clone + Send + Sync + 'static,
{
    async fn put(
        &self,
        config: &RunnableConfig,
        checkpoint: &Checkpoint<S>,
    ) -> Result<String, CheckpointError> {
        let key = Self::thread_key(config)?;
        let id = checkpoint.id.clone();
        let cp = checkpoint.clone();
        let mut guard = self.inner.write().await;
        let next_id = guard.next_id;
        guard.next_id = next_id.wrapping_add(1);
        let entries = guard.by_thread.entry(key).or_default();
        if let Some(existing) = entries
            .iter_mut()
            .find(|(existing_id, _)| existing_id == &id)
        {
            *existing = (id.clone(), cp);
        } else {
            entries.push((id.clone(), cp));
        }
        Ok(id)
    }

    async fn get_tuple(
        &self,
        config: &RunnableConfig,
    ) -> Result<Option<(Checkpoint<S>, CheckpointMetadata)>, CheckpointError> {
        let key = Self::thread_key(config)?;
        let guard = self.inner.read().await;
        let list = match guard.by_thread.get(&key) {
            Some(l) if !l.is_empty() => l,
            _ => return Ok(None),
        };
        let result = if let Some(cid) = &config.checkpoint_id {
            list.iter()
                .find(|(id, _)| id == cid)
                .map(|(_, cp)| (cp.clone(), cp.kernel.clone()))
        } else {
            list.last().map(|(_, cp)| (cp.clone(), cp.kernel.clone()))
        };
        Ok(result)
    }

    async fn list(
        &self,
        config: &RunnableConfig,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Vec<CheckpointListItem>, CheckpointError> {
        let key = Self::thread_key(config)?;
        let guard = self.inner.read().await;
        let list = match guard.by_thread.get(&key) {
            Some(l) => l,
            None => return Ok(Vec::new()),
        };
        let mut items: Vec<CheckpointListItem> = list
            .iter()
            .map(|(id, cp)| CheckpointListItem {
                checkpoint_id: id.clone(),
                metadata: cp.kernel.clone(),
            })
            .collect();
        if let Some(a) = after {
            if let Some(pos) = items.iter().position(|i| i.checkpoint_id.as_str() == a) {
                items = items[pos + 1..].to_vec();
            }
        }
        if let Some(b) = before {
            if let Some(pos) = items.iter().position(|i| i.checkpoint_id.as_str() == b) {
                items = items[..pos].to_vec();
            }
        }
        if let Some(n) = limit {
            let len = items.len();
            if len > n {
                items = items[len - n..].to_vec();
            }
        }
        Ok(items)
    }

    async fn put_writes(
        &self,
        config: &RunnableConfig,
        checkpoint_id: &str,
        task_id: &str,
        writes: &[(String, serde_json::Value)],
    ) -> Result<(), CheckpointError> {
        let key = Self::writes_key(config, checkpoint_id)?;
        let mut guard = self.inner.write().await;
        let entries = guard.writes.entry(key).or_default();
        for (idx, (channel, value)) in writes.iter().enumerate() {
            let idx = idx as i64;
            // Mirror SQLite INSERT OR IGNORE: skip rows whose (task_id, idx)
            // already exists so resumed runs do not duplicate writes.
            if entries.iter().any(|e| e.task_id == task_id && e.idx == idx) {
                continue;
            }
            entries.push(WriteEntry {
                task_id: task_id.to_string(),
                idx,
                channel: channel.clone(),
                value: value.clone(),
            });
        }
        Ok(())
    }

    async fn get_writes(
        &self,
        config: &RunnableConfig,
        checkpoint_id: &str,
    ) -> Result<Vec<PendingWrite>, CheckpointError> {
        let key = Self::writes_key(config, checkpoint_id)?;
        let guard = self.inner.read().await;
        let mut entries: Vec<WriteEntry> = guard.writes.get(&key).cloned().unwrap_or_default();
        entries.sort_by(|a, b| a.task_id.cmp(&b.task_id).then_with(|| a.idx.cmp(&b.idx)));
        Ok(entries
            .into_iter()
            .map(|e| (e.task_id, e.channel, e.value))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Checkpoint, CheckpointSource, KernelMetadata, CHECKPOINT_VERSION};

    #[tokio::test(flavor = "current_thread")]
    async fn put_replaces_existing_checkpoint_id_instead_of_appending() {
        let saver = MemorySaver::<serde_json::Value>::new();
        let config = RunnableConfig {
            thread_id: Some("thread-memory-replace".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let checkpoint = Checkpoint {
            v: CHECKPOINT_VERSION,
            id: "ck-1".to_string(),
            ts: "1".to_string(),
            channel_values: serde_json::json!({"value": 1}),
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            pending_writes: Vec::new(),
            pending_interrupts: Vec::new(),
            user: (),
            kernel: KernelMetadata {
                source: CheckpointSource::Loop,
                step: 1,
                created_at: None,
                parents: HashMap::new(),
                children: HashMap::new(),
                summary: None,
            },
        };
        saver.put(&config, &checkpoint).await.unwrap();

        let mut updated = checkpoint.clone();
        updated.channel_values = serde_json::json!({"value": 2});
        updated
            .kernel
            .children
            .insert("fork".to_string(), vec!["child-1".to_string()]);
        saver.put(&config, &updated).await.unwrap();

        let history = saver.list(&config, None, None, None).await.unwrap();
        assert_eq!(history.len(), 1);

        let latest = saver.get_tuple(&config).await.unwrap().unwrap().0;
        assert_eq!(latest.channel_values["value"], serde_json::json!(2));
        assert_eq!(
            latest.kernel.children.get("fork"),
            Some(&vec!["child-1".to_string()])
        );
    }

    /// **Scenario**: put_writes then get_writes returns entries ordered by (task_id, idx).
    #[tokio::test(flavor = "current_thread")]
    async fn put_writes_roundtrip_and_ordering() {
        let saver = MemorySaver::<serde_json::Value>::new();
        let config = RunnableConfig {
            thread_id: Some("thread-writes-order".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        // task-a has two channel writes (idx 0, 1), task-b has one write (idx 0).
        // Insertion order is interleaved; get_writes must reorder by (task_id, idx).
        saver
            .put_writes(
                &config,
                "ck-1",
                "task-b",
                &[("channel-1".to_string(), serde_json::json!("b1"))],
            )
            .await
            .unwrap();
        saver
            .put_writes(
                &config,
                "ck-1",
                "task-a",
                &[
                    ("channel-1".to_string(), serde_json::json!(1)),
                    ("channel-2".to_string(), serde_json::json!("a2")),
                ],
            )
            .await
            .unwrap();

        let writes = saver.get_writes(&config, "ck-1").await.unwrap();
        let summary: Vec<(String, String)> = writes
            .iter()
            .map(|(t, c, _)| (t.clone(), c.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("task-a".to_string(), "channel-1".to_string()),
                ("task-a".to_string(), "channel-2".to_string()),
                ("task-b".to_string(), "channel-1".to_string()),
            ]
        );
        // Values should be intact (preserved across the roundtrip)
        assert_eq!(writes[0].2, serde_json::json!(1));
        assert_eq!(writes[1].2, serde_json::json!("a2"));
        assert_eq!(writes[2].2, serde_json::json!("b1"));
    }

    /// **Scenario**: Re-inserting the same (task_id, idx) does not duplicate entries.
    #[tokio::test(flavor = "current_thread")]
    async fn put_writes_is_idempotent_on_task_id_idx() {
        let saver = MemorySaver::<serde_json::Value>::new();
        let config = RunnableConfig {
            thread_id: Some("thread-writes-idem".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        saver
            .put_writes(
                &config,
                "ck-1",
                "task-a",
                &[("ch".to_string(), serde_json::json!("first"))],
            )
            .await
            .unwrap();
        saver
            .put_writes(
                &config,
                "ck-1",
                "task-a",
                &[("ch".to_string(), serde_json::json!("second"))],
            )
            .await
            .unwrap();

        let writes = saver.get_writes(&config, "ck-1").await.unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].2, serde_json::json!("first"));
    }

    /// **Scenario**: Writes are isolated by thread_id, checkpoint_ns, and checkpoint_id.
    #[tokio::test(flavor = "current_thread")]
    async fn put_writes_isolated_by_thread_ns_checkpoint() {
        let saver = MemorySaver::<serde_json::Value>::new();
        let config_a = RunnableConfig {
            thread_id: Some("thread-a".to_string()),
            checkpoint_ns: "ns-a".to_string(),
            ..Default::default()
        };
        let config_b = RunnableConfig {
            thread_id: Some("thread-b".to_string()),
            checkpoint_ns: "ns-a".to_string(),
            ..Default::default()
        };
        let config_a_ns_b = RunnableConfig {
            thread_id: Some("thread-a".to_string()),
            checkpoint_ns: "ns-b".to_string(),
            ..Default::default()
        };
        let config_a_ck2 = RunnableConfig {
            thread_id: Some("thread-a".to_string()),
            checkpoint_ns: "ns-a".to_string(),
            ..Default::default()
        };

        saver
            .put_writes(
                &config_a,
                "ck-1",
                "task",
                &[("ch".to_string(), serde_json::json!("a/ck-1"))],
            )
            .await
            .unwrap();
        saver
            .put_writes(
                &config_b,
                "ck-1",
                "task",
                &[("ch".to_string(), serde_json::json!("b/ck-1"))],
            )
            .await
            .unwrap();
        saver
            .put_writes(
                &config_a_ns_b,
                "ck-1",
                "task",
                &[("ch".to_string(), serde_json::json!("a/ns-b/ck-1"))],
            )
            .await
            .unwrap();
        saver
            .put_writes(
                &config_a_ck2,
                "ck-2",
                "task",
                &[("ch".to_string(), serde_json::json!("a/ck-2"))],
            )
            .await
            .unwrap();

        let a_ck1 = saver.get_writes(&config_a, "ck-1").await.unwrap();
        let b_ck1 = saver.get_writes(&config_b, "ck-1").await.unwrap();
        let a_ns_b_ck1 = saver.get_writes(&config_a_ns_b, "ck-1").await.unwrap();
        let a_ck2 = saver.get_writes(&config_a_ck2, "ck-2").await.unwrap();
        let a_ck1_missing = saver.get_writes(&config_a, "ck-missing").await.unwrap();

        assert_eq!(a_ck1.len(), 1);
        assert_eq!(a_ck1[0].2, serde_json::json!("a/ck-1"));
        assert_eq!(b_ck1.len(), 1);
        assert_eq!(b_ck1[0].2, serde_json::json!("b/ck-1"));
        assert_eq!(a_ns_b_ck1.len(), 1);
        assert_eq!(a_ns_b_ck1[0].2, serde_json::json!("a/ns-b/ck-1"));
        assert_eq!(a_ck2.len(), 1);
        assert_eq!(a_ck2[0].2, serde_json::json!("a/ck-2"));
        assert!(a_ck1_missing.is_empty());
    }

    /// **Scenario**: get_writes for a never-touched checkpoint returns empty Vec.
    #[tokio::test(flavor = "current_thread")]
    async fn get_writes_empty_when_no_writes() {
        let saver = MemorySaver::<serde_json::Value>::new();
        let config = RunnableConfig {
            thread_id: Some("thread-empty".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };
        let writes = saver.get_writes(&config, "ck-missing").await.unwrap();
        assert!(writes.is_empty());
    }
}
