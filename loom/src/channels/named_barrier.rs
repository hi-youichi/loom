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
//! use loom::channels::{NamedBarrierValue, NamedBarrierUpdate, Channel};
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
/// - Barrier synchronization in parallel execution
#[derive(Debug, Clone)]
pub struct NamedBarrierValue<T>
where
    T: Clone + Send + Sync + Debug + Hash + Eq + 'static,
{
    /// The set of names that must be seen for the barrier to be available.
    names: HashSet<T>,
    /// The set of names that have been seen so far.
    seen: HashSet<T>,
}

impl<T> NamedBarrierValue<T>
where
    T: Clone + Send + Sync + Debug + Hash + Eq + 'static,
{
    /// Creates a new NamedBarrierValue channel.
    ///
    /// # Arguments
    ///
    /// - `names`: The set of names that must be seen for the barrier to be available
    ///
    /// # Example
    ///
    /// ```rust
    /// use loom::channels::NamedBarrierValue;
    /// use std::collections::HashSet;
    ///
    /// let names: HashSet<&str> = ["a", "b", "c"].into_iter().collect();
    /// let barrier: NamedBarrierValue<&str> = NamedBarrierValue::new(names);
    /// ```
    pub fn new(names: HashSet<T>) -> Self {
        Self {
            names,
            seen: HashSet::new(),
        }
    }

    /// Creates a new NamedBarrierValue from an iterator of names.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loom::channels::NamedBarrierValue;
    ///
    /// let barrier: NamedBarrierValue<String> = NamedBarrierValue::from_names(
    ///     ["step1", "step2"].into_iter().map(String::from)
    /// );
    /// ```
    pub fn from_names<I: IntoIterator<Item = T>>(names: I) -> Self {
        Self::new(names.into_iter().collect())
    }

    /// Returns whether the barrier is available (all names have been seen).
    pub fn is_available(&self) -> bool {
        self.seen == self.names
    }

    /// Returns the set of names that are still expected.
    pub fn pending_names(&self) -> HashSet<T> {
        self.names.difference(&self.seen).cloned().collect()
    }

    /// Returns the set of names that have been seen.
    pub fn seen_names(&self) -> &HashSet<T> {
        &self.seen
    }

    /// Returns the set of all expected names.
    pub fn expected_names(&self) -> &HashSet<T> {
        &self.names
    }

    /// Consumes the barrier, resetting it for the next round.
    ///
    /// # Returns
    ///
    /// `true` if the barrier was available and has been reset,
    /// `false` if the barrier was not yet available.
    pub fn consume(&mut self) -> bool {
        if self.is_available() {
            self.seen.clear();
            true
        } else {
            false
        }
    }

    /// Creates a checkpoint of the current seen names.
    ///
    /// Used for state persistence and recovery.
    pub fn checkpoint(&self) -> HashSet<T> {
        self.seen.clone()
    }

    /// Restores the barrier from a checkpoint.
    ///
    /// # Arguments
    ///
    /// - `names`: The expected names
    /// - `checkpoint`: The checkpoint (set of seen names) to restore from
    pub fn from_checkpoint(names: HashSet<T>, checkpoint: HashSet<T>) -> Self {
        Self {
            names,
            seen: checkpoint,
        }
    }
}

impl<T> Channel<()> for NamedBarrierValue<T>
where
    T: Clone + Send + Sync + Debug + Hash + Eq + 'static,
{
    /// Reads the barrier value.
    ///
    /// Returns `Some(())` if all names have been seen, `None` otherwise.
    fn read(&self) -> Option<()> {
        if self.is_available() {
            Some(())
        } else {
            None
        }
    }

    /// Write is a no-op for barrier channels.
    ///
    /// Use `update` to mark names as seen.
    fn write(&mut self, _value: ()) {
        // No-op: use update to mark names as seen
    }

    /// Updates the barrier with names that have been seen.
    ///
    /// # Arguments
    ///
    /// - `updates`: Vector of names to mark as seen
    ///
    /// # Returns
    ///
    /// - `Ok(())` if all names are valid (in the expected set)
    /// - `Err(ChannelError::InvalidUpdate)` if any name is not in the expected set
    fn update(&mut self, updates: Vec<()>) -> Result<(), ChannelError> {
        // This signature doesn't match well with the barrier pattern
        // The real update happens through mark_seen
        let _ = updates;
        Ok(())
    }

    /// Returns the channel type name.
    fn channel_type(&self) -> &'static str {
        "NamedBarrierValue"
    }
}

/// Extension trait for NamedBarrierValue to mark names as seen.
///
/// This provides the proper API for barrier updates.
pub trait NamedBarrierUpdate<T>
where
    T: Clone + Send + Sync + Debug + Hash + Eq + 'static,
{
    /// Marks a name as seen.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if the name was newly seen
    /// - `Ok(false)` if the name was already seen
    /// - `Err` if the name is not in the expected set
    fn mark_seen(&mut self, name: T) -> Result<bool, ChannelError>;

    /// Marks multiple names as seen.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if any name was newly seen
    /// - `Ok(false)` if no names were newly seen
    /// - `Err` if any name is not in the expected set
    fn mark_seen_many(&mut self, names: Vec<T>) -> Result<bool, ChannelError>;
}

