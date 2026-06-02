//! Retry mechanism for node execution.
//!
//! Provides retry policies for handling transient failures during graph execution.

use std::time::Duration;

/// Retry policy for handling failures.
///
/// Defines how many times and with what strategy to retry a failed operation.
#[derive(Debug, Clone, Default)]
pub enum RetryPolicy {
    /// No retry - fail immediately on error.
    #[default]
    None,
    /// Fixed interval retry - retry with a constant delay between attempts.
    Fixed {
        /// Maximum number of retry attempts.
        max_attempts: usize,
        /// Fixed interval between retries.
        interval: Duration,
    },
    /// Exponential backoff retry - retry with exponentially increasing delays.
    Exponential {
        /// Maximum number of retry attempts.
        max_attempts: usize,
        /// Initial interval before the first retry.
        initial_interval: Duration,
        /// Maximum interval cap (won't exceed this).
        max_interval: Duration,
        /// Multiplier for exponential backoff (e.g., 2.0 doubles each time).
        multiplier: f64,
    },
}

impl RetryPolicy {
    /// Creates a new retry policy with no retries.
    pub fn none() -> Self {
        RetryPolicy::None
    }

    /// Creates a new fixed interval retry policy.
    pub fn fixed(max_attempts: usize, interval: Duration) -> Self {
        RetryPolicy::Fixed {
            max_attempts,
            interval,
        }
    }

    /// Creates a new exponential backoff retry policy.
    pub fn exponential(
        max_attempts: usize,
        initial_interval: Duration,
        max_interval: Duration,
        multiplier: f64,
    ) -> Self {
        RetryPolicy::Exponential {
            max_attempts,
            initial_interval,
            max_interval,
            multiplier,
        }
    }

    /// Gets the maximum number of retry attempts.
    pub fn max_attempts(&self) -> usize {
        match self {
            RetryPolicy::None => 1, // Only the initial attempt
            RetryPolicy::Fixed { max_attempts, .. } => *max_attempts,
            RetryPolicy::Exponential { max_attempts, .. } => *max_attempts,
        }
    }

    /// Calculates the delay for a given retry attempt.
    ///
    /// `attempt` is 1-indexed (1 = first retry, 2 = second retry, etc.)
    pub fn delay_for_attempt(&self, attempt: usize) -> Option<Duration> {
        match self {
            RetryPolicy::None => None,
            RetryPolicy::Fixed { interval, .. } => {
                if attempt <= self.max_attempts() {
                    Some(*interval)
                } else {
                    None
                }
            }
            RetryPolicy::Exponential {
                initial_interval,
                max_interval,
                multiplier,
                ..
            } => {
                if attempt > self.max_attempts() {
                    return None;
                }
                
                let delay_ms = initial_interval.as_millis() as f64 * multiplier.powi(attempt as i32 - 1);
                let max_ms = max_interval.as_millis() as f64;
                let clamped_ms = delay_ms.min(max_ms);
                Some(Duration::from_millis(clamped_ms as u64))
            }
        }
    }

    /// Checks if this policy allows retries.
    pub fn allows_retries(&self) -> bool {
        matches!(self, RetryPolicy::Fixed { .. } | RetryPolicy::Exponential { .. })
    }
    
    /// Checks if we should retry for the given attempt number.
    pub fn should_retry(&self, attempt: usize) -> bool {
        attempt < self.max_attempts()
    }
    
    /// Gets the delay for a given attempt number.
    pub fn delay(&self, attempt: usize) -> std::time::Duration {
        self.delay_for_attempt(attempt).unwrap_or(std::time::Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_retry_policy_none() {
        let policy = RetryPolicy::none();
        assert!(!policy.allows_retries());
        assert_eq!(policy.max_attempts(), 1);
        assert_eq!(policy.delay_for_attempt(1), None);
    }

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::fixed(3, Duration::from_millis(100));
        assert!(policy.allows_retries());
        assert_eq!(policy.max_attempts(), 3);
        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_millis(100)));
        assert_eq!(policy.delay_for_attempt(2), Some(Duration::from_millis(100)));
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_millis(100)));
        assert_eq!(policy.delay_for_attempt(4), None);
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::exponential(
            4,
            Duration::from_millis(100),
            Duration::from_millis(5000),
            2.0,
        );
        assert!(policy.allows_retries());
        assert_eq!(policy.max_attempts(), 4);
        
        // First retry: 100ms * 2^0 = 100ms
        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_millis(100)));
        
        // Second retry: 100ms * 2^1 = 200ms
        assert_eq!(policy.delay_for_attempt(2), Some(Duration::from_millis(200)));
        
        // Third retry: 100ms * 2^2 = 400ms
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_millis(400)));
        
        // Fourth retry: 100ms * 2^3 = 800ms
        assert_eq!(policy.delay_for_attempt(4), Some(Duration::from_millis(800)));
        
        // Fifth retry exceeds max_attempts
        assert_eq!(policy.delay_for_attempt(5), None);
    }

    #[test]
    fn test_retry_policy_exponential_max_cap() {
        let policy = RetryPolicy::exponential(
            10,
            Duration::from_millis(1000),
            Duration::from_millis(5000), // Cap at 5 seconds
            2.0,
        );
        
        // These delays should be capped at 5 seconds
        assert_eq!(policy.delay_for_attempt(5), Some(Duration::from_millis(5000)));
        assert_eq!(policy.delay_for_attempt(10), Some(Duration::from_millis(5000)));
    }
}