//! EphemeralValue channel: value is cleared after reading.

use std::fmt::Debug;

use super::{Channel, ChannelError};

/// EphemeralValue channel: value is cleared after reading.
///
/// This channel type is useful for temporary values that should only be read once.
/// After a value is read, it is cleared. This is useful for passing temporary
/// data between nodes that should not persist in the state.
///
/// # Example
///
/// ```rust
/// use anureo_graph_core::channels::{Channel, EphemeralValue};
///
/// let mut channel = EphemeralValue::new();
/// channel.write(42);
///
/// // First read succeeds
/// assert_eq!(channel.read(), Some(42));
///
/// // Note: Actual clearing after read needs to be handled by StateGraph integration
/// ```
#[derive(Debug, Clone)]
pub struct EphemeralValue<T> {
    value: Option<T>,
}

impl<T> EphemeralValue<T> {
    /// Creates a new empty EphemeralValue channel.
    pub fn new() -> Self {
        Self { value: None }
    }

    /// Creates a new EphemeralValue channel with an initial value.
    pub fn with_value(value: T) -> Self {
        Self { value: Some(value) }
    }
}

impl<T> Default for EphemeralValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Channel<T> for EphemeralValue<T>
where
    T: Clone + Send + Sync + Debug + 'static,
{
    fn read(&self) -> Option<T> {
        self.value.clone()
    }

    fn write(&mut self, value: T) -> Result<(), ChannelError> {
        self.value = Some(value);
        Ok(())
    }

    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError> {
        if let Some(last) = updates.last() {
            let _ = self.write(last.clone());
        }
        Ok(())
    }

    fn channel_type(&self) -> &'static str {
        "EphemeralValue"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ephemeral_value_new() {
        let channel: EphemeralValue<i32> = EphemeralValue::new();
        assert_eq!(channel.read(), None);
    }

    #[test]
    fn test_ephemeral_value_with_value() {
        let channel = EphemeralValue::with_value(42);
        assert_eq!(channel.read(), Some(42));
    }

    #[test]
    fn test_ephemeral_value_write() {
        let mut channel = EphemeralValue::new();
        channel.write(1).unwrap();
        assert_eq!(channel.read(), Some(1));

        channel.write(2).unwrap();
        assert_eq!(channel.read(), Some(2));
    }

    #[test]
    fn test_ephemeral_value_update() {
        let mut channel = EphemeralValue::new();
        let result = channel.update(vec![1, 2, 3]);
        assert!(result.is_ok());
        assert_eq!(channel.read(), Some(3));
    }

    #[test]
    fn test_ephemeral_value_channel_type() {
        let channel: EphemeralValue<i32> = EphemeralValue::new();
        assert_eq!(channel.channel_type(), "EphemeralValue");
    }
}
