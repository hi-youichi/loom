//! In-memory checkpointer (MemorySaver).
//!
//! In-memory checkpointer. Not persistent; for dev and tests.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::memory::checkpoint::{Checkpoint, CheckpointListItem, CheckpointMetadata};
use crate::memory::checkpointer::{CheckpointError, Checkpointer};
use crate::memory::config::RunnableConfig;

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
        guard
            .by_thread
            .entry(key)
            .or_default()
            .push((id.clone(), cp));
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
                .map(|(_, cp)| (cp.clone(), cp.metadata.clone()))
        } else {
            list.last().map(|(_, cp)| (cp.clone(), cp.metadata.clone()))
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
                metadata: cp.metadata.clone(),
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
}
