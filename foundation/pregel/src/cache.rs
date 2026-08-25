//! Pregel task cache primitives.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::types::{ChannelName, ChannelValue, TaskId, TaskKind};

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

    fn create_test_cache_key() -> TaskCacheKey {
        TaskCacheKey {
            node_name: "test-node".to_string(),
            step: 5,
            input_hash: "hash-123".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread-1".to_string()),
            checkpoint_ns: "root/test".to_string(),
        }
    }

    fn create_test_cached_writes(task_id: &str) -> CachedTaskWrites {
        CachedTaskWrites {
            task_id: task_id.to_string(),
            writes: vec![
                ("channel-1".to_string(), serde_json::json!("value1")),
                ("channel-2".to_string(), serde_json::json!("value2")),
            ],
        }
    }

    #[test]
    fn test_in_memory_pregel_task_cache_new() {
        let cache = InMemoryPregelTaskCache::new();
        let entries = cache.entries();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_in_memory_pregel_task_cache_default() {
        let cache = InMemoryPregelTaskCache::default();
        let entries = cache.entries();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_in_memory_pregel_task_cache_put_and_get() {
        let cache = InMemoryPregelTaskCache::new();
        let key = create_test_cache_key();
        let writes = create_test_cached_writes("task-1");

        cache.put(key.clone(), writes.clone());

        let retrieved = cache.get(&key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), writes);
    }

    #[test]
    fn test_in_memory_pregel_task_cache_get_missing() {
        let cache = InMemoryPregelTaskCache::new();
        let key = create_test_cache_key();

        let retrieved = cache.get(&key);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_in_memory_pregel_task_cache_clear() {
        let cache = InMemoryPregelTaskCache::new();
        let key = create_test_cache_key();
        let writes = create_test_cached_writes("task-1");

        cache.put(key.clone(), writes);
        assert_eq!(cache.entries().len(), 1);

        cache.clear();
        assert_eq!(cache.entries().len(), 0);
    }

    #[test]
    fn test_in_memory_pregel_task_cache_clear_nodes() {
        let cache = InMemoryPregelTaskCache::new();

        let key1 = TaskCacheKey {
            node_name: "node-1".to_string(),
            step: 1,
            input_hash: "hash-1".to_string(),
            kind: TaskKind::Pull,
            thread_id: None,
            checkpoint_ns: "root".to_string(),
        };

        let key2 = TaskCacheKey {
            node_name: "node-2".to_string(),
            step: 1,
            input_hash: "hash-2".to_string(),
            kind: TaskKind::Pull,
            thread_id: None,
            checkpoint_ns: "root".to_string(),
        };

        let key3 = TaskCacheKey {
            node_name: "node-1".to_string(),
            step: 2,
            input_hash: "hash-3".to_string(),
            kind: TaskKind::Pull,
            thread_id: None,
            checkpoint_ns: "root".to_string(),
        };

        cache.put(key1.clone(), create_test_cached_writes("task-1"));
        cache.put(key2.clone(), create_test_cached_writes("task-2"));
        cache.put(key3.clone(), create_test_cached_writes("task-3"));

        assert_eq!(cache.entries().len(), 3);

        cache.clear_nodes(&["node-1".to_string()]);

        let entries = cache.entries();
        assert_eq!(entries.len(), 1);
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key3).is_none());
    }

    #[test]
    fn test_in_memory_pregel_task_cache_entries() {
        let cache = InMemoryPregelTaskCache::new();

        let key1 = create_test_cache_key();
        let writes1 = create_test_cached_writes("task-1");

        let key2 = TaskCacheKey {
            node_name: "node-2".to_string(),
            step: 3,
            input_hash: "hash-456".to_string(),
            kind: TaskKind::Push,
            thread_id: None,
            checkpoint_ns: "root/other".to_string(),
        };
        let writes2 = create_test_cached_writes("task-2");

        cache.put(key1.clone(), writes1.clone());
        cache.put(key2.clone(), writes2.clone());

        let entries = cache.entries();
        assert_eq!(entries.len(), 2);

        let entry1 = entries.iter().find(|(k, _)| k == &key1);
        let entry2 = entries.iter().find(|(k, _)| k == &key2);

        assert!(entry1.is_some());
        assert!(entry2.is_some());
        assert_eq!(entry1.unwrap().1, writes1);
        assert_eq!(entry2.unwrap().1, writes2);
    }

    #[test]
    fn test_in_memory_pregel_task_cache_overwrite() {
        let cache = InMemoryPregelTaskCache::new();
        let key = create_test_cache_key();

        let writes1 = create_test_cached_writes("task-1");
        let writes2 = CachedTaskWrites {
            task_id: "task-2".to_string(),
            writes: vec![("channel-3".to_string(), serde_json::json!("value3"))],
        };

        cache.put(key.clone(), writes1.clone());
        cache.put(key.clone(), writes2.clone());

        let retrieved = cache.get(&key);
        assert_eq!(retrieved, Some(writes2));
        assert_ne!(retrieved, Some(writes1));
    }

    #[test]
    fn test_task_cache_key_equality() {
        let key1 = create_test_cache_key();
        let key2 = create_test_cache_key();
        assert_eq!(key1, key2);

        let key3 = TaskCacheKey {
            node_name: "other-node".to_string(),
            step: 5,
            input_hash: "hash-123".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread-1".to_string()),
            checkpoint_ns: "root/test".to_string(),
        };
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_task_cache_key_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();

        let key1 = create_test_cache_key();
        let key2 = TaskCacheKey {
            node_name: "node-2".to_string(),
            step: 5,
            input_hash: "hash-123".to_string(),
            kind: TaskKind::Pull,
            thread_id: Some("thread-1".to_string()),
            checkpoint_ns: "root/test".to_string(),
        };

        set.insert(key1.clone());
        set.insert(key2.clone());
        set.insert(key1);

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_cached_task_writes_fields() {
        let writes = create_test_cached_writes("task-123");
        assert_eq!(writes.task_id, "task-123");
        assert_eq!(writes.writes.len(), 2);
        assert_eq!(writes.writes[0].0, "channel-1");
        assert_eq!(writes.writes[0].1, serde_json::json!("value1"));
    }

    #[test]
    fn test_cached_task_writes_clone() {
        let writes = create_test_cached_writes("task-1");
        let cloned = writes.clone();
        assert_eq!(writes, cloned);
    }

    #[test]
    fn test_in_memory_pregel_task_cache_debug() {
        let cache = InMemoryPregelTaskCache::new();
        let debug_str = format!("{:?}", cache);
        assert!(debug_str.contains("InMemoryPregelTaskCache"));
    }
}
