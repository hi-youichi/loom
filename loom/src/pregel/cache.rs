//! Pregel task cache primitives.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::pregel::types::{ChannelName, ChannelValue, TaskId, TaskKind};

/// Cache key used to reuse task writes for identical prepared tasks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskCacheKey {
    pub node_name: String,
    pub step: u64,
    pub input_hash: String,
    pub kind: TaskKind,
    pub thread_id: Option<String>,
    pub checkpoint_ns: String,
}

/// Cached writes captured from a previously successful task.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedTaskWrites {
    pub task_id: TaskId,
    pub writes: Vec<(ChannelName, ChannelValue)>,
}

/// Trait for persistent or in-memory task cache implementations.
pub trait PregelTaskCache: Send + Sync {
    fn get(&self, key: &TaskCacheKey) -> Option<CachedTaskWrites>;
    fn put(&self, key: TaskCacheKey, value: CachedTaskWrites);
    fn clear(&self);
    fn clear_nodes(&self, node_names: &[String]);
}

/// Simple in-memory task cache for tests and local execution.
#[derive(Debug, Default)]
pub struct InMemoryPregelTaskCache {
    inner: Arc<RwLock<HashMap<TaskCacheKey, CachedTaskWrites>>>,
}

impl InMemoryPregelTaskCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all cached entries. Useful for testing/inspection.
    pub fn entries(&self) -> Vec<(TaskCacheKey, CachedTaskWrites)> {
        self.inner
            .read()
            .map(|guard| guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }
}

impl PregelTaskCache for InMemoryPregelTaskCache {
    fn get(&self, key: &TaskCacheKey) -> Option<CachedTaskWrites> {
        self.inner.read().ok()?.get(key).cloned()
    }

    fn put(&self, key: TaskCacheKey, value: CachedTaskWrites) {
        if let Ok(mut guard) = self.inner.write() {
            guard.insert(key, value);
        }
    }

    fn clear(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.clear();
        }
    }

    fn clear_nodes(&self, node_names: &[String]) {
        let names = node_names.iter().collect::<std::collections::HashSet<_>>();
        if let Ok(mut guard) = self.inner.write() {
            guard.retain(|key, _| !names.contains(&key.node_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_cache_can_be_cleared() {
        let cache = InMemoryPregelTaskCache::new();
        let key = TaskCacheKey {
            node_name: "node".to_string(),
            step: 1,
            input_hash: "hash".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread".to_string()),
            checkpoint_ns: String::new(),
        };
        cache.put(
            key.clone(),
            CachedTaskWrites {
                task_id: "task-1".to_string(),
                writes: vec![("out".to_string(), serde_json::json!(1))],
            },
        );
        assert!(cache.get(&key).is_some());

        cache.clear();

        assert!(cache.get(&key).is_none());
        assert!(cache.entries().is_empty());
    }

    #[test]
    fn in_memory_cache_can_clear_selected_nodes() {
        let cache = InMemoryPregelTaskCache::new();
        let keep_key = TaskCacheKey {
            node_name: "keep".to_string(),
            step: 1,
            input_hash: "hash-keep".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread".to_string()),
            checkpoint_ns: String::new(),
        };
        let drop_key = TaskCacheKey {
            node_name: "drop".to_string(),
            step: 1,
            input_hash: "hash-drop".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread".to_string()),
            checkpoint_ns: String::new(),
        };
        cache.put(
            keep_key.clone(),
            CachedTaskWrites {
                task_id: "task-keep".to_string(),
                writes: vec![("out".to_string(), serde_json::json!(1))],
            },
        );
        cache.put(
            drop_key.clone(),
            CachedTaskWrites {
                task_id: "task-drop".to_string(),
                writes: vec![("out".to_string(), serde_json::json!(2))],
            },
        );

        cache.clear_nodes(&["drop".to_string()]);

        assert!(cache.get(&keep_key).is_some());
        assert!(cache.get(&drop_key).is_none());
    }
}
