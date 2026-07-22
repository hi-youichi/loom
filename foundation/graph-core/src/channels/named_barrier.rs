//! Named barrier channel for waiting until all named values are received.
//!
//! A channel that waits until all named values are received before making
//! the value available. Useful for synchronization points in graph execution.
//!
//! NamedBarrierValue channel for synchronization.
//!
//! # Example
//!
//! ```rust
//! use loom_graph_core::channels::{NamedBarrierValue, NamedBarrierUpdate, Channel};
//! use std::collections::HashSet;
//!
//! // Create a barrier waiting for "step1" and "step2"
//! let names: HashSet<String> = ["step1".to_string(), "step2".to_string()].into_iter().collect();
//! let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);
//!
//! // Barrier not ready yet
//! assert!(barrier.read().is_none());
//!
//! // Mark "step1" as seen
//! barrier.mark_seen("step1".to_string()).unwrap();
//! assert!(barrier.read().is_none()); // Still waiting for "step2"
//!
//! // Mark "step2" as seen
//! barrier.mark_seen("step2".to_string()).unwrap();
//! assert!(barrier.read().is_some()); // Now available
//! ```

use super::{Channel, ChannelError};
use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

/// A channel that waits until all named values are received before making
/// the value available.
///
/// # Type Parameters
///
/// - `T`: The type of the named values (must be hashable and comparable)
///
/// # Behavior
///
/// - Tracks a set of expected names
/// - Updates mark names as "seen"
/// - Value is only available when all expected names have been seen
/// - After being consumed, resets to wait for all names again
///
/// # Use Cases
///
/// - Synchronization points where multiple nodes must complete
/// - Fan-in patterns where all branches must finish
/// - Barrier synchronization in parallel workflows
#[derive(Debug, Clone)]
pub struct NamedBarrierValue<T>
where
    T: Eq + Hash + Clone + Send + Sync + Debug + 'static,
{
    /// The set of names we're waiting for
    expected_names: HashSet<T>,
    /// The set of names that have been seen so far
    seen_names: HashSet<T>,
    /// The accumulated value
    value: Vec<T>,
}

impl<T> NamedBarrierValue<T>
where
    T: Eq + Hash + Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new NamedBarrierValue with the expected names.
    ///
    /// # Arguments
    ///
    /// - `expected_names`: Set of names that must be received before the value becomes available
    pub fn new(expected_names: HashSet<T>) -> Self {
        Self {
            expected_names,
            seen_names: HashSet::new(),
            value: Vec::new(),
        }
    }

    /// Marks a name as seen and potentially makes the value available.
    ///
    /// # Arguments
    ///
    /// - `name`: The name to mark as seen
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the name was successfully marked
    /// - `Err(ChannelError)` if the name was not expected
    pub fn mark_seen(&mut self, name: T) -> Result<(), ChannelError> {
        if !self.expected_names.contains(&name) {
            return Err(ChannelError::InvalidUpdate(format!(
                "Unexpected name: {:?}",
                name
            )));
        }

        // Clone name before moving it into insert
        if self.seen_names.insert(name.clone()) {
            self.value.push(name);
        }

        Ok(())
    }

    /// Checks if all expected names have been seen.
    pub fn is_ready(&self) -> bool {
        self.seen_names == self.expected_names
    }

    /// Resets the barrier, clearing all seen names and values.
    pub fn reset(&mut self) {
        self.seen_names.clear();
        self.value.clear();
    }

    /// Returns the number of expected names.
    pub fn expected_count(&self) -> usize {
        self.expected_names.len()
    }

    /// Returns the number of seen names.
    pub fn seen_count(&self) -> usize {
        self.seen_names.len()
    }

    /// Returns the set of expected names.
    pub fn expected_names(&self) -> &HashSet<T> {
        &self.expected_names
    }

    /// Returns the set of seen names.
    pub fn seen_names(&self) -> &HashSet<T> {
        &self.seen_names
    }
}

impl<T> Channel<Vec<T>> for NamedBarrierValue<T>
where
    T: Eq + Hash + Clone + Send + Sync + Debug + 'static,
{
    fn read(&self) -> Option<Vec<T>> {
        if self.is_ready() {
            Some(self.value.clone())
        } else {
            None
        }
    }

    fn write(&mut self, names: Vec<T>) -> Result<(), ChannelError> {
        for name in names {
            self.mark_seen(name)?;
        }
        Ok(())
    }

    fn update(&mut self, updates: Vec<Vec<T>>) -> Result<(), ChannelError> {
        for names in updates {
            self.write(names)?;
        }
        Ok(())
    }

    fn channel_type(&self) -> &'static str {
        "NamedBarrierValue"
    }
}

