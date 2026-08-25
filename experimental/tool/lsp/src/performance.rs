//! Performance monitoring and metrics for LSP operations.
//!
//! Tracks performance metrics like request latency, cache hit rates,
//! and language server health.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::{debug, info};

/// Performance metrics for a single LSP operation.
#[derive(Debug, Clone)]
pub struct OperationMetric {
    pub operation_type: String,
    pub language: String,
    pub duration: Duration,
    pub success: bool,
    pub cache_hit: bool,
    pub timestamp: Instant,
}

/// Aggregated statistics for an operation type.
#[derive(Debug, Clone, Default)]
pub struct OperationStats {
    pub total_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub cache_hit_count: u64,
    pub total_duration: Duration,
    pub min_duration: Option<Duration>,
    pub max_duration: Option<Duration>,
}

impl OperationStats {
    pub fn average_duration(&self) -> Duration {
        if self.total_count == 0 {
            Duration::ZERO
        } else {
            self.total_duration / self.total_count as u32
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_count == 0 {
            0.0
        } else {
            (self.success_count as f64) / (self.total_count as f64) * 100.0
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_count == 0 {
            0.0
        } else {
            (self.cache_hit_count as f64) / (self.total_count as f64) * 100.0
        }
    }
}

/// Performance monitor for tracking LSP operations.
pub struct PerformanceMonitor {
    /// Recent operation metrics (circular buffer)
    recent_metrics: Arc<RwLock<Vec<OperationMetric>>>,

    /// Aggregated stats by operation type
    stats_by_operation: DashMap<String, OperationStats>,

    /// Stats by language
    stats_by_language: DashMap<String, OperationStats>,

    /// Maximum number of recent metrics to keep
    max_recent_metrics: usize,
}

impl PerformanceMonitor {
    pub fn new(max_recent_metrics: usize) -> Self {
        Self {
            recent_metrics: Arc::new(RwLock::new(Vec::with_capacity(max_recent_metrics))),
            stats_by_operation: DashMap::new(),
            stats_by_language: DashMap::new(),
            max_recent_metrics,
        }
    }

    /// Record an operation metric.
    pub fn record(&self, metric: OperationMetric) {
        // Update recent metrics
        if let Ok(mut recent) = self.recent_metrics.write() {
            if recent.len() >= self.max_recent_metrics {
                recent.remove(0);
            }
            recent.push(metric.clone());
        }

        // Update stats by operation
        self.update_stats(&metric.operation_type, &self.stats_by_operation, &metric);

        // Update stats by language
        self.update_stats(&metric.language, &self.stats_by_language, &metric);

        debug!(
            operation = %metric.operation_type,
            language = %metric.language,
            duration_ms = %metric.duration.as_millis(),
            success = %metric.success,
            cache_hit = %metric.cache_hit,
            "Recorded LSP operation metric"
        );
    }

    fn update_stats(
        &self,
        key: &str,
        map: &DashMap<String, OperationStats>,
        metric: &OperationMetric,
    ) {
        let mut entry = map.entry(key.to_string()).or_default();

        entry.total_count += 1;
        entry.total_duration += metric.duration;

        if metric.success {
            entry.success_count += 1;
        } else {
            entry.failure_count += 1;
        }

        if metric.cache_hit {
            entry.cache_hit_count += 1;
        }

        match entry.min_duration {
            None => entry.min_duration = Some(metric.duration),
            Some(min) if metric.duration < min => entry.min_duration = Some(metric.duration),
            _ => {}
        }

        match entry.max_duration {
            None => entry.max_duration = Some(metric.duration),
            Some(max) if metric.duration > max => entry.max_duration = Some(metric.duration),
            _ => {}
        }
    }

    /// Get statistics for an operation type.
    pub fn get_operation_stats(&self, operation_type: &str) -> Option<OperationStats> {
        self.stats_by_operation
            .get(operation_type)
            .map(|s| s.clone())
    }

    /// Get statistics for a language.
    pub fn get_language_stats(&self, language: &str) -> Option<OperationStats> {
        self.stats_by_language.get(language).map(|s| s.clone())
    }

    /// Get all operation types with stats.
    pub fn operation_types(&self) -> Vec<String> {
        self.stats_by_operation
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all languages with stats.
    pub fn languages(&self) -> Vec<String> {
        self.stats_by_language
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Clear all metrics and stats.
    pub fn clear(&self) {
        if let Ok(mut recent) = self.recent_metrics.write() {
            recent.clear();
        }
        self.stats_by_operation.clear();
        self.stats_by_language.clear();
    }

    /// Generate a performance report.
    pub fn generate_report(&self) -> PerformanceReport {
        let mut total_operations = 0u64;
        let mut total_success = 0u64;
        let mut total_failures = 0u64;
        let mut total_cache_hits = 0u64;
        let mut total_duration = Duration::ZERO;

        let mut operation_breakdown = HashMap::new();
        let mut language_breakdown = HashMap::new();

        // Aggregate operation stats
        for entry in self.stats_by_operation.iter() {
            let stats = entry.value();
            total_operations += stats.total_count;
            total_success += stats.success_count;
            total_failures += stats.failure_count;
            total_cache_hits += stats.cache_hit_count;
            total_duration += stats.total_duration;

            operation_breakdown.insert(
                entry.key().clone(),
                OperationReport {
                    count: stats.total_count,
                    success_rate: stats.success_rate(),
                    cache_hit_rate: stats.cache_hit_rate(),
                    average_latency_ms: stats.average_duration().as_millis() as f64,
                    min_latency_ms: stats
                        .min_duration
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0),
                    max_latency_ms: stats
                        .max_duration
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0),
                },
            );
        }

        // Aggregate language stats
        for entry in self.stats_by_language.iter() {
            let stats = entry.value();

            language_breakdown.insert(
                entry.key().clone(),
                OperationReport {
                    count: stats.total_count,
                    success_rate: stats.success_rate(),
                    cache_hit_rate: stats.cache_hit_rate(),
                    average_latency_ms: stats.average_duration().as_millis() as f64,
                    min_latency_ms: stats
                        .min_duration
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0),
                    max_latency_ms: stats
                        .max_duration
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0),
                },
            );
        }

        let average_latency_ms = if total_operations > 0 {
            total_duration.as_millis() as f64 / total_operations as f64
        } else {
            0.0
        };

        PerformanceReport {
            total_operations,
            total_success,
            total_failures,
            total_cache_hits,
            average_latency_ms,
            operation_breakdown,
            language_breakdown,
        }
    }

