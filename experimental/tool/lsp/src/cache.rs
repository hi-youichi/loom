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
        if let Some((oldest_uri, _)) = cache.iter().min_by_key(|(_, entry)| entry.timestamp) {
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
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 10,
                },
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

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_put_and_get() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics.clone()).await;

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), diagnostics);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_invalidation() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics).await;
        cache.invalidate(&uri).await;

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_version_mismatch() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics).await;

        let cached = cache.get(&uri, 2).await;
        assert!(cached.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_stats() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        cache.put(uri.clone(), 1, vec![]).await;

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.active_entries, 1);
        assert_eq!(stats.expired_entries, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_expiry() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_millis(100), // Short TTL
            max_entries: 1000,
        };
        let cache = DiagnosticCache::with_config(config);
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics.clone()).await;

        // Should be available immediately
        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be expired
        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_eviction_when_full() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_secs(10), // Long TTL
            max_entries: 2,               // Small cache
        };
        let cache = DiagnosticCache::with_config(config);

        // Fill the cache beyond capacity
        for i in 0..5 {
            let uri = Url::parse(&format!("file:///test{}.rs", i)).unwrap();
            let diagnostics = vec![create_test_diagnostic(&format!("Error {}", i))];
            cache.put(uri.clone(), i, diagnostics).await;
        }

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 2); // Should be at max capacity
        assert_eq!(stats.active_entries, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_get_latest() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics.clone()).await;

        let cached = cache.get_latest(&uri).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), diagnostics);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_get_latest_expired() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_millis(100),
            max_entries: 1000,
        };
        let cache = DiagnosticCache::with_config(config);
        let uri = Url::parse("file:///test.rs").unwrap();
        let diagnostics = vec![create_test_diagnostic("Test error")];

        cache.put(uri.clone(), 1, diagnostics).await;

        tokio::time::sleep(Duration::from_millis(150)).await;

        let cached = cache.get_latest(&uri).await;
        assert!(cached.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_multiple_entries() {
        let cache = DiagnosticCache::new();

        for i in 0..5 {
            let uri = Url::parse(&format!("file:///test{}.rs", i)).unwrap();
            let diagnostics = vec![create_test_diagnostic(&format!("Error {}", i))];
            cache.put(uri.clone(), i, diagnostics).await;
        }

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 5);
        assert_eq!(stats.active_entries, 5);
        assert_eq!(stats.expired_entries, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_empty_diagnostics() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        let empty_diagnostics = vec![];

        cache.put(uri.clone(), 1, empty_diagnostics.clone()).await;

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), empty_diagnostics);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_nonexistent_file() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///nonexistent.rs").unwrap();

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_invalidate_nonexistent() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///nonexistent.rs").unwrap();

        cache.invalidate(&uri).await;

        // Should not panic and stats should remain 0
        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_overwrite_existing() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        let diagnostics1 = vec![create_test_diagnostic("Error 1")];
        let diagnostics2 = vec![create_test_diagnostic("Error 2")];

        cache.put(uri.clone(), 1, diagnostics1).await;
        cache.put(uri.clone(), 1, diagnostics2.clone()).await;

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), diagnostics2); // Should have latest data
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_config_custom() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_secs(30),
            max_entries: 500,
        };
        let cache = DiagnosticCache::with_config(config);

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 0); // Verify cache is initialized correctly
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_stats_display() {
        let cache = DiagnosticCache::new();
        let stats = cache.stats().await;

        let stats_str = format!("{:?}", stats);
        assert!(stats_str.contains("DiagnosticCacheStats"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_with_multiple_severities() {
        let cache = DiagnosticCache::new();
        let uri = Url::parse("file:///test.rs").unwrap();

        let diagnostics = vec![
            Diagnostic {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("test".to_string()),
                message: "Error message".to_string(),
                related_information: None,
                tags: None,
                data: None,
            },
            Diagnostic {
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 10,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: None,
                code_description: None,
                source: Some("test".to_string()),
                message: "Warning message".to_string(),
                related_information: None,
                tags: None,
                data: None,
            },
            Diagnostic {
                range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 10,
                    },
                },
                severity: Some(DiagnosticSeverity::HINT),
                code: None,
                code_description: None,
                source: Some("test".to_string()),
                message: "Hint message".to_string(),
                related_information: None,
                tags: None,
                data: None,
            },
        ];

        cache.put(uri.clone(), 1, diagnostics.clone()).await;

        let cached = cache.get(&uri, 1).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 3);
    }

    #[test]
    fn test_diagnostic_cache_entry_struct() {
        let entry = DiagnosticCacheEntry {
            diagnostics: vec![create_test_diagnostic("Test")],
            timestamp: Instant::now(),
            version: 5,
        };

        assert_eq!(entry.version, 5);
        assert_eq!(entry.diagnostics.len(), 1);
    }

    #[test]
    fn test_diagnostic_cache_config_default() {
        let config = DiagnosticCacheConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(5));
        assert_eq!(config.max_entries, 1000);
    }

    #[test]
    fn test_diagnostic_cache_config_clone() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_secs(10),
            max_entries: 500,
        };

        let cloned = config.clone();
        assert_eq!(cloned.ttl, Duration::from_secs(10));
        assert_eq!(cloned.max_entries, 500);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_diagnostic_cache_default() {
        let cache = DiagnosticCache::default();
        assert!(cache
            .get(&Url::parse("file:///test.rs").unwrap(), 1)
            .await
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_concurrent_access() {
        let cache = Arc::new(DiagnosticCache::new());
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let cache = Arc::clone(&cache);
                tokio::spawn(async move {
                    let uri = Url::parse(&format!("file:///test{}.rs", i)).unwrap();
                    let diagnostics = vec![create_test_diagnostic(&format!("Error {}", i))];
                    cache.put(uri.clone(), i, diagnostics).await;
                    cache.get(&uri, i).await
                })
            })
            .collect();

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_some());
        }

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 10);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cache_stats_active_after_expiry() {
        let config = DiagnosticCacheConfig {
            ttl: Duration::from_millis(100),
            max_entries: 1000,
        };
        let cache = DiagnosticCache::with_config(config);

        // Add entries that will expire
        for i in 0..3 {
            let uri = Url::parse(&format!("file:///test{}.rs", i)).unwrap();
            let diagnostics = vec![create_test_diagnostic(&format!("Error {}", i))];
            cache.put(uri.clone(), i, diagnostics).await;
        }

        tokio::time::sleep(Duration::from_millis(150)).await;

        // Add a fresh entry
        let fresh_uri = Url::parse("file:///fresh.rs").unwrap();
        let fresh_diagnostics = vec![create_test_diagnostic("Fresh error")];
        cache.put(fresh_uri.clone(), 1, fresh_diagnostics).await;

        let stats = cache.stats().await;
        assert!(stats.active_entries >= 1);
        assert!(stats.expired_entries >= 3);
    }
}
