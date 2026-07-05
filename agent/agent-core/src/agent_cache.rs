//! Agent instance cache (priority #21 gap, Hermes parity `gateway/run.py`).
//!
//! Hermes' `_enforce_agent_cache_cap` + `_session_expiry_watcher` keep a
//! bounded LRU of agent handles and evict entries idle longer than an
//! expiry threshold. Loom today rebuilds the agent handle per turn in
//! `agent_core::run::config_builder::build_agent`, which pays the
//! cold-start cost (skill registry load, config evaluation, MCP
//! discovery) every time. Under load, that dominates.
//!
//! This module provides:
//!   - `AgentCache::get_or_build(key, build_fn)`: lazy LRU lookup that
//!     calls `build_fn` only on miss or eviction.
//!   - `AgentCache::sweep_idle()`: removes entries whose `last_used`
//!     timestamp is older than `IDLE_EVICTION_SECS` (default 3600s).
//!
//! Wiring lives in `agent/agent-core/src/run/config_builder.rs`
//! (replace the per-turn rebuild with `cache.get_or_build(...)`).

use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Maximum entries kept in the LRU.
pub const MAX_CACHE_SIZE: usize = 128;

/// Idle-eviction threshold — entries unused longer than this are
/// evicted by `sweep_idle()` (called from a tokio background task or
/// inline before each `get_or_build`).
pub const IDLE_EVICTION_SECS: u64 = 3600;

/// Cache key — combination of (agent_id, model_id, working_folder).
/// Together these uniquely identify the runtime configuration an
/// `AgentHandle` was built for; different keys must produce
/// independent handles.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AgentKey {
    pub agent_id: String,
    pub model_id: String,
    pub working_folder: String,
}

impl AgentKey {
    pub fn new(
        agent_id: impl Into<String>,
        model_id: impl Into<String>,
        working_folder: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            model_id: model_id.into(),
            working_folder: working_folder.into(),
        }
    }
}

/// Cached agent handle. The handle type is intentionally generic so
/// the cache can be reused by any caller that needs a bounded LRU of
/// per-key resources (model clients, RAG retrievers, etc.).
#[derive(Debug)]
pub struct CachedEntry<T> {
    pub handle: Arc<T>,
    pub last_used: Instant,
}

/// Bounded LRU cache with idle eviction.
///
/// Not a perfect LRU (no recency bump on read), but `last_used` is
/// refreshed on every `get_or_build` so a sweep pass evicts true
/// cold entries. The eviction cap is enforced on insert.
pub struct AgentCache<T: Send + Sync + 'static> {
    inner: Mutex<lru::LruCache<AgentKey, CachedEntry<T>>>,
}