    /// Log a summary of performance metrics.
    pub fn log_summary(&self) {
        let report = self.generate_report();

        info!(
            total_operations = %report.total_operations,
            success_rate = %report.success_rate(),
            cache_hit_rate = %report.cache_hit_rate(),
            avg_latency_ms = %report.average_latency_ms,
            "LSP Performance Summary"
        );

        for (operation, op_report) in &report.operation_breakdown {
            info!(
                operation = %operation,
                count = %op_report.count,
                success_rate = %op_report.success_rate,
                cache_hit_rate = %op_report.cache_hit_rate,
                avg_latency_ms = %op_report.average_latency_ms,
                "Operation stats"
            );
        }
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Performance report.
#[derive(Debug, Clone)]
pub struct PerformanceReport {
    pub total_operations: u64,
    pub total_success: u64,
    pub total_failures: u64,
    pub total_cache_hits: u64,
    pub average_latency_ms: f64,
    pub operation_breakdown: HashMap<String, OperationReport>,
    pub language_breakdown: HashMap<String, OperationReport>,
}

impl PerformanceReport {
    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            0.0
        } else {
            (self.total_success as f64) / (self.total_operations as f64) * 100.0
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_operations == 0 {
            0.0
        } else {
            (self.total_cache_hits as f64) / (self.total_operations as f64) * 100.0
        }
    }
}

/// Report for a specific operation type.
#[derive(Debug, Clone)]
pub struct OperationReport {
    pub count: u64,
    pub success_rate: f64,
    pub cache_hit_rate: f64,
    pub average_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
}

/// Helper to time an operation.
pub struct OperationTimer {
    operation_type: String,
    language: String,
    start: Instant,
    monitor: Arc<PerformanceMonitor>,
}

impl OperationTimer {
    pub fn new(operation_type: String, language: String, monitor: Arc<PerformanceMonitor>) -> Self {
        Self {
            operation_type,
            language,
            start: Instant::now(),
            monitor,
        }
    }

