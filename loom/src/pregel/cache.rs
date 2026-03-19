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
}
