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
//! use graphweave::channels::{Topic, TopicSingleWrite, Channel};
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
    /// If `false`, values are cleared at the start of each `update()` call.
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
    /// - `accumulate`: Whether to accumulate values across steps.
    ///   - `true`: Values persist across update cycles
    ///   - `false`: Values are cleared at the start of each update cycle
    ///
    /// # Example
    ///
    /// ```rust
    /// use graphweave::channels::Topic;
    ///
    /// // Topic that accumulates messages
    /// let topic: Topic<String> = Topic::new(true);
    ///
    /// // Topic that resets each step
    /// let ephemeral_topic: Topic<i32> = Topic::new(false);
    /// ```
    pub fn new(accumulate: bool) -> Self {
        Self {
            values: Vec::new(),
            accumulate,
        }
    }

    /// Creates a new accumulating Topic channel.
    ///
    /// This is a convenience method equivalent to `Topic::new(true)`.
    pub fn accumulating() -> Self {
        Self::new(true)
    }

    /// Creates a new non-accumulating Topic channel.
    ///
    /// This is a convenience method equivalent to `Topic::new(false)`.
    /// Values are cleared at the start of each update cycle.
    pub fn ephemeral() -> Self {
        Self::new(false)
    }

    /// Returns whether this topic accumulates values across steps.
    pub fn is_accumulating(&self) -> bool {
        self.accumulate
    }

    /// Returns the number of values in the topic.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the topic is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Clears all values from the topic.
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Extends the topic with values from an iterator.
    ///
    /// # Arguments
    ///
    /// - `iter`: An iterator of values to add
    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.values.extend(iter);
    }

    /// Creates a checkpoint of the current values.
    ///
    /// Used for state persistence and recovery.
    pub fn checkpoint(&self) -> Vec<T> {
        self.values.clone()
    }

    /// Restores the topic from a checkpoint.
    ///
    /// # Arguments
    ///
    /// - `checkpoint`: The checkpoint to restore from
    pub fn from_checkpoint(checkpoint: Vec<T>, accumulate: bool) -> Self {
        Self {
            values: checkpoint,
            accumulate,
        }
    }
}

impl<T> Default for Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new accumulating Topic channel by default.
    fn default() -> Self {
        Self::accumulating()
    }
}

impl<T> Channel<Vec<T>> for Topic<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Reads all accumulated values from the topic.
    ///
    /// Returns `None` if the topic is empty, otherwise returns a clone
    /// of all values.
    fn read(&self) -> Option<Vec<T>> {
        if self.values.is_empty() {
            None
        } else {
            Some(self.values.clone())
        }
    }

    /// Writes a single value (as a vec) to the topic.
    ///
    /// Note: For single value writes, use `update` with a single-element vector,
    /// or use the `TopicSingleWrite` extension trait.
    fn write(&mut self, value: Vec<T>) {
        self.values.extend(value);
    }

    /// Updates the topic with multiple values.
    ///
    /// If `accumulate` is `false`, clears existing values before adding new ones.
    ///
    /// # Arguments
    ///
    /// - `updates`: Vector of value vectors to add
    ///
    /// # Returns
    ///
    /// - `Ok(())` on success
    fn update(&mut self, updates: Vec<Vec<T>>) -> Result<(), ChannelError> {
        // Clear if not accumulating
        if !self.accumulate {
            self.values.clear();
        }

        // Flatten and extend
        for batch in updates {
            self.values.extend(batch);
        }

        Ok(())
    }

    /// Returns the channel type name.
    fn channel_type(&self) -> &'static str {
        "Topic"
    }
}

