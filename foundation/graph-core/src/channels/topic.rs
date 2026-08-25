//! Topic channel for message list accumulation.
//!
//! A configurable PubSub Topic that accumulates values into a list.
//! Topic channel for accumulating values.
//!
//! # Features
//!
//! - Accumulates values into a vector
//! - Optional accumulation across steps (if `accumulate` is false, clears on each step)
//! - Supports both single values and lists as updates
//!
//! # Example
//!
//! ```rust
//! use anureo_graph_core::channels::{Topic, TopicSingleWrite, Channel};
//!
//! // Create a topic that accumulates across steps
//! let mut topic: Topic<String> = Topic::new(true);
//!
//! // Write multiple values using write_single
//! topic.write_single("message1".to_string());
//! topic.write_single("message2".to_string());
//!
//! // Read all values
//! assert_eq!(topic.read(), Some(vec!["message1".to_string(), "message2".to_string()]));
//! ```

use super::{Channel, ChannelError};
use std::fmt::Debug;

/// A configurable PubSub Topic channel that accumulates values into a list.
///
/// # Type Parameters
///
/// - `T`: The type of values stored in the topic
///
/// # Fields
///
/// - `values`: The accumulated values
/// - `accumulate`: Whether to accumulate values across steps
///
/// # Behavior
///
/// - When `accumulate` is `true`: Values persist across update cycles
/// - When `accumulate` is `false`: Values are cleared at the start of each update cycle
///
/// # Interaction
///
/// - Used by `StateGraph` for message list fields (e.g., chat history)
/// - Works with `CompiledStateGraph::run_loop` for state updates
#[derive(Debug, Clone)]
pub struct Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// The accumulated values in the topic.
    values: Vec<T>,
    /// Whether to accumulate values across steps.
    accumulate: bool,
}

impl<T> Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new Topic channel.
    ///
    /// # Arguments
    ///
    /// - `accumulate`: If true, values persist across update cycles; if false, values are cleared at each update
    pub fn new(accumulate: bool) -> Self {
        Self {
            values: Vec::new(),
            accumulate,
        }
    }

    /// Creates a new Topic channel with initial values.
    ///
    /// # Arguments
    ///
    /// - `values`: Initial values to add to the topic
    /// - `accumulate`: If true, values persist across update cycles
    pub fn with_values(values: Vec<T>, accumulate: bool) -> Self {
        Self { values, accumulate }
    }

    /// Writes a single value to the topic.
    ///
    /// # Arguments
    ///
    /// - `value`: The value to append to the topic
    pub fn write_single(&mut self, value: T) {
        self.values.push(value);
    }

    /// Clears all values from the topic.
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Returns the number of values in the topic.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true if the topic contains no values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Gets a reference to the underlying values.
    pub fn get_values(&self) -> &[T] {
        &self.values
    }
}

impl<T> Default for Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    fn default() -> Self {
        Self::new(true)
    }
}

/// Convenience wrapper for writing single values to a Topic.
///
/// This is a marker trait that indicates a channel supports single-value writes.
pub trait TopicSingleWrite<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Writes a single value to the topic.
    fn write_single(&mut self, value: T);
}

impl<T> TopicSingleWrite<T> for Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    fn write_single(&mut self, value: T) {
        self.write_single(value);
    }
}

impl<T> Channel<Vec<T>> for Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    fn read(&self) -> Option<Vec<T>> {
        if self.values.is_empty() {
            None
        } else {
            Some(self.values.clone())
        }
    }

    fn write(&mut self, value: Vec<T>) -> Result<(), ChannelError> {
        self.values.extend(value);
        Ok(())
    }

    fn update(&mut self, updates: Vec<Vec<T>>) -> Result<(), ChannelError> {
        if !self.accumulate {
            self.values.clear();
        }
        for values in updates {
            self.values.extend(values);
        }
        Ok(())
    }

    fn channel_type(&self) -> &'static str {
        "Topic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_new() {
        let topic: Topic<String> = Topic::new(true);
        assert!(topic.is_empty());
        assert_eq!(topic.len(), 0);
    }

    #[test]
    fn test_topic_write_single() {
        let mut topic: Topic<String> = Topic::new(true);
        topic.write_single("msg1".to_string());
        topic.write_single("msg2".to_string());

        assert_eq!(topic.len(), 2);
        assert_eq!(
            topic.read(),
            Some(vec!["msg1".to_string(), "msg2".to_string()])
        );
    }

    #[test]
    fn test_topic_write() {
        let mut topic: Topic<i32> = Topic::new(true);
        topic.write(vec![1, 2, 3]).unwrap();

        assert_eq!(topic.len(), 3);
        assert_eq!(topic.read(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_topic_accumulate_true() {
        let mut topic: Topic<i32> = Topic::new(true);

        topic.update(vec![vec![1, 2]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2]));

        topic.update(vec![vec![3, 4]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2, 3, 4])); // Accumulates
    }

    #[test]
    fn test_topic_accumulate_false() {
        let mut topic: Topic<i32> = Topic::new(false);

        topic.update(vec![vec![1, 2]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2]));

        topic.update(vec![vec![3, 4]]).unwrap();
        assert_eq!(topic.read(), Some(vec![3, 4])); // Replaces
    }

    #[test]
    fn test_topic_clear() {
        let mut topic: Topic<String> = Topic::new(true);
        topic.write_single("msg1".to_string());
        topic.write_single("msg2".to_string());

        assert_eq!(topic.len(), 2);

        topic.clear();
        assert!(topic.is_empty());
    }

    #[test]
    fn test_topic_with_values() {
        let topic = Topic::with_values(vec![1, 2, 3], true);
        assert_eq!(topic.len(), 3);
        assert_eq!(topic.read(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_topic_get_values() {
        let mut topic: Topic<String> = Topic::new(true);
        topic.write_single("msg1".to_string());
        topic.write_single("msg2".to_string());

        let values = topic.get_values();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], "msg1");
        assert_eq!(values[1], "msg2");
    }

    #[test]
    fn test_topic_channel_type() {
        let topic: Topic<i32> = Topic::new(true);
        assert_eq!(topic.channel_type(), "Topic");
    }

    #[test]
    fn test_topic_default() {
        let topic: Topic<i32> = Topic::default();
        assert!(topic.accumulate); // Default is accumulate = true
    }
}
