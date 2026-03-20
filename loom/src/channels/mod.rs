//! Channels for graph-state aggregation strategies.
//!
//! A [`Channel`] defines how concurrent or repeated writes to one logical state
//! slot are merged between graph steps. Different implementations model
//! different semantics:
//!
//! - `LastValue`: Keeps only the last written value
//! - `EphemeralValue`: Value is cleared after reading
//! - `BinaryOperatorAggregate`: Aggregates values using a binary operator
//! - `Topic`: Accumulates values into a list (for message history, etc.)
//! - `NamedBarrierValue`: Waits until all named values are received
//!
//! Additionally, [`StateUpdater`] customizes how whole-node outputs are merged
//! back into the graph state:
//!
//! - `ReplaceUpdater`: Default, replaces entire state
//! - `FieldBasedUpdater`: Custom per-field update logic
//!
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

/// Aggregation contract for one state slot.
///
/// Channel implementations decide how a sequence of writes should be combined
/// before the next graph step observes them.
pub trait Channel<T>: Send + Sync + Debug
where
    T: Clone + Send + Sync + Debug + 'static,
{
    /// Reads the current channel value, if any.
    fn read(&self) -> Option<T>;

    /// Applies one write to the channel.
    ///
    /// The merge behavior is implementation-defined.
    fn write(&mut self, value: T);

    /// Applies multiple writes to the channel in one batch.
    fn update(&mut self, updates: Vec<T>) -> Result<(), ChannelError>;

    /// Returns a stable channel type name for debugging and introspection.
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
