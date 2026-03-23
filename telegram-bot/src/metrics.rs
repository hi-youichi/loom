//! Metrics collection module for telegram-bot
//!
//! Provides atomic counters for monitoring bot performance and usage.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct BotMetrics {
    pub messages_total: AtomicU64,
    pub messages_failed: AtomicU64,
    pub files_downloaded: AtomicU64,
    pub agent_calls: AtomicU64,
    pub agent_failures: AtomicU64,
    pub messages_sent: AtomicU64,
    pub messages_edited: AtomicU64,
}

impl BotMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment_messages(&self) {
        self.messages_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_failures(&self) {
        self.messages_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_downloads(&self) {
        self.files_downloaded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_agent_calls(&self) {
        self.agent_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_agent_failures(&self) {
        self.agent_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_messages_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_messages_edited(&self) {
        self.messages_edited.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_total: self.messages_total.load(Ordering::Relaxed),
            messages_failed: self.messages_failed.load(Ordering::Relaxed),
            files_downloaded: self.files_downloaded.load(Ordering::Relaxed),
            agent_calls: self.agent_calls.load(Ordering::Relaxed),
            agent_failures: self.agent_failures.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_edited: self.messages_edited.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub messages_total: u64,
    pub messages_failed: u64,
    pub files_downloaded: u64,
    pub agent_calls: u64,
    pub agent_failures: u64,
    pub messages_sent: u64,
    pub messages_edited: u64,
}

pub fn create_metrics_middleware(metrics: Arc<BotMetrics>) -> impl Fn() + Clone {
    move || {
        let m = metrics.clone();
        m.increment_messages();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_increment() {
        let metrics = BotMetrics::new();

        metrics.increment_messages();
        metrics.increment_messages();
        metrics.increment_failures();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.messages_total, 2);
        assert_eq!(snapshot.messages_failed, 1);
    }

    #[test]
    fn test_metrics_all_counters() {
        let metrics = BotMetrics::new();

        metrics.increment_downloads();
        metrics.increment_agent_calls();
        metrics.increment_agent_failures();
        metrics.increment_messages_sent();
        metrics.increment_messages_edited();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.files_downloaded, 1);
        assert_eq!(snapshot.agent_calls, 1);
        assert_eq!(snapshot.agent_failures, 1);
        assert_eq!(snapshot.messages_sent, 1);
        assert_eq!(snapshot.messages_edited, 1);
    }
}
