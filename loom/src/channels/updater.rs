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
//! use loom::channels::{StateUpdater, ReplaceUpdater};
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
    /// Apply an update to the current state.
    ///
    /// This method is called after each node execution to merge the node's
    /// output (update) into the current state.
    ///
    /// # Arguments
    ///
    /// * `current` - Mutable reference to the current state
    /// * `update` - The update returned by the node
    fn apply_update(&self, current: &mut S, update: &S);
}

/// Default state updater that replaces the entire state.
///
/// This is the default behavior: the node's return value completely replaces
/// the previous state.
#[derive(Debug, Clone, Default)]
pub struct ReplaceUpdater;

impl<S> StateUpdater<S> for ReplaceUpdater
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn apply_update(&self, current: &mut S, update: &S) {
        *current = update.clone();
    }
}

/// A state updater that applies updates field-by-field using registered field updaters.
///
/// This allows different fields to have different update strategies (e.g., LastValue, Append, etc.).
///
/// # Type Parameters
///
/// * `S` - The state type
/// * `F` - The field updater function type
pub struct FieldBasedUpdater<S, F>
where
    S: Clone + Send + Sync + Debug + 'static,
    F: Fn(&mut S, &S) + Send + Sync + 'static,
{
    /// The function that applies field-level updates
    updater_fn: F,
    _marker: std::marker::PhantomData<S>,
}

impl<S, F> Debug for FieldBasedUpdater<S, F>
where
    S: Clone + Send + Sync + Debug + 'static,
    F: Fn(&mut S, &S) + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldBasedUpdater")
            .field("updater_fn", &"<function>")
            .finish()
    }
}

impl<S, F> FieldBasedUpdater<S, F>
where
    S: Clone + Send + Sync + Debug + 'static,
    F: Fn(&mut S, &S) + Send + Sync + 'static,
{
    /// Creates a new FieldBasedUpdater with the given update function.
    ///
    /// # Arguments
    ///
    /// * `updater_fn` - A function that defines how to merge updates into the current state
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use loom::channels::FieldBasedUpdater;
    ///
    /// #[derive(Clone, Debug)]
    /// struct State { messages: Vec<String>, count: i32 }
    ///
    /// let updater = FieldBasedUpdater::new(|current: &mut State, update: &State| {
    ///     current.messages.extend(update.messages.iter().cloned());
    ///     current.count = update.count; // Replace count
    /// });
    /// ```
    pub fn new(updater_fn: F) -> Self {
        Self {
            updater_fn,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<S, F> StateUpdater<S> for FieldBasedUpdater<S, F>
where
    S: Clone + Send + Sync + Debug + 'static,
    F: Fn(&mut S, &S) + Send + Sync + 'static,
{
    fn apply_update(&self, current: &mut S, update: &S) {
        (self.updater_fn)(current, update);
    }
}

/// Boxed state updater for type erasure.
///
/// This allows storing different updater implementations in a single container.
pub type BoxedStateUpdater<S> = Arc<dyn StateUpdater<S>>;

/// Helper function to create a boxed state updater.
pub fn boxed_updater<S, U>(updater: U) -> BoxedStateUpdater<S>
where
    S: Clone + Send + Sync + Debug + 'static,
    U: StateUpdater<S> + 'static,
{
    Arc::new(updater)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestState {
        messages: Vec<String>,
        count: i32,
    }

    /// Test that ReplaceUpdater replaces the entire state.
    #[test]
    fn test_replace_updater() {
        let updater = ReplaceUpdater;
        let mut current = TestState {
            messages: vec!["old".to_string()],
            count: 10,
        };
        let update = TestState {
            messages: vec!["new".to_string()],
            count: 20,
        };

        updater.apply_update(&mut current, &update);

        assert_eq!(current.messages, vec!["new".to_string()]);
        assert_eq!(current.count, 20);
    }

    /// Test that FieldBasedUpdater can implement custom merge logic.
    #[test]
    fn test_field_based_updater_append() {
        let updater = FieldBasedUpdater::new(|current: &mut TestState, update: &TestState| {
            // Append messages
            current.messages.extend(update.messages.iter().cloned());
            // Replace count
            current.count = update.count;
        });

        let mut current = TestState {
            messages: vec!["msg1".to_string()],
            count: 10,
        };
        let update = TestState {
            messages: vec!["msg2".to_string()],
            count: 20,
        };

        updater.apply_update(&mut current, &update);

        assert_eq!(
            current.messages,
            vec!["msg1".to_string(), "msg2".to_string()]
        );
        assert_eq!(current.count, 20);
    }

    /// Test that FieldBasedUpdater can implement additive logic.
    #[test]
    fn test_field_based_updater_add() {
        let updater = FieldBasedUpdater::new(|current: &mut TestState, update: &TestState| {
            current.messages.extend(update.messages.iter().cloned());
            current.count += update.count; // Add instead of replace
        });

        let mut current = TestState {
            messages: vec![],
            count: 10,
        };
        let update = TestState {
            messages: vec!["msg".to_string()],
            count: 5,
        };

        updater.apply_update(&mut current, &update);

        assert_eq!(current.messages, vec!["msg".to_string()]);
        assert_eq!(current.count, 15);
    }

    /// Test that boxed_updater works for type erasure.
    #[test]
    fn test_boxed_updater() {
        let updater: BoxedStateUpdater<TestState> = boxed_updater(ReplaceUpdater);
        let mut current = TestState {
            messages: vec!["old".to_string()],
            count: 10,
        };
        let update = TestState {
            messages: vec!["new".to_string()],
            count: 20,
        };

        updater.apply_update(&mut current, &update);

        assert_eq!(current.messages, vec!["new".to_string()]);
        assert_eq!(current.count, 20);
    }
}
