//! Channels for state management with different update strategies.
//!
//! Channels provide different ways to aggregate and manage state updates in a graph.
//! Each channel type implements a specific update strategy:
//!
//! - `LastValue`: Keeps only the last written value
//! - `EphemeralValue`: Value is cleared after reading
//! - `BinaryOperatorAggregate`: Aggregates values using a binary operator
//! - `Topic`: Accumulates values into a list (for message history, etc.)
//! - `NamedBarrierValue`: Waits until all named values are received
//!
//! Additionally, `StateUpdater` provides a way to customize how node outputs are
//! merged into the graph state:
//!
//! - `ReplaceUpdater`: Default, replaces entire state
//! - `FieldBasedUpdater`: Custom per-field update logic
//!
//! See the implementation plans document for more details.

mod binop;
mod ephemeral_value;
mod error;
mod last_value;
mod named_barrier;
mod topic;
mod updater;

pub use binop::BinaryOperatorAggregate;
pub use ephemeral_value::EphemeralValue;
pub use error::ChannelError;
pub use last_value::LastValue;
pub use named_barrier::{NamedBarrierUpdate, NamedBarrierValue};
pub use topic::{Topic, TopicSingleWrite};
pub use updater::{
    boxed_updater, BoxedStateUpdater, FieldBasedUpdater, ReplaceUpdater, StateUpdater,
};

use std::fmt::Debug;

/// Channel trait for state management with different update strategies.
///
/// Channels are used to manage how state values are updated when multiple nodes
/// write to the same state field. Each channel type implements a specific aggregation strategy.
pub trait Channel<T>: Send + Sync + Debug
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Read the current value from the channel.
    ///
    /// Returns `None` if the channel has no value.
    fn read(&self) -> Option<T>;

    /// Write a new value to the channel.
    ///
    /// The behavior depends on the channel type:
    /// - `LastValue`: Replaces the current value
    /// - `EphemeralValue`: Sets the value (will be cleared after read)
    /// - `BinaryOperatorAggregate`: Aggregates with existing value using the reducer
    fn write(&mut self, value: T);

    /// Update the channel with multiple values.
    ///
    /// The aggregation strategy depends on the channel implementation.
    /// For example, `LastValue` keeps only the last value, while
    /// `BinaryOperatorAggregate` applies the reducer sequentially.
    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError>;

    /// Get the channel type name for debugging and introspection.
    fn channel_type(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_trait_object() {
        // Test that we can use channels as trait objects
        let mut channel: Box<dyn Channel<i32>> = Box::new(LastValue::new());
        channel.write(42);
        assert_eq!(channel.read(), Some(42));
        assert_eq!(channel.channel_type(), "LastValue");
    }
}
