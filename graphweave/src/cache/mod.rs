//! Cache system for LLM calls and other expensive operations.
//!
//! Provides caching capabilities to avoid redundant computations, especially
//! useful for LLM calls where the same prompt with the same parameters should
//! return the same result.

mod error;
mod in_memory;

pub use error::CacheError;
pub use in_memory::InMemoryCache;

use async_trait::async_trait;
use std::time::Duration;

/// Cache trait for key-value storage with optional TTL.
///
/// Used primarily for caching LLM responses, but can be used for any
/// expensive computation that should be cached.
#[async_trait]
pub trait Cache<K, V>: Send + Sync
where
    K: Send + Sync,
    V: Clone + Send + Sync,
{
    /// Get a value from the cache by key.
    ///
    /// Returns `None` if the key is not found or has expired.
    async fn get(&self, key: &K) -> Option<V>;

    /// Set a value in the cache with an optional TTL.
    ///
    /// If `ttl` is `None`, the value will not expire.
    /// If `ttl` is `Some(duration)`, the value will expire after that duration.
    async fn set(&self, key: K, value: V, ttl: Option<Duration>) -> Result<(), CacheError>;

    /// Delete a value from the cache.
    async fn delete(&self, key: &K) -> Result<(), CacheError>;

    /// Clear all entries from the cache.
    async fn clear(&self) -> Result<(), CacheError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_trait_object() {
        let cache: Box<dyn Cache<String, String>> = Box::new(InMemoryCache::new());
        cache
            .set("key".to_string(), "value".to_string(), None)
            .await
            .unwrap();
        assert_eq!(
            cache.get(&"key".to_string()).await,
            Some("value".to_string())
        );
    }
}
