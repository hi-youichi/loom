//! Diagnostic caching system for LSP.
//!
//! Provides efficient caching of diagnostic results to avoid redundant LSP requests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lsp_types::{Diagnostic, Url};
use tokio::sync::RwLock;

/// Cache entry for diagnostics.
#[derive(Clone, Debug)]
pub struct DiagnosticCacheEntry {
    /// The diagnostics for this file.
    pub diagnostics: Vec<Diagnostic>,
    /// When this cache entry was created.
    pub timestamp: Instant,
    /// File version when diagnostics were computed.
    pub version: i32,
}

/// Diagnostic cache configuration.
#[derive(Clone, Debug)]
pub struct DiagnosticCacheConfig {
    /// Time-to-live for cache entries (default: 5 seconds).
    pub ttl: Duration,
    /// Maximum number of cache entries (default: 1000).
    pub max_entries: usize,
}

impl Default for DiagnosticCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(5),
            max_entries: 1000,
        }
    }
}

/// Diagnostic cache for storing and retrieving diagnostic results.
#[derive(Debug)]
pub struct DiagnosticCache {
    /// Cache storage.
    cache: Arc<RwLock<HashMap<Url, DiagnosticCacheEntry>>>,
    /// Cache configuration.
    config: DiagnosticCacheConfig,
}

impl DiagnosticCache {
    /// Create a new diagnostic cache with default configuration.
    pub fn new() -> Self {
        Self::with_config(DiagnosticCacheConfig::default())
    }

    /// Create a new diagnostic cache with custom configuration.
    pub fn with_config(config: DiagnosticCacheConfig) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Get diagnostics from cache if available and not expired.
    pub async fn get(&self, uri: &Url, version: i32) -> Option<Vec<Diagnostic>> {
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(uri) {
            if entry.timestamp.elapsed() > self.config.ttl {
                return None;
            }

            if entry.version != version {
                return None;
            }

            return Some(entry.diagnostics.clone());
        }

        None
    }

    pub async fn get_latest(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(uri) {
            if entry.timestamp.elapsed() > self.config.ttl {
                return None;
            }

            return Some(entry.diagnostics.clone());
        }

        None
    }

    /// Store diagnostics in cache.
    pub async fn put(&self, uri: Url, version: i32, diagnostics: Vec<Diagnostic>) {
        let mut cache = self.cache.write().await;
        
        // Remove expired entries if cache is full
        if cache.len() >= self.config.max_entries {
            self.evict_expired_entries(&mut cache);
            
            // If still full, remove oldest entry
            if cache.len() >= self.config.max_entries {
                self.evict_oldest_entry(&mut cache);
            }
        }
        
        cache.insert(
            uri,
            DiagnosticCacheEntry {
                diagnostics,
                timestamp: Instant::now(),
                version,
            },
        );
    }

    /// Invalidate cache entry for a specific file.
    pub async fn invalidate(&self, uri: &Url) {
        let mut cache = self.cache.write().await;
        cache.remove(uri);
    }

    /// Clear all cache entries.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> DiagnosticCacheStats {
        let cache = self.cache.read().await;
        let total_entries = cache.len();
        let expired_entries = cache
            .values()
            .filter(|entry| entry.timestamp.elapsed() > self.config.ttl)
            .count();
        
        DiagnosticCacheStats {
            total_entries,
            expired_entries,
            active_entries: total_entries - expired_entries,
        }
    }

    /// Evict expired entries from cache.
    fn evict_expired_entries(&self, cache: &mut HashMap<Url, DiagnosticCacheEntry>) {
        cache.retain(|_, entry| entry.timestamp.elapsed() <= self.config.ttl);
    }

    /// Evict the oldest entry from cache.
    fn evict_oldest_entry(&self, cache: &mut HashMap<Url, DiagnosticCacheEntry>) {
        if let Some((oldest_uri, _)) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.timestamp)
        {
            let oldest_uri = oldest_uri.clone();
            cache.remove(&oldest_uri);
        }
    }
}

impl Default for DiagnosticCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct DiagnosticCacheStats {
    /// Total number of cache entries.
    pub total_entries: usize,
    /// Number of expired entries.
    pub expired_entries: usize,
    /// Number of active (non-expired) entries.
    pub active_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DiagnosticSeverity, Position, Range};

    fn create_test_diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 0, character: 10 },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some("test".to_string()),
            message: message.to_string(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];
        
        cache.put(uri.clone(), 1, diagnostics.clone()).await;
        
        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), diagnostics);
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];
        
        cache.put(uri.clone(), 1, diagnostics).await;
        cache.invalidate(&uri).await;
        
        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_version_mismatch() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];
        
        cache.put(uri.clone(), 1, diagnostics).await;
        
        let cached = cache.get(&uri, 2).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = DiagnosticCache::new();
        let uri1 = Url::parse("file:///test1.rs").unwrap();
        let uri2 = Url::parse("file:///test2.rs").unwrap();
        
        cache.put(uri1.clone(), 1, vec![]).await;
        cache.put(uri2.clone(), 1, vec![]).await;
        
        cache.clear().await;
        
        assert!(cache.get(&uri1, 1).await.is_none());
        assert!(cache.get(&uri2, 1).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        
        cache.put(uri.clone(), 1, vec![]).await;
        
        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.active_entries, 1);
        assert_eq!(stats.expired_entries, 0);
    }
}