    pub fn finish(self, success: bool, cache_hit: bool) {
        let metric = OperationMetric {
            operation_type: self.operation_type,
            language: self.language,
            duration: self.start.elapsed(),
            success,
            cache_hit,
            timestamp: Instant::now(),
        };

        self.monitor.record(metric);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_monitor() {
        let monitor = PerformanceMonitor::new(100);

        let metric = OperationMetric {
            operation_type: "completion".to_string(),
            language: "rust".to_string(),
            duration: Duration::from_millis(50),
            success: true,
            cache_hit: false,
            timestamp: Instant::now(),
        };

        monitor.record(metric);

        let stats = monitor.get_operation_stats("completion").unwrap();
        assert_eq!(stats.total_count, 1);
        assert_eq!(stats.success_count, 1);
    }

    #[test]
    fn test_operation_stats() {
        let stats = OperationStats {
            total_count: 10,
            success_count: 8,
            cache_hit_count: 5,
            total_duration: Duration::from_millis(100),
            ..OperationStats::default()
        };

        assert_eq!(stats.success_rate(), 80.0);
        assert_eq!(stats.cache_hit_rate(), 50.0);
        assert_eq!(stats.average_duration(), Duration::from_millis(10));
    }

    #[test]
    fn test_operation_stats_empty() {
        let stats = OperationStats::default();
        assert_eq!(stats.total_count, 0);
        assert_eq!(stats.success_count, 0);
        assert_eq!(stats.failure_count, 0);
        assert_eq!(stats.cache_hit_count, 0);
        assert_eq!(stats.total_duration, Duration::ZERO);
        assert!(stats.min_duration.is_none());
        assert!(stats.max_duration.is_none());
        assert_eq!(stats.success_rate(), 0.0);
        assert_eq!(stats.cache_hit_rate(), 0.0);
        assert_eq!(stats.average_duration(), Duration::ZERO);
    }

    #[test]
    fn test_operation_stats_min_max_duration() {
        let stats = OperationStats {
            min_duration: Some(Duration::from_millis(5)),
            max_duration: Some(Duration::from_millis(100)),
            ..Default::default()
        };

        assert_eq!(stats.min_duration, Some(Duration::from_millis(5)));
        assert_eq!(stats.max_duration, Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_performance_monitor_default() {
        let monitor = PerformanceMonitor::default();
        assert_eq!(monitor.operation_types().len(), 0);
        assert_eq!(monitor.languages().len(), 0);
    }

    #[test]
    fn test_performance_monitor_multiple_operations() {
        let monitor = PerformanceMonitor::new(100);

        for i in 0..10 {
            let metric = OperationMetric {
                operation_type: format!("operation_{}", i % 3),
                language: format!("lang_{}", i % 2),
                duration: Duration::from_millis(10 + i as u64 * 5),
                success: i % 2 == 0,
                cache_hit: i % 3 == 0,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        let operations = monitor.operation_types();
        assert_eq!(operations.len(), 3);

        let languages = monitor.languages();
        assert_eq!(languages.len(), 2);
    }

    #[test]
    fn test_performance_monitor_clear() {
        let monitor = PerformanceMonitor::new(100);

        for _ in 0..5 {
            let metric = OperationMetric {
                operation_type: "test".to_string(),
                language: "rust".to_string(),
                duration: Duration::from_millis(10),
                success: true,
                cache_hit: false,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        assert!(!monitor.operation_types().is_empty());
        assert!(!monitor.languages().is_empty());

        monitor.clear();

        assert_eq!(monitor.operation_types().len(), 0);
        assert_eq!(monitor.languages().len(), 0);
    }

    #[test]
    fn test_performance_monitor_language_stats() {
        let monitor = PerformanceMonitor::new(100);

        let metric1 = OperationMetric {
            operation_type: "completion".to_string(),
            language: "rust".to_string(),
            duration: Duration::from_millis(50),
            success: true,
            cache_hit: false,
            timestamp: Instant::now(),
        };

        let metric2 = OperationMetric {
            operation_type: "completion".to_string(),
            language: "typescript".to_string(),
            duration: Duration::from_millis(30),
            success: false,
            cache_hit: true,
            timestamp: Instant::now(),
        };

        monitor.record(metric1);
        monitor.record(metric2);

        let rust_stats = monitor.get_language_stats("rust").unwrap();
        assert_eq!(rust_stats.total_count, 1);
        assert_eq!(rust_stats.success_count, 1);

        let ts_stats = monitor.get_language_stats("typescript").unwrap();
        assert_eq!(ts_stats.total_count, 1);
        assert_eq!(ts_stats.failure_count, 1);
    }

    #[test]
    fn test_performance_monitor_stats_aggregation() {
        let monitor = PerformanceMonitor::new(100);

        for i in 0..5 {
            let metric = OperationMetric {
                operation_type: "completion".to_string(),
                language: "rust".to_string(),
                duration: Duration::from_millis(10 + i * 10),
                success: true,
                cache_hit: i % 2 == 0,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        let stats = monitor.get_operation_stats("completion").unwrap();
        assert_eq!(stats.total_count, 5);
        assert_eq!(stats.success_count, 5);
        assert_eq!(stats.cache_hit_count, 3); // 0, 2, 4
        assert_eq!(stats.total_duration.as_millis(), 150); // 10 + 20 + 30 + 40 + 50
        assert_eq!(stats.min_duration, Some(Duration::from_millis(10)));
        assert_eq!(stats.max_duration, Some(Duration::from_millis(50)));
    }

    #[test]
    fn test_performance_report_generation() {
        let monitor = PerformanceMonitor::new(100);

        let metric = OperationMetric {
            operation_type: "completion".to_string(),
            language: "rust".to_string(),
            duration: Duration::from_millis(50),
            success: true,
            cache_hit: false,
            timestamp: Instant::now(),
        };

        monitor.record(metric);

        let report = monitor.generate_report();
        assert_eq!(report.total_operations, 1);
        assert_eq!(report.total_success, 1);
        assert_eq!(report.total_failures, 0);
        assert_eq!(report.total_cache_hits, 0);
        assert!(report.average_latency_ms > 0.0);
        assert_eq!(report.operation_breakdown.len(), 1);
        assert_eq!(report.language_breakdown.len(), 1);
    }

    #[test]
    fn test_performance_report_empty() {
        let monitor = PerformanceMonitor::new(100);
        let report = monitor.generate_report();

        assert_eq!(report.total_operations, 0);
        assert_eq!(report.total_success, 0);
        assert_eq!(report.total_failures, 0);
        assert_eq!(report.total_cache_hits, 0);
        assert_eq!(report.average_latency_ms, 0.0);
        assert!(report.operation_breakdown.is_empty());
        assert!(report.language_breakdown.is_empty());
    }

    #[test]
    fn test_performance_report_success_rate() {
        let report = PerformanceReport {
            total_operations: 10,
            total_success: 8,
            total_failures: 2,
            total_cache_hits: 5,
            average_latency_ms: 50.0,
            operation_breakdown: HashMap::new(),
            language_breakdown: HashMap::new(),
        };

        assert_eq!(report.success_rate(), 80.0);
        assert_eq!(report.cache_hit_rate(), 50.0);
    }

    #[test]
    fn test_performance_report_zero_operations() {
        let report = PerformanceReport {
            total_operations: 0,
            total_success: 0,
            total_failures: 0,
            total_cache_hits: 0,
            average_latency_ms: 0.0,
            operation_breakdown: HashMap::new(),
            language_breakdown: HashMap::new(),
        };

        assert_eq!(report.success_rate(), 0.0);
        assert_eq!(report.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_operation_timer() {
        let monitor = Arc::new(PerformanceMonitor::new(100));
        let timer = OperationTimer::new(
            "test_operation".to_string(),
            "test_language".to_string(),
            monitor.clone(),
        );

        std::thread::sleep(Duration::from_millis(10));
        timer.finish(true, true);

        let stats = monitor.get_operation_stats("test_operation").unwrap();
        assert_eq!(stats.total_count, 1);
        assert_eq!(stats.success_count, 1);
        assert_eq!(stats.cache_hit_count, 1);
    }

    #[test]
    fn test_operation_timer_failure() {
        let monitor = Arc::new(PerformanceMonitor::new(100));
        let timer = OperationTimer::new(
            "failing_operation".to_string(),
            "test_language".to_string(),
            monitor.clone(),
        );

        std::thread::sleep(Duration::from_millis(5));
        timer.finish(false, false);

        let stats = monitor.get_operation_stats("failing_operation").unwrap();
        assert_eq!(stats.total_count, 1);
        assert_eq!(stats.failure_count, 1);
        assert_eq!(stats.success_count, 0);
    }

    #[test]
    fn test_operation_metric_fields() {
        let metric = OperationMetric {
            operation_type: "test_op".to_string(),
            language: "test_lang".to_string(),
            duration: Duration::from_millis(100),
            success: true,
            cache_hit: false,
            timestamp: Instant::now(),
        };

        assert_eq!(metric.operation_type, "test_op");
        assert_eq!(metric.language, "test_lang");
        assert_eq!(metric.duration, Duration::from_millis(100));
        assert!(metric.success);
        assert!(!metric.cache_hit);
    }

    #[test]
    fn test_operation_report_fields() {
        let op_report = OperationReport {
            count: 100,
            success_rate: 95.0,
            cache_hit_rate: 40.0,
            average_latency_ms: 25.5,
            min_latency_ms: 5.0,
            max_latency_ms: 200.0,
        };

        assert_eq!(op_report.count, 100);
        assert_eq!(op_report.success_rate, 95.0);
        assert_eq!(op_report.cache_hit_rate, 40.0);
        assert_eq!(op_report.average_latency_ms, 25.5);
        assert_eq!(op_report.min_latency_ms, 5.0);
        assert_eq!(op_report.max_latency_ms, 200.0);
    }

    #[test]
    fn test_performance_monitor_circular_buffer() {
        let monitor = PerformanceMonitor::new(3); // Small buffer

        for i in 0..5 {
            let metric = OperationMetric {
                operation_type: "test".to_string(),
                language: "rust".to_string(),
                duration: Duration::from_millis(i as u64 * 10),
                success: true,
                cache_hit: false,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        let stats = monitor.get_operation_stats("test").unwrap();
        assert_eq!(stats.total_count, 5); // Stats should still count all operations
    }

    #[test]
    fn test_multiple_languages_stats() {
        let monitor = PerformanceMonitor::new(100);

        let languages = vec!["rust", "typescript", "python", "go", "java"];

        for (i, lang) in languages.iter().enumerate() {
            let metric = OperationMetric {
                operation_type: "completion".to_string(),
                language: lang.to_string(),
                duration: Duration::from_millis(((i + 1) * 10) as u64),
                success: true,
                cache_hit: i % 2 == 0,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        assert_eq!(monitor.languages().len(), 5);

        for lang in &languages {
            let stats = monitor.get_language_stats(lang);
            assert!(stats.is_some());
        }
    }

    #[test]
    fn test_performance_report_comprehensive() {
        let monitor = PerformanceMonitor::new(100);

        // Add various operations
        let operations = vec![
            ("completion", "rust", Duration::from_millis(50), true, true),
            (
                "completion",
                "typescript",
                Duration::from_millis(30),
                true,
                false,
            ),
            ("hover", "rust", Duration::from_millis(20), true, true),
            (
                "definition",
                "python",
                Duration::from_millis(80),
                true,
                false,
            ),
            (
                "completion",
                "rust",
                Duration::from_millis(40),
                false,
                false,
            ),
        ];

        for (op_type, lang, duration, success, cache_hit) in operations {
            let metric = OperationMetric {
                operation_type: op_type.to_string(),
                language: lang.to_string(),
                duration,
                success,
                cache_hit,
                timestamp: Instant::now(),
            };
            monitor.record(metric);
        }

        let report = monitor.generate_report();
        assert_eq!(report.total_operations, 5);
        assert_eq!(report.total_success, 4);
        assert_eq!(report.total_failures, 1);
        assert_eq!(report.total_cache_hits, 2);
        assert_eq!(report.operation_breakdown.len(), 3);
        assert_eq!(report.language_breakdown.len(), 3);
    }

    #[test]
    fn test_operation_stats_failure_count() {
        let stats = OperationStats {
            total_count: 10,
            success_count: 7,
            failure_count: 3,
            ..Default::default()
        };

        assert_eq!(stats.total_count, 10);
        assert_eq!(stats.success_count, 7);
        assert_eq!(stats.failure_count, 3);
        assert_eq!(stats.success_rate(), 70.0);
    }
}