impl<T: Send + Sync + 'static> AgentCache<T> {
    /// Construct a new cache with `MAX_CACHE_SIZE` capacity.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(MAX_CACHE_SIZE)
                    .expect("MAX_CACHE_SIZE > 0"),
            )),
        }
    }

    /// Construct with custom capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inner: Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(cap.max(1)).expect("capacity > 0"),
            )),
        }
    }

    /// Lookup or build. On hit: refresh `last_used` and return the
    /// existing handle. On miss: call `build_fn`, insert (evicting
    /// the LRU tail if necessary), and return the new handle.
    pub fn get_or_build<F>(&self, key: AgentKey, build_fn: F) -> Arc<T>
    where
        F: FnOnce() -> Arc<T>,
    {
        let mut guard = self.inner.lock();
        if let Some(entry) = guard.get_mut(&key) {
            entry.last_used = Instant::now();
            return entry.handle.clone();
        }
        drop(guard);
        let handle = build_fn();
        let mut guard = self.inner.lock();
        guard.push(
            key,
            CachedEntry {
                handle: handle.clone(),
                last_used: Instant::now(),
            },
        );
        handle
    }

    /// Evict entries whose `last_used` is older than the idle
    /// threshold. Returns the number of evicted entries.
    pub fn sweep_idle(&self) -> usize {
        let threshold = Duration::from_secs(IDLE_EVICTION_SECS);
        let now = Instant::now();
        let mut guard = self.inner.lock();
        let before = guard.len();
        let stale_keys: Vec<AgentKey> = guard
            .iter()
            .filter_map(|(k, v)| {
                if now.duration_since(v.last_used) > threshold {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        for k in stale_keys {
            guard.pop(&k);
        }
        before - guard.len()
    }

    /// Number of cached entries (for tests / diagnostics).
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// True if the cache has no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl<T: Send + Sync + 'static> Default for AgentCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn a background tokio task that calls `sweep_idle()` every
/// `IDLE_EVICTION_SECS / 6` (10 minutes by default). Returns the
/// `JoinHandle` so callers can abort it on shutdown.
pub fn spawn_idle_sweeper<T: Send + Sync + 'static>(
    cache: Arc<AgentCache<T>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_secs(IDLE_EVICTION_SECS / 6);
        loop {
            tokio::time::sleep(interval).await;
            let evicted = cache.sweep_idle();
            if evicted > 0 {
                tracing::info!("agent_cache: evicted {} idle entries", evicted);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DummyHandle(u64);

    #[test]
    fn get_or_build_caches_on_second_call() {
        let cache = AgentCache::<DummyHandle>::new();
        let key = AgentKey::new("a", "m", "/tmp");
        let build_count = Arc::new(AtomicUsize::new(0));
        let bc = build_count.clone();
        let h1 = cache.get_or_build(key.clone(), move || {
            bc.fetch_add(1, Ordering::SeqCst);
            Arc::new(DummyHandle(1))
        });
        let h2 = cache.get_or_build(key.clone(), || Arc::new(DummyHandle(2)));
        assert_eq!(h1.0, 1);
        assert_eq!(h2.0, 1, "second call must return cached handle");
        assert_eq!(build_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn get_or_build_distinguishes_keys() {
        let cache = AgentCache::<DummyHandle>::new();
        let k1 = AgentKey::new("a", "m", "/tmp");
        let k2 = AgentKey::new("b", "m", "/tmp");
        let h1 = cache.get_or_build(k1, || Arc::new(DummyHandle(1)));
        let h2 = cache.get_or_build(k2, || Arc::new(DummyHandle(2)));
        assert_eq!(h1.0, 1);
        assert_eq!(h2.0, 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn lru_evicts_when_over_capacity() {
        let cache = AgentCache::<DummyHandle>::with_capacity(2);
        let k1 = AgentKey::new("a", "m", "/tmp");
        let k2 = AgentKey::new("b", "m", "/tmp");
        let k3 = AgentKey::new("c", "m", "/tmp");
        cache.get_or_build(k1.clone(), || Arc::new(DummyHandle(1)));
        cache.get_or_build(k2.clone(), || Arc::new(DummyHandle(2)));
        cache.get_or_build(k3.clone(), || Arc::new(DummyHandle(3)));
        assert_eq!(cache.len(), 2);
        // k1 was least-recently inserted; rebuild path proves it was evicted.
        let h1 = cache.get_or_build(k1, || Arc::new(DummyHandle(99)));
        assert_eq!(h1.0, 99);
    }

    #[test]
    fn sweep_idle_evicts_stale_entries() {
        let cache = AgentCache::<DummyHandle>::new();
        let key = AgentKey::new("a", "m", "/tmp");
        cache.get_or_build(key.clone(), || Arc::new(DummyHandle(1)));
        // Force last_used into the past.
        {
            let mut guard = cache.inner.lock();
            let entry = guard.peek_mut(&key).unwrap();
            entry.last_used =
                Instant::now() - Duration::from_secs(IDLE_EVICTION_SECS + 1);
        }
        let evicted = cache.sweep_idle();
        assert_eq!(evicted, 1);
        assert!(cache.is_empty());
    }

    #[test]
    fn sweep_idle_keeps_fresh_entries() {
        let cache = AgentCache::<DummyHandle>::new();
        let key = AgentKey::new("a", "m", "/tmp");
        cache.get_or_build(key, || Arc::new(DummyHandle(1)));
        let evicted = cache.sweep_idle();
        assert_eq!(evicted, 0);
        assert_eq!(cache.len(), 1);
    }
}