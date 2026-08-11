//! Interrupt mechanism for graph execution.
//!
//! Provides support for interrupting graph execution, useful for human-in-the-loop
//! scenarios where execution needs to pause for user input or approval.

pub use crate::error::Interrupt;

use crate::error::GraphError;

/// Trait for handling interrupts during graph execution.
///
/// Implement this trait to define custom interrupt handling logic.
pub trait InterruptHandler: Send + Sync {
    /// Handle an interrupt and return a value to continue execution.
    ///
    /// This method is called when an interrupt is raised. The handler can
    /// perform actions like prompting the user, logging, or modifying state,
    /// then return a value that will be used to continue execution.
    fn handle_interrupt(&self, interrupt: &Interrupt) -> Result<serde_json::Value, GraphError>;
}

/// Default interrupt handler that returns the interrupt value as-is.
#[derive(Debug, Clone)]
pub struct DefaultInterruptHandler;

impl InterruptHandler for DefaultInterruptHandler {
    fn handle_interrupt(&self, interrupt: &Interrupt) -> Result<serde_json::Value, GraphError> {
        Ok(interrupt.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_new() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.value, serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.id, None);
    }

    #[test]
    fn test_interrupt_with_id() {
        let interrupt = Interrupt::with_id(
            serde_json::json!({"action": "approve"}),
            "interrupt_1".to_string(),
        );
        assert_eq!(interrupt.value, serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.id, Some("interrupt_1".to_string()));
    }

    #[test]
    fn test_default_interrupt_handler() {
        let handler = DefaultInterruptHandler;
        let interrupt = Interrupt::new(serde_json::json!({"result": "success"}));

        let result = handler.handle_interrupt(&interrupt).unwrap();
        assert_eq!(result, serde_json::json!({"result": "success"}));
    }

    #[test]
    fn test_custom_interrupt_handler() {
        struct CustomHandler;

        impl InterruptHandler for CustomHandler {
            fn handle_interrupt(
                &self,
                interrupt: &Interrupt,
            ) -> Result<serde_json::Value, GraphError> {
                let mut value = interrupt.value.clone();
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("handled".to_string(), serde_json::json!(true));
                }
                Ok(value)
            }
        }

        let handler = CustomHandler;
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));

        let result = handler.handle_interrupt(&interrupt).unwrap();
        assert_eq!(
            result,
            serde_json::json!({"action": "approve", "handled": true})
        );
    }
}
