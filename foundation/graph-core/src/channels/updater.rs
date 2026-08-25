//! State updater for custom state merge semantics.
//!
//! This module provides traits and implementations for customizing how state updates
//! are applied in the graph execution. By default, state is fully replaced by the node's
//! return value. Custom updaters can implement more sophisticated merge logic.
//!
//! # Background
//!
//! Per-field update strategies (similar to `Annotated` in graph frameworks):
//!
//! ```python
//! class State(TypedDict):
//!     messages: Annotated[list, add_messages]  # Append new messages
//!     count: int                                # Replace value
//! ```
//!
//! In Rust, we use `StateUpdater` trait to achieve similar functionality at the type level.
//!
//! # Example
//!
//! ```rust,ignore
//! use anureo_graph_core::channels::{StateUpdater, ReplaceUpdater};
//!
//! // Custom state type
//! #[derive(Clone, Debug)]
//! struct MyState {
//!     messages: Vec<String>,
//!     count: i32,
//! }
//!
//! // Custom updater that appends messages and adds counts
//! struct MyStateUpdater;
//!
//! impl StateUpdater<MyState> for MyStateUpdater {
//!     fn apply_update(&self, current: &mut MyState, update: &MyState) {
//!         // Append messages instead of replacing
//!         current.messages.extend(update.messages.iter().cloned());
//!         // Add counts instead of replacing
//!         current.count += update.count;
//!     }
//! }
//! ```

use std::fmt::Debug;
use std::sync::Arc;

/// Trait for customizing how state updates are applied.
///
/// Implement this trait to define custom merge logic for your state type.
/// The default implementation (`ReplaceUpdater`) simply replaces the entire state.
pub trait StateUpdater<S>: Send + Sync + Debug
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Applies an update to the current state.
    ///
    /// This method is called when a node returns a new state. The updater can
    /// decide how to merge the update into the current state.
    ///
    /// # Arguments
    ///
    /// - `current`: Mutable reference to the current state (will be modified)
    /// - `update`: The update state returned by the node
    fn apply_update(&self, current: &mut S, update: &S);
}

/// Default state updater that replaces the entire state.
///
/// This is the default behavior for most graph execution scenarios.
#[derive(Debug, Clone)]
pub struct ReplaceUpdater;

impl<S> StateUpdater<S> for ReplaceUpdater
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn apply_update(&self, current: &mut S, update: &S) {
        *current = update.clone();
    }
}

/// Field-based state updater for per-field update semantics.
///
/// This updater allows you to customize how individual fields are updated.
#[derive(Debug, Clone)]
pub struct FieldBasedUpdater<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    _phantom: std::marker::PhantomData<S>,
}

impl<S> Default for FieldBasedUpdater<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<S> StateUpdater<S> for FieldBasedUpdater<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn apply_update(&self, current: &mut S, update: &S) {
        // Default behavior is replace for FieldBasedUpdater
        // Custom implementations should override this
        *current = update.clone();
    }
}

/// Boxed state updater for dynamic dispatch.
///
/// Useful when you need to store updaters in collections or pass them as trait objects.
pub type BoxedStateUpdater<S> = Arc<dyn StateUpdater<S>>;

/// Helper function to create a boxed state updater.
///
/// # Arguments
///
/// - `updater`: The updater to box
///
/// # Returns
///
/// A boxed state updater that can be stored and used dynamically
pub fn boxed_updater<S>(updater: impl StateUpdater<S> + 'static) -> BoxedStateUpdater<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    Arc::new(updater)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_updater() {
        let updater = ReplaceUpdater;
        let mut current = vec![1, 2, 3];
        let update = vec![4, 5, 6];

        updater.apply_update(&mut current, &update);
        assert_eq!(current, vec![4, 5, 6]); // Replaced entirely
    }

    #[test]
    fn test_replace_updater_simple_types() {
        let updater = ReplaceUpdater;
        let mut current = 10;
        let update = 20;

        updater.apply_update(&mut current, &update);
        assert_eq!(current, 20);
    }

    #[test]
    fn test_field_based_updater_default() {
        let updater = FieldBasedUpdater::<Vec<i32>>::default();
        let mut current = vec![1, 2, 3];
        let update = vec![4, 5, 6];

        updater.apply_update(&mut current, &update);
        assert_eq!(current, vec![4, 5, 6]); // Default is replace
    }

    #[test]
    fn test_custom_updater() {
        #[derive(Debug)]
        struct AppendUpdater;

        impl StateUpdater<Vec<i32>> for AppendUpdater {
            fn apply_update(&self, current: &mut Vec<i32>, update: &Vec<i32>) {
                current.extend(update.iter().cloned());
            }
        }

        let updater = AppendUpdater;
        let mut current = vec![1, 2, 3];
        let update = vec![4, 5, 6];

        updater.apply_update(&mut current, &update);
        assert_eq!(current, vec![1, 2, 3, 4, 5, 6]); // Appended
    }

    #[test]
    fn test_boxed_updater() {
        let mut current = vec![1, 2, 3];
        let update = vec![4, 5, 6];

        // Test with ReplaceUpdater
        let boxed_replacer: BoxedStateUpdater<Vec<i32>> = boxed_updater(ReplaceUpdater);
        boxed_replacer.apply_update(&mut current, &update);
        assert_eq!(current, vec![4, 5, 6]);

        // Reset and test with custom updater
        current = vec![1, 2, 3];

        #[derive(Debug)]
        struct AppendUpdater;
        impl StateUpdater<Vec<i32>> for AppendUpdater {
            fn apply_update(&self, current: &mut Vec<i32>, update: &Vec<i32>) {
                current.extend(update.iter().cloned());
            }
        }

        let boxed_appender: BoxedStateUpdater<Vec<i32>> = boxed_updater(AppendUpdater);
        boxed_appender.apply_update(&mut current, &update);
        assert_eq!(current, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_complex_state_updater() {
        #[derive(Clone, Debug)]
        struct ComplexState {
            messages: Vec<String>,
            count: i32,
        }

        #[derive(Debug)]
        struct ComplexStateUpdater;

        impl StateUpdater<ComplexState> for ComplexStateUpdater {
            fn apply_update(&self, current: &mut ComplexState, update: &ComplexState) {
                current.messages.extend(update.messages.iter().cloned());
                current.count += update.count;
            }
        }

        let updater = ComplexStateUpdater;
        let mut current = ComplexState {
            messages: vec!["hello".to_string()],
            count: 5,
        };
        let update = ComplexState {
            messages: vec!["world".to_string()],
            count: 3,
        };

        updater.apply_update(&mut current, &update);
        assert_eq!(current.messages, vec!["hello", "world"]);
        assert_eq!(current.count, 8); // 5 + 3
    }
}
