//! Managed values for graph execution.
//!
//! Managed values provide runtime information that is computed or managed by the graph
//! execution system, rather than being part of the state itself. Examples include
//! `IsLastStep` which indicates whether the current step is the last one.

use std::fmt::Debug;

use crate::graph::RunContext;

/// Managed value trait for runtime-computed values.
///
/// Managed values provide information that is computed during graph execution,
/// such as whether the current step is the last one, or other runtime metadata.
pub trait ManagedValue<T, S>: Send + Sync
where
    T: Clone + Send + Sync + Debug + 'static,
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Get the managed value for the current context.
    ///
    /// The value is computed based on the current execution context.
    fn get(&self, context: &RunContext<S>) -> T;
}

/// IsLastStep managed value: indicates whether the current step is the last one.
///
/// This managed value can be used by nodes to determine if they are executing
/// in the final step of the graph, which can be useful for cleanup or finalization logic.
#[derive(Debug, Clone)]
pub struct IsLastStep {
    is_last: bool,
}

impl IsLastStep {
    /// Creates a new IsLastStep managed value.
    pub fn new(is_last: bool) -> Self {
        Self { is_last }
    }

    /// Returns true if this is the last step.
    pub fn value(&self) -> bool {
        self.is_last
    }
}

impl<S> ManagedValue<bool, S> for IsLastStep
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn get(&self, _context: &RunContext<S>) -> bool {
        self.is_last
    }
}

// Also implement for serde_json::Value for use in RunContext
impl<S> ManagedValue<serde_json::Value, S> for IsLastStep
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn get(&self, _context: &RunContext<S>) -> serde_json::Value {
        serde_json::Value::Bool(self.is_last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::RunContext;
    use crate::memory::RunnableConfig;

    fn create_test_context<S>() -> RunContext<S>
    where
        S: Clone + Send + Sync + Debug + 'static,
    {
        RunContext::new(RunnableConfig::default())
    }

    #[test]
    fn test_is_last_step_true() {
        let is_last: IsLastStep = IsLastStep::new(true);
        let context = create_test_context::<String>();
        // Test bool implementation
        let value: bool = <IsLastStep as ManagedValue<bool, String>>::get(&is_last, &context);
        assert_eq!(value, true);
        assert_eq!(is_last.value(), true);
    }

    #[test]
    fn test_is_last_step_false() {
        let is_last: IsLastStep = IsLastStep::new(false);
        let context = create_test_context::<String>();
        // Test bool implementation
        let value: bool = <IsLastStep as ManagedValue<bool, String>>::get(&is_last, &context);
        assert_eq!(value, false);
        assert_eq!(is_last.value(), false);
    }
}