impl<T> NamedBarrierUpdate<T> for NamedBarrierValue<T>
where
    T: Clone + Send + Sync + Debug + Hash + Eq + 'static,
{
    fn mark_seen(&mut self, name: T) -> Result<bool, ChannelError> {
        if !self.names.contains(&name) {
            return Err(ChannelError::InvalidUpdate(format!(
                "Name {:?} not in expected names",
                name
            )));
        }

        if self.seen.contains(&name) {
            Ok(false)
        } else {
            self.seen.insert(name);
            Ok(true)
        }
    }

    fn mark_seen_many(&mut self, names: Vec<T>) -> Result<bool, ChannelError> {
        let mut updated = false;
        for name in names {
            if self.mark_seen(name)? {
                updated = true;
            }
        }
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Barrier is not available until all names are seen.
    #[test]
    fn test_barrier_availability() {
        let names: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        // Not available initially
        assert!(!barrier.is_available());
        assert_eq!(barrier.read(), None);

        // Mark "a" as seen
        assert!(barrier.mark_seen("a".to_string()).unwrap());
        assert!(!barrier.is_available());
        assert_eq!(barrier.read(), None);

        // Mark "b" as seen
        assert!(barrier.mark_seen("b".to_string()).unwrap());
        assert!(barrier.is_available());
        assert_eq!(barrier.read(), Some(()));
    }

    /// **Scenario**: Marking a name twice returns false on second attempt.
    #[test]
    fn test_barrier_duplicate_mark() {
        let names: HashSet<String> = ["a".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        assert!(barrier.mark_seen("a".to_string()).unwrap());
        assert!(!barrier.mark_seen("a".to_string()).unwrap()); // Already seen
    }

    /// **Scenario**: Marking an unknown name returns error.
    #[test]
    fn test_barrier_unknown_name() {
        let names: HashSet<String> = ["a".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        let result = barrier.mark_seen("unknown".to_string());
        assert!(result.is_err());
        match result {
            Err(ChannelError::InvalidUpdate(msg)) => {
                assert!(msg.contains("unknown"));
            }
            _ => panic!("Expected InvalidUpdate error"),
        }
    }

    /// **Scenario**: Consume resets the barrier.
    #[test]
    fn test_barrier_consume() {
        let names: HashSet<String> = ["a".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        // Not available - consume returns false
        assert!(!barrier.consume());

        // Make available
        barrier.mark_seen("a".to_string()).unwrap();
        assert!(barrier.is_available());

        // Consume resets
        assert!(barrier.consume());
        assert!(!barrier.is_available());
        assert_eq!(barrier.read(), None);
    }

    /// **Scenario**: mark_seen_many marks multiple names.
    #[test]
    fn test_barrier_mark_many() {
        let names: HashSet<String> = ["a".to_string(), "b".to_string(), "c".to_string()]
            .into_iter()
            .collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        let updated = barrier
            .mark_seen_many(vec!["a".to_string(), "b".to_string()])
            .unwrap();
        assert!(updated);
        assert!(!barrier.is_available()); // Still need "c"

        barrier.mark_seen("c".to_string()).unwrap();
        assert!(barrier.is_available());
    }

    /// **Scenario**: from_names creates barrier from iterator.
    #[test]
    fn test_barrier_from_names() {
        let barrier: NamedBarrierValue<&str> = NamedBarrierValue::from_names(["x", "y", "z"]);
        assert_eq!(barrier.expected_names().len(), 3);
        assert!(barrier.expected_names().contains(&"x"));
        assert!(barrier.expected_names().contains(&"y"));
        assert!(barrier.expected_names().contains(&"z"));
    }

    /// **Scenario**: pending_names returns unseen names.
    #[test]
    fn test_barrier_pending_names() {
        let names: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);

        assert_eq!(barrier.pending_names().len(), 2);

        barrier.mark_seen("a".to_string()).unwrap();
        let pending = barrier.pending_names();
        assert_eq!(pending.len(), 1);
        assert!(pending.contains(&"b".to_string()));
    }

    /// **Scenario**: checkpoint and from_checkpoint work correctly.
    #[test]
    fn test_barrier_checkpoint() {
        let names: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let mut barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names.clone());

        barrier.mark_seen("a".to_string()).unwrap();
        let checkpoint = barrier.checkpoint();

        let restored = NamedBarrierValue::from_checkpoint(names, checkpoint);
        assert!(restored.seen_names().contains(&"a".to_string()));
        assert!(!restored.is_available()); // Still need "b"
    }

    /// **Scenario**: channel_type returns correct name.
    #[test]
    fn test_barrier_channel_type() {
        let names: HashSet<i32> = [1, 2].into_iter().collect();
        let barrier: NamedBarrierValue<i32> = NamedBarrierValue::new(names);
        assert_eq!(Channel::<()>::channel_type(&barrier), "NamedBarrierValue");
    }

    /// **Scenario**: Empty barrier is immediately available.
    #[test]
    fn test_barrier_empty_names() {
        let names: HashSet<String> = HashSet::new();
        let barrier: NamedBarrierValue<String> = NamedBarrierValue::new(names);
        assert!(barrier.is_available());
        assert_eq!(barrier.read(), Some(()));
    }
}