/// Extension trait for writing single values to a Topic.
///
/// This provides a more ergonomic API for adding individual values.
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
    /// Writes a single value to the topic.
    ///
    /// # Example
    ///
    /// ```rust
    /// use graphweave::channels::{Topic, TopicSingleWrite, Channel};
    ///
    /// let mut topic: Topic<String> = Topic::new(true);
    /// topic.write_single("hello".to_string());
    /// topic.write_single("world".to_string());
    ///
    /// assert_eq!(topic.read(), Some(vec!["hello".to_string(), "world".to_string()]));
    /// ```
    fn write_single(&mut self, value: T) {
        self.values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Topic accumulates values when accumulate=true.
    #[test]
    fn test_topic_accumulating() {
        let mut topic: Topic<i32> = Topic::new(true);

        // First update
        topic.update(vec![vec![1, 2]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2]));

        // Second update - values should accumulate
        topic.update(vec![vec![3, 4]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2, 3, 4]));
    }

    /// **Scenario**: Topic clears values when accumulate=false.
    #[test]
    fn test_topic_ephemeral() {
        let mut topic: Topic<i32> = Topic::new(false);

        // First update
        topic.update(vec![vec![1, 2]]).unwrap();
        assert_eq!(topic.read(), Some(vec![1, 2]));

        // Second update - values should be cleared first
        topic.update(vec![vec![3, 4]]).unwrap();
        assert_eq!(topic.read(), Some(vec![3, 4]));
    }

    /// **Scenario**: Empty topic returns None on read.
    #[test]
    fn test_topic_empty_read() {
        let topic: Topic<i32> = Topic::new(true);
        assert_eq!(topic.read(), None);
    }

    /// **Scenario**: Topic write extends values.
    #[test]
    fn test_topic_write() {
        let mut topic: Topic<String> = Topic::new(true);
        topic.write(vec!["a".to_string(), "b".to_string()]);
        topic.write(vec!["c".to_string()]);
        assert_eq!(
            topic.read(),
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    /// **Scenario**: TopicSingleWrite allows adding single values.
    #[test]
    fn test_topic_single_write() {
        let mut topic: Topic<i32> = Topic::new(true);
        topic.write_single(1);
        topic.write_single(2);
        topic.write_single(3);
        assert_eq!(topic.read(), Some(vec![1, 2, 3]));
    }

    /// **Scenario**: Topic convenience constructors work correctly.
    #[test]
    fn test_topic_constructors() {
        let acc: Topic<i32> = Topic::accumulating();
        assert!(acc.is_accumulating());

        let eph: Topic<i32> = Topic::ephemeral();
        assert!(!eph.is_accumulating());

        let def: Topic<i32> = Topic::default();
        assert!(def.is_accumulating());
    }

    /// **Scenario**: Topic checkpoint and restore work correctly.
    #[test]
    fn test_topic_checkpoint() {
        let mut topic: Topic<i32> = Topic::new(true);
        topic.write_single(1);
        topic.write_single(2);

        let checkpoint = topic.checkpoint();

        let restored = Topic::from_checkpoint(checkpoint, true);
        assert_eq!(restored.read(), Some(vec![1, 2]));
        assert!(restored.is_accumulating());
    }

    /// **Scenario**: Topic len and is_empty work correctly.
    #[test]
    fn test_topic_len() {
        let mut topic: Topic<i32> = Topic::new(true);
        assert!(topic.is_empty());
        assert_eq!(topic.len(), 0);

        topic.write_single(1);
        assert!(!topic.is_empty());
        assert_eq!(topic.len(), 1);

        topic.write_single(2);
        assert_eq!(topic.len(), 2);
    }

    /// **Scenario**: Topic clear removes all values.
    #[test]
    fn test_topic_clear() {
        let mut topic: Topic<i32> = Topic::new(true);
        topic.write_single(1);
        topic.write_single(2);
        assert_eq!(topic.len(), 2);

        topic.clear();
        assert!(topic.is_empty());
        assert_eq!(topic.read(), None);
    }

    /// **Scenario**: Topic extend adds multiple values.
    #[test]
    fn test_topic_extend() {
        let mut topic: Topic<i32> = Topic::new(true);
        topic.extend(vec![1, 2, 3]);
        assert_eq!(topic.read(), Some(vec![1, 2, 3]));

        topic.extend([4, 5].into_iter());
        assert_eq!(topic.read(), Some(vec![1, 2, 3, 4, 5]));
    }

    /// **Scenario**: Topic channel_type returns correct name.
    #[test]
    fn test_topic_channel_type() {
        let topic: Topic<i32> = Topic::new(true);
        assert_eq!(Channel::<Vec<i32>>::channel_type(&topic), "Topic");
    }

    /// **Scenario**: Topic can be used as trait object.
    #[test]
    fn test_topic_trait_object() {
        let mut channel: Box<dyn Channel<Vec<i32>>> = Box::new(Topic::new(true));
        channel.write(vec![1, 2, 3]);
        assert_eq!(channel.read(), Some(vec![1, 2, 3]));
        assert_eq!(channel.channel_type(), "Topic");
    }
}
