//! In-memory cache implementation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use super::{Cache, CacheError};

struct CacheEntry<V> {
    value: V,
    expires_at: Option<Instant>,
}

impl<V> CacheEntry<V> {
    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            expires_at < Instant::now()
        } else {
            false
        }
    }
}

/// In-memory cache implementation.
///
/// This cache stores values in memory using a `HashMap`. It supports TTL
/// (time-to-live) for automatic expiration of entries.
///
/// # Example
///
/// ```rust,ignore
/// use graphweave::cache::{Cache, InMemoryCache};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let cache = InMemoryCache::new();
///     cache.set("key".to_string(), "value".to_string(), None).await.unwrap();
///     assert_eq!(cache.get(&"key".to_string()).await, Some("value".to_string()));
///
///     // With TTL
///     cache.set("key2".to_string(), "value2".to_string(), Some(Duration::from_secs(1))).await.unwrap();
///     assert_eq!(cache.get(&"key2".to_string()).await, Some("value2".to_string()));
/// }
/// ```
pub struct InMemoryCache<K, V> {
    data: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
}

impl<K, V> InMemoryCache<K, V>
where
    K: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates a new in-memory cache.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<K, V> Default for InMemoryCache<K, V>
where
    K: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl<K, V> Cache<K, V> for InMemoryCache<K, V>
where
    K: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    async fn get(&self, key: &K) -> Option<V> {
        let data = self.data.read().await;
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                return None;
            }
            return Some(entry.value.clone());
        }
        None
    }

    async fn set(&self, key: K, value: V, ttl: Option<Duration>) -> Result<(), CacheError> {
        let expires_at = ttl.map(|d| Instant::now() + d);
        let entry = CacheEntry { value, expires_at };
        let mut data = self.data.write().await;
        data.insert(key, entry);
        Ok(())
    }

    async fn delete(&self, key: &K) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        data.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        data.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_in_memory_cache_basic() {
        let cache = InMemoryCache::new();
        assert_eq!(cache.get(&"key".to_string()).await, None);

        cache
            .set("key".to_string(), "value".to_string(), None)
            .await
            .unwrap();
        assert_eq!(
            cache.get(&"key".to_string()).await,
            Some("value".to_string())
        );
    }

    #[tokio::test]
    async fn test_in_memory_cache_ttl() {
        let cache = InMemoryCache::new();
        cache
            .set(
                "key".to_string(),
                "value".to_string(),
                Some(Duration::from_millis(100)),
            )
            .await
            .unwrap();

        assert_eq!(
            cache.get(&"key".to_string()).await,
            Some("value".to_string())
        );

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(cache.get(&"key".to_string()).await, None);
    }

    #[tokio::test]
    async fn test_in_memory_cache_delete() {
        let cache = InMemoryCache::new();
        cache
            .set("key".to_string(), "value".to_string(), None)
            .await
            .unwrap();
        cache.delete(&"key".to_string()).await.unwrap();
        assert_eq!(cache.get(&"key".to_string()).await, None);
    }

    #[tokio::test]
    async fn test_in_memory_cache_clear() {
        let cache = InMemoryCache::new();
        cache
            .set("key1".to_string(), "value1".to_string(), None)
            .await
            .unwrap();
        cache
            .set("key2".to_string(), "value2".to_string(), None)
            .await
            .unwrap();
        cache.clear().await.unwrap();
        assert_eq!(cache.get(&"key1".to_string()).await, None);
        assert_eq!(cache.get(&"key2".to_string()).await, None);
    }
}
