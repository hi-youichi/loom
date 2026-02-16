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
/// use graphweave::channels::{Channel, BinaryOperatorAggregate};
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
/// Note: This example requires `use graphweave::channels::Channel;` to be in scope.
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

    fn write(&mut self, value: T) {
        if let Some(current) = self.value.take() {
            self.value = Some((self.reducer)(current, value));
        } else {
            self.value = Some(value);
        }
    }

    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError> {
        for update in updates {
            self.write(update);
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
    fn test_binop_sum() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        channel.write(1);
        channel.write(2);
        channel.write(3);
        assert_eq!(channel.read(), Some(6));
    }

    #[test]
    fn test_binop_product() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a * b);
        channel.write(2);
        channel.write(3);
        channel.write(4);
        assert_eq!(channel.read(), Some(24));
    }

    #[test]
    fn test_binop_update() {
        let mut channel = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        channel.update(vec![1, 2, 3]).unwrap();
        assert_eq!(channel.read(), Some(6));
    }

    #[test]
    fn test_binop_with_initial_value() {
        let mut channel = BinaryOperatorAggregate::with_value(10, |a: i32, b: i32| a + b);
        channel.write(5);
        assert_eq!(channel.read(), Some(15));
    }

    #[test]
    fn test_binop_list_append() {
        let mut channel = BinaryOperatorAggregate::new(|mut a: Vec<i32>, b: Vec<i32>| {
            a.extend(b);
            a
        });
        channel.write(vec![1, 2]);
        channel.write(vec![3, 4]);
        assert_eq!(channel.read(), Some(vec![1, 2, 3, 4]));
    }
}
