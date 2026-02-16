//! LastValue channel: keeps only the last written value.

use std::fmt::Debug;

use super::{Channel, ChannelError};

/// LastValue channel: keeps only the last written value.
///
/// This is the default channel type for most state fields. When multiple nodes
/// write to a field using LastValue, only the last value is kept.
///
/// # Example
///
/// ```rust
/// use graphweave::channels::{Channel, LastValue};
///
/// let mut channel = LastValue::new();
/// channel.write(1);
/// channel.write(2);
/// channel.write(3);
///
/// assert_eq!(channel.read(), Some(3));
/// ```
///
/// Note: This example requires `use graphweave::channels::Channel;` to be in scope.
#[derive(Debug, Clone)]
pub struct LastValue<T> {
    value: Option<T>,
}

impl<T> LastValue<T> {
    /// Creates a new empty LastValue channel.
    pub fn new() -> Self {
        Self { value: None }
    }

    /// Creates a new LastValue channel with an initial value.
    pub fn with_value(value: T) -> Self {
        Self { value: Some(value) }
    }
}

impl<T> Default for LastValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Channel<T> for LastValue<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    fn read(&self) -> Option<T> {
        self.value.clone()
    }

    fn write(&mut self, value: T) {
        self.value = Some(value);
    }

    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError> {
        if let Some(last) = updates.last() {
            self.write(last.clone());
        }
        Ok(())
    }

    fn channel_type(&self) -> &'static str {
        "LastValue"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_value_basic() {
        let mut channel = LastValue::new();
        assert_eq!(channel.read(), None);

        channel.write(1);
        assert_eq!(channel.read(), Some(1));

        channel.write(2);
        assert_eq!(channel.read(), Some(2));
    }

    #[test]
    fn test_last_value_update() {
        let mut channel = LastValue::new();
        channel.update(vec![1, 2, 3]).unwrap();
        assert_eq!(channel.read(), Some(3));
    }

    #[test]
    fn test_last_value_with_initial_value() {
        let channel = LastValue::with_value(42);
        assert_eq!(channel.read(), Some(42));
    }
}
