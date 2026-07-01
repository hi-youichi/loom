//! BinaryOperatorAggregate channel: aggregates values using a binary operator.

use std::fmt::Debug;

use super::{Channel, ChannelError};

/// BinaryOperatorAggregate channel: aggregates values using a binary operator.
///
/// This channel type applies a reducer function to aggregate multiple values.
/// The reducer is called sequentially: `reducer(accumulator, new_value)`.
///
/// # Example
///
/// ```rust
/// use loom_graph_core::channels::{Channel, BinaryOperatorAggregate};
///
/// // Sum reducer
/// let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
/// channel.write(1);
/// channel.write(2);
/// channel.write(3);
///
/// assert_eq!(channel.read(), Some(6));
/// ```
///
/// Note: This example requires `use loom_graph_core::channels::Channel;` to be in scope.
pub struct BinaryOperatorAggregate<T, F> {
    value: Option<T>,
    reducer: F,
}

impl<T, F> BinaryOperatorAggregate<T, F>
where
    F: Fn(T, T) -> T + Send + Sync + 'static,
{
    /// Creates a new BinaryOperatorAggregate channel with a reducer function.
    pub fn new(reducer: F) -> Self {
        Self {
            value: None,
            reducer,
        }
    }

    /// Creates a new BinaryOperatorAggregate channel with an initial value and reducer.
    pub fn with_value(value: T, reducer: F) -> Self {
        Self {
            value: Some(value),
            reducer,
        }
    }
}

impl<T, F> std::fmt::Debug for BinaryOperatorAggregate<T, F>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BinaryOperatorAggregate")
            .field("value", &self.value)
            .field("reducer", &"<function>")
            .finish()
    }
}

impl<T, F> Channel<T> for BinaryOperatorAggregate<T, F>
where
    T: Clone + Send + Sync + Debug + 'static,
    F: Fn(T, T) -> T + Send + Sync + 'static,
{
    fn read(&self) -> Option<T> {
        self.value.clone()
    }

    fn write(&mut self, value: T) -> Result<(), ChannelError> {
        if let Some(current) = &self.value {
            self.value = Some((self.reducer)(current.clone(), value));
        } else {
            self.value = Some(value);
        }
        Ok(())
    }

    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError> {
        for value in updates {
            let _ = self.write(value);
        }
        Ok(())
    }

    fn channel_type(&self) -> &'static str {
        "BinaryOperatorAggregate"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_operator_sum() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        channel.write(1).unwrap();
        channel.write(2).unwrap();
        channel.write(3).unwrap();
        
        assert_eq!(channel.read(), Some(6));
    }

    #[test]
    fn test_binary_operator_multiply() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a * b);
        channel.write(2).unwrap();
        channel.write(3).unwrap();
        channel.write(4).unwrap();
        
        assert_eq!(channel.read(), Some(24));
    }

    #[test]
    fn test_binary_operator_with_initial_value() {
        let mut channel = BinaryOperatorAggregate::with_value(10, |a: i32, b: i32| a + b);
        channel.write(5).unwrap();
        
        assert_eq!(channel.read(), Some(15));
    }

    #[test]
    fn test_binary_operator_string_concat() {
        let mut channel = BinaryOperatorAggregate::new(|a: String, b: String| format!("{} {}", a, b));
        channel.write("Hello".to_string()).unwrap();
        channel.write("World".to_string()).unwrap();
        channel.write("!".to_string()).unwrap();
        
        assert_eq!(channel.read(), Some("Hello World !".to_string()));
    }

    #[test]
    fn test_binary_operator_update() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        let result = channel.update(vec![1, 2, 3, 4]);
        
        assert!(result.is_ok());
        assert_eq!(channel.read(), Some(10));
    }

    #[test]
    fn test_binary_operator_channel_type() {
        let channel: BinaryOperatorAggregate<i32, _> = BinaryOperatorAggregate::new(|a, b| a + b);
        assert_eq!(channel.channel_type(), "BinaryOperatorAggregate");
    }

    #[test]
    fn test_binary_operator_max() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a.max(b));
        channel.write(5).unwrap();
        channel.write(3).unwrap();
        channel.write(8).unwrap();
        channel.write(1).unwrap();
        
        assert_eq!(channel.read(), Some(8));
    }
}