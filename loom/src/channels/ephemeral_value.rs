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
/// use loom::channels::{Channel, EphemeralValue};
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
        "EphemeralValue"
    }
}

// Note: The actual clearing after read needs to be handled by the StateGraph
// integration. This channel type marks values as ephemeral, but the clearing
// logic should be implemented in the graph execution layer.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ephemeral_value_basic() {
        let mut channel = EphemeralValue::new();
        assert_eq!(channel.read(), None);

        channel.write(42);
        assert_eq!(channel.read(), Some(42));
    }

    #[test]
    fn test_ephemeral_value_update() {
        let mut channel = EphemeralValue::new();
        channel.update(vec![1, 2, 3]).unwrap();
        assert_eq!(channel.read(), Some(3));
    }

    #[test]
    fn test_ephemeral_value_with_initial_value() {
        let channel = EphemeralValue::with_value(100);
        assert_eq!(channel.read(), Some(100));
    }
}
