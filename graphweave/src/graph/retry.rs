//! Retry mechanism for node execution.
//!
//! Provides retry policies for handling transient failures during graph execution.

use std::time::Duration;

/// Retry policy for handling failures.
///
/// Defines how many times and with what strategy to retry a failed operation.
#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// No retry - fail immediately on error.
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

    /// Checks if a retry should be attempted for the given attempt number.
    ///
    /// Returns `true` if the attempt number is less than the maximum attempts.
    pub fn should_retry(&self, attempt: usize) -> bool {
        match self {
            RetryPolicy::None => false,
            RetryPolicy::Fixed { max_attempts, .. } => attempt < *max_attempts,
            RetryPolicy::Exponential { max_attempts, .. } => attempt < *max_attempts,
        }
    }

    /// Calculates the delay for the given attempt number.
    ///
    /// Returns `Duration::ZERO` if no retry should be attempted.
    pub fn delay(&self, attempt: usize) -> Duration {
        match self {
            RetryPolicy::None => Duration::ZERO,
            RetryPolicy::Fixed { interval, .. } => *interval,
            RetryPolicy::Exponential {
                initial_interval,
                max_interval,
                multiplier,
                ..
            } => {
                let delay_secs = initial_interval.as_secs_f64() * multiplier.powi(attempt as i32);
                let delay = Duration::from_secs_f64(delay_secs);
                delay.min(*max_interval)
            }
        }
    }

    /// Gets the maximum number of attempts for this policy.
    pub fn max_attempts(&self) -> usize {
        match self {
            RetryPolicy::None => 0,
            RetryPolicy::Fixed { max_attempts, .. } => *max_attempts,
            RetryPolicy::Exponential { max_attempts, .. } => *max_attempts,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_policy_none() {
        let policy = RetryPolicy::none();
        assert!(!policy.should_retry(0));
        assert_eq!(policy.delay(0), Duration::ZERO);
        assert_eq!(policy.max_attempts(), 0);
    }

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::fixed(3, Duration::from_secs(1));
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
        assert_eq!(policy.delay(0), Duration::from_secs(1));
        assert_eq!(policy.delay(1), Duration::from_secs(1));
        assert_eq!(policy.max_attempts(), 3);
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy =
            RetryPolicy::exponential(3, Duration::from_secs(1), Duration::from_secs(10), 2.0);
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));

        assert_eq!(policy.delay(0), Duration::from_secs(1)); // 1 * 2^0 = 1
        assert_eq!(policy.delay(1), Duration::from_secs(2)); // 1 * 2^1 = 2
        assert_eq!(policy.delay(2), Duration::from_secs(4)); // 1 * 2^2 = 4
        assert_eq!(policy.max_attempts(), 3);
    }

    #[test]
    fn test_retry_policy_exponential_max_cap() {
        let policy =
            RetryPolicy::exponential(5, Duration::from_secs(1), Duration::from_secs(5), 2.0);
        // delay(3) = 1 * 2^3 = 8, but capped at 5
        assert_eq!(policy.delay(3), Duration::from_secs(5));
    }
}
