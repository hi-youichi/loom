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
}