/// Trait for barrier update operations.
///
/// This trait defines the interface for updating a named barrier.
pub trait NamedBarrierUpdate<T>
where
    T: Eq + Hash + Clone + Send + Sync + Debug + 'static,
{
    /// Marks a name as seen.
    fn mark_seen(&mut self, name: T) -> Result<(), ChannelError>;

    /// Checks if all expected names have been seen.
    fn is_ready(&self) -> bool;

    /// Resets the barrier.
    fn reset(&mut self);
}

impl<T> NamedBarrierUpdate<T> for NamedBarrierValue<T>
where
    T: Eq + Hash + Clone + Send + Sync + Debug + 'static,
{
    fn mark_seen(&mut self, name: T) -> Result<(), ChannelError> {
        NamedBarrierValue::mark_seen(self, name)
    }

    fn is_ready(&self) -> bool {
        NamedBarrierValue::is_ready(self)
    }

    fn reset(&mut self) {
        NamedBarrierValue::reset(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_barrier_new() {
        let names: HashSet<String> = ["step1".to_string(), "step2".to_string()]
            .into_iter()
            .collect();
        let barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        assert_eq!(barrier.expected_count(), 2);
        assert_eq!(barrier.seen_count(), 0);
        assert!(!barrier.is_ready());
        assert!(barrier.read().is_none());
    }

    #[test]
    fn test_named_barrier_mark_seen() {
        let names: HashSet<String> = ["step1".to_string(), "step2".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        // Mark first step
        barrier.mark_seen("step1".to_string()).unwrap();
        assert_eq!(barrier.seen_count(), 1);
        assert!(!barrier.is_ready());
        assert!(barrier.read().is_none());

        // Mark second step
        barrier.mark_seen("step2".to_string()).unwrap();
        assert_eq!(barrier.seen_count(), 2);
        assert!(barrier.is_ready());
        assert!(barrier.read().is_some());
    }

    #[test]
    fn test_named_barrier_duplicate_mark() {
        let names: HashSet<String> = ["step1".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        barrier.mark_seen("step1".to_string()).unwrap();
        barrier.mark_seen("step1".to_string()).unwrap(); // Should not error

        assert_eq!(barrier.seen_count(), 1); // Should still count as 1
    }

    #[test]
    fn test_named_barrier_unexpected_name() {
        let names: HashSet<String> = ["step1".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        let result = barrier.mark_seen("step2".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_named_barrier_reset() {
        let names: HashSet<String> = ["step1".to_string(), "step2".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        barrier.mark_seen("step1".to_string()).unwrap();
        barrier.mark_seen("step2".to_string()).unwrap();
        assert!(barrier.is_ready());

        barrier.reset();
        assert_eq!(barrier.seen_count(), 0);
        assert!(!barrier.is_ready());
    }

    #[test]
    fn test_named_barrier_write() {
        let names: HashSet<String> = ["step1".to_string(), "step2".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        barrier
            .write(vec!["step1".to_string(), "step2".to_string()])
            .unwrap();
        assert!(barrier.is_ready());
    }

    #[test]
    fn test_named_barrier_update() {
        let names: HashSet<String> = ["step1".to_string(), "step2".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        barrier
            .update(vec![vec!["step1".to_string()], vec!["step2".to_string()]])
            .unwrap();
        assert!(barrier.is_ready());
    }

    #[test]
    fn test_named_barrier_channel_type() {
        let names: HashSet<String> = ["step1".to_string()].into_iter().collect();
        let barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);
        assert_eq!(barrier.channel_type(), "NamedBarrierValue");
    }

    #[test]
    fn test_named_barrier_value_order() {
        let names: HashSet<String> = ["step2".to_string(), "step1".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        // Mark in different order
        barrier.mark_seen("step1".to_string()).unwrap();
        barrier.mark_seen("step2".to_string()).unwrap();

        let value = barrier.read().unwrap();
        // Value should preserve the order they were marked
        assert_eq!(value[0], "step1");
        assert_eq!(value[1], "step2");
    }
}
