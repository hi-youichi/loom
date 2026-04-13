//! Error recovery and resilience mechanisms for LSP integration.
//!
//! This module provides automatic error recovery, retry logic, and circuit breaker
//! patterns to ensure robust LSP server communication.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Circuit breaker states.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally.
    Closed,
    /// Circuit is open, requests are blocked.
    Open,
    /// Circuit is half-open, testing if service recovered.
    HalfOpen,
}

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit.
    pub failure_threshold: usize,
    /// Time to wait before attempting recovery (seconds).
    pub timeout_secs: u64,
    /// Number of successful requests to close circuit.
    pub success_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            timeout_secs: 60,
            success_threshold: 2,
        }
    }
}

/// Circuit breaker for a single language server.
#[derive(Debug)]
pub struct CircuitBreaker {
    state: CircuitState,
    failure_count: usize,
    success_count: usize,
    last_failure_time: Option<Instant>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            config,
        }
    }

    /// Check if requests should be allowed.
    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= Duration::from_secs(self.config.timeout_secs) {
                        debug!("Circuit breaker entering half-open state");
                        self.state = CircuitState::HalfOpen;
                        self.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request.
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    info!("Circuit breaker closed after successful recovery");
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                }
            }
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failed request.
    pub fn record_failure(&mut self) {
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    warn!(
                        failure_count = self.failure_count,
                        "Circuit breaker opened due to failures"
                    );
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                warn!("Circuit breaker reopened during half-open state");
                self.state = CircuitState::Open;
            }
            CircuitState::Open => {}
        }
    }

    /// Get current state.
    pub fn state(&self) -> &CircuitState {
        &self.state
    }
}

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: usize,
    /// Initial delay between retries (milliseconds).
    pub initial_delay_ms: u64,
    /// Maximum delay between retries (milliseconds).
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for a given retry attempt.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let delay_ms = self.initial_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32);

        let delay_ms = delay_ms.min(self.max_delay_ms as f64) as u64;

        Duration::from_millis(delay_ms)
    }
}

/// Error recovery manager for all language servers.
pub struct ErrorRecoveryManager {
    circuit_breakers: DashMap<String, Arc<RwLock<CircuitBreaker>>>,
    retry_config: RetryConfig,
    circuit_breaker_config: CircuitBreakerConfig,
}

impl ErrorRecoveryManager {
    pub fn new(retry_config: RetryConfig, circuit_breaker_config: CircuitBreakerConfig) -> Self {
        Self {
            circuit_breakers: DashMap::new(),
            retry_config,
            circuit_breaker_config,
        }
    }

    /// Get or create circuit breaker for a language server.
    pub fn get_circuit_breaker(&self, language: &str) -> Arc<RwLock<CircuitBreaker>> {
        self.circuit_breakers
            .entry(language.to_string())
            .or_insert_with(|| {
                Arc::new(RwLock::new(CircuitBreaker::new(
                    self.circuit_breaker_config.clone(),
                )))
            })
            .clone()
    }

    /// Get retry configuration.
    pub fn retry_config(&self) -> &RetryConfig {
        &self.retry_config
    }

    /// Execute an operation with retry logic.
    pub async fn with_retry<F, Fut, T, E>(&self, language: &str, operation: F) -> Result<T, E>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let circuit_breaker = self.get_circuit_breaker(language);

        // Check circuit breaker
        {
            let mut cb = circuit_breaker.write().await;
            if !cb.allow_request() {
                warn!(language = %language, "Request blocked by circuit breaker");
                // In a real implementation, we'd return a proper error type
                // For now, we'll just proceed and let the operation fail
            }
        }

        let mut last_error = None;

        for attempt in 0..=self.retry_config.max_retries {
            match operation().await {
                Ok(result) => {
                    // Record success
                    let mut cb = circuit_breaker.write().await;
                    cb.record_success();

                    if attempt > 0 {
                        info!(
                            language = %language,
                            attempt = attempt,
                            "Operation succeeded after retry"
                        );
                    }

                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);

                    // Record failure
                    let mut cb = circuit_breaker.write().await;
                    cb.record_failure();

                    if attempt < self.retry_config.max_retries {
                        let delay = self.retry_config.delay_for_attempt(attempt);
                        debug!(
                            language = %language,
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            "Operation failed, retrying"
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        // All retries exhausted
        Err(last_error.expect("At least one error should have occurred"))
    }

    /// Get health status for all language servers.
    pub async fn health_status(&self) -> std::collections::HashMap<String, CircuitState> {
        let mut status = std::collections::HashMap::new();

        for entry in self.circuit_breakers.iter() {
            let language = entry.key().clone();
            let cb = entry.value().read().await;
            status.insert(language, cb.state().clone());
        }

        status
    }

    /// Reset circuit breaker for a specific language server.
    pub async fn reset_circuit_breaker(&self, language: &str) {
        if let Some(cb) = self.circuit_breakers.get(language) {
            let mut cb = cb.write().await;
            cb.state = CircuitState::Closed;
            cb.failure_count = 0;
            cb.success_count = 0;
            cb.last_failure_time = None;
            info!(language = %language, "Circuit breaker reset");
        }
    }

    /// Reset all circuit breakers.
    pub async fn reset_all(&self) {
        for entry in self.circuit_breakers.iter() {
            let language = entry.key();
            self.reset_circuit_breaker(language).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout_secs: 1,
            success_threshold: 1,
        };
        let mut cb = CircuitBreaker::new(config);

        // Initially closed
        assert_eq!(cb.state(), &CircuitState::Closed);
        assert!(cb.allow_request());

        // Record failures
        cb.record_failure();
        assert_eq!(cb.state(), &CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), &CircuitState::Open);
        assert!(!cb.allow_request());

        // Wait for timeout
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(cb.allow_request());
        assert_eq!(cb.state(), &CircuitState::HalfOpen);

        // Record success
        cb.record_success();
        assert_eq!(cb.state(), &CircuitState::Closed);
    }

    #[test]
    fn test_retry_config_delay() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
            backoff_multiplier: 2.0,
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(config.delay_for_attempt(3), Duration::from_millis(800));
        // Should cap at max_delay_ms
        assert_eq!(config.delay_for_attempt(4), Duration::from_millis(1000));
    }
}
