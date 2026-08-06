//! Application state machine.
//!
//! Manages the global application state transitions for the Loom TUI.
//! States represent the lifecycle of a user interaction session:
//! idle → input → submit → process → (approval loop) → idle → ...

use std::fmt;

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Application global state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppState {
    /// Idle, waiting for user input.
    Idle,
    /// User is typing input.
    Inputting,
    /// Submitting input to the agent.
    Submitting,
    /// AI is thinking / executing.
    Processing,
    /// Waiting for user approval.
    AwaitingApproval,
    /// Interrupted (e.g., Ctrl+C).
    Interrupted,
    /// Error state.
    Error,
    /// Exiting the application.
    Exiting,
}

// ---------------------------------------------------------------------------
// AppEvent
// ---------------------------------------------------------------------------

/// Application events that trigger state transitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppEvent {
    /// User starts typing.
    StartInput,
    /// User submits input.
    Submit,
    /// AI begins processing.
    Processing,
    /// AI completed processing.
    Completed,
    /// AI requests user approval.
    RequestApproval,
    /// User completed the approval action.
    ApprovalDone,
    /// Interrupt signal (e.g., Ctrl+C).
    Interrupt,
    /// Resume from interrupted or error state.
    Resume,
    /// An error occurred.
    Error,
    /// Exit the application.
    Exit,
}

// ---------------------------------------------------------------------------
// StateError
// ---------------------------------------------------------------------------

/// State transition error.
#[derive(Debug, Clone)]
pub struct StateError(pub String);

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StateError {}

// ---------------------------------------------------------------------------
// Transition table
// ---------------------------------------------------------------------------

impl AppState {
    /// Attempt a state transition given an event.
    ///
    /// Returns the new state on success, or a [`StateError`] describing the
    /// invalid transition.
    ///
    /// # Transition table
    ///
    /// | Current State        | Event            | Next State         |
    /// |----------------------|------------------|--------------------|
    /// | Idle                 | StartInput       | Inputting          |
    /// | Inputting            | Submit           | Submitting         |
    /// | Submitting           | Processing       | Processing         |
    /// | Processing           | Completed        | Idle               |
    /// | Processing           | RequestApproval  | AwaitingApproval   |
    /// | AwaitingApproval     | ApprovalDone     | Processing         |
    /// | *                    | Interrupt        | Interrupted        |
    /// | Interrupted          | Resume           | Idle               |
    /// | *                    | Error            | Error              |
    /// | Error                | Resume           | Idle               |
    /// | *                    | Exit             | Exiting            |
    pub fn transition(&self, event: AppEvent) -> Result<AppState, StateError> {
        match (self, event) {
            // Idle → Inputting (user starts typing)
            (AppState::Idle, AppEvent::StartInput) => Ok(AppState::Inputting),

            // Idle → Submitting (user submits directly — simplifies flow)
            (AppState::Idle, AppEvent::Submit) => Ok(AppState::Submitting),

            // Inputting → Submitting (user submits)
            (AppState::Inputting, AppEvent::Submit) => Ok(AppState::Submitting),

            // Submitting → Processing (AI starts processing)
            (AppState::Submitting, AppEvent::Processing) => Ok(AppState::Processing),

            // Processing → Idle (AI completed)
            (AppState::Processing, AppEvent::Completed) => Ok(AppState::Idle),

            // Processing → AwaitingApproval (AI requests approval)
            (AppState::Processing, AppEvent::RequestApproval) => Ok(AppState::AwaitingApproval),

            // AwaitingApproval → Processing (user approved)
            (AppState::AwaitingApproval, AppEvent::ApprovalDone) => Ok(AppState::Processing),

            // Any state → Interrupted (Ctrl+C)
            (_, AppEvent::Interrupt) => Ok(AppState::Interrupted),

            // Interrupted → Idle (resume)
            (AppState::Interrupted, AppEvent::Resume) => Ok(AppState::Idle),

            // Any state → Error
            (_, AppEvent::Error) => Ok(AppState::Error),

            // Error → Idle (resume)
            (AppState::Error, AppEvent::Resume) => Ok(AppState::Idle),

            // Any state → Exiting
            (_, AppEvent::Exit) => Ok(AppState::Exiting),

            // Disallowed transitions
            _ => Err(StateError(format!(
                "cannot transition from {:?} to {:?}",
                self, event
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // Enum derives: Debug, Clone, Copy, PartialEq for AppState
    // ===================================================================

    #[test]
    fn test_app_state_debug() {
        // Verify every variant implements Debug and produces non-empty output.
        for state in &[
            AppState::Idle,
            AppState::Inputting,
            AppState::Submitting,
            AppState::Processing,
            AppState::AwaitingApproval,
            AppState::Interrupted,
            AppState::Error,
            AppState::Exiting,
        ] {
            let dbg = format!("{:?}", state);
            assert!(!dbg.is_empty(), "Debug output for {:?} should not be empty", state);
        }
    }

    #[test]
    fn test_app_state_clone_copy_partial_eq() {
        // Clone + Copy
        let a = AppState::Idle;
        let b = a; // Copy (no move)
        assert_eq!(a, b); // PartialEq

        let a = AppState::Processing;
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_app_state_inequality() {
        assert_ne!(AppState::Idle, AppState::Inputting);
        assert_ne!(AppState::Idle, AppState::Exiting);
        assert_ne!(AppState::Processing, AppState::AwaitingApproval);
        assert_ne!(AppState::Error, AppState::Interrupted);
    }

    // ===================================================================
    // Enum derives: Debug, Clone, Copy, PartialEq for AppEvent
    // ===================================================================

    #[test]
    fn test_app_event_debug() {
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Interrupt,
            AppEvent::Resume,
            AppEvent::Error,
            AppEvent::Exit,
        ] {
            let dbg = format!("{:?}", event);
            assert!(!dbg.is_empty(), "Debug output for {:?} should not be empty", event);
        }
    }

    #[test]
    fn test_app_event_clone_copy_partial_eq() {
        let a = AppEvent::Submit;
        let b = a;
        assert_eq!(a, b);

        let a = AppEvent::Interrupt;
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_app_event_inequality() {
        assert_ne!(AppEvent::StartInput, AppEvent::Submit);
        assert_ne!(AppEvent::Completed, AppEvent::RequestApproval);
        assert_ne!(AppEvent::Interrupt, AppEvent::Exit);
    }

    // ===================================================================
    // StateError
    // ===================================================================

    #[test]
    fn test_state_error_display() {
        let err = StateError("something went wrong".into());
        assert_eq!(format!("{}", err), "something went wrong");
    }

    #[test]
    fn test_state_error_empty_string() {
        let err = StateError("".into());
        assert_eq!(format!("{}", err), "");
    }

    #[test]
    fn test_state_error_debug() {
        let err = StateError("test error".into());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("test error"));
    }

    #[test]
    fn test_state_error_impl_error() {
        // Verify StateError implements std::error::Error (trait-object safe)
        let err = StateError("oops".into());
        let err_ref: &dyn std::error::Error = &err;
        assert_eq!(err_ref.to_string(), "oops");
    }

    #[test]
    fn test_state_error_clone() {
        let err = StateError("clone me".into());
        let cloned = err.clone();
        assert_eq!(err.0, cloned.0);
    }

    // ===================================================================
    // Valid transitions — normal flow
    // ===================================================================

    #[test]
    fn test_idle_start_input() {
        assert_eq!(
            AppState::Idle.transition(AppEvent::StartInput).unwrap(),
            AppState::Inputting
        );
    }

    #[test]
    fn test_inputting_submit() {
        assert_eq!(
            AppState::Inputting.transition(AppEvent::Submit).unwrap(),
            AppState::Submitting
        );
    }

    #[test]
    fn test_submitting_processing() {
        assert_eq!(
            AppState::Submitting.transition(AppEvent::Processing).unwrap(),
            AppState::Processing
        );
    }

    #[test]
    fn test_processing_completed() {
        assert_eq!(
            AppState::Processing.transition(AppEvent::Completed).unwrap(),
            AppState::Idle
        );
    }

    #[test]
    fn test_processing_request_approval() {
        assert_eq!(
            AppState::Processing.transition(AppEvent::RequestApproval).unwrap(),
            AppState::AwaitingApproval
        );
    }

    #[test]
    fn test_awaiting_approval_approval_done() {
        assert_eq!(
            AppState::AwaitingApproval.transition(AppEvent::ApprovalDone).unwrap(),
            AppState::Processing
        );
    }

    #[test]
    fn test_interrupted_resume() {
        assert_eq!(
            AppState::Interrupted.transition(AppEvent::Resume).unwrap(),
            AppState::Idle
        );
    }

    #[test]
    fn test_error_resume() {
        assert_eq!(
            AppState::Error.transition(AppEvent::Resume).unwrap(),
            AppState::Idle
        );
    }

    // ===================================================================
    // Wildcard transitions — Interrupt from any state
    // ===================================================================

    #[test]
    fn test_interrupt_from_all_states() {
        for state in &[
            AppState::Idle,
            AppState::Inputting,
            AppState::Submitting,
            AppState::Processing,
            AppState::AwaitingApproval,
            AppState::Interrupted,
            AppState::Error,
            AppState::Exiting,
        ] {
            assert_eq!(
                state.transition(AppEvent::Interrupt).unwrap(),
                AppState::Interrupted,
                "Interrupt should work from {:?}",
                state
            );
        }
    }

    // ===================================================================
    // Wildcard transitions — Error from any state
    // ===================================================================

    #[test]
    fn test_error_from_all_states() {
        for state in &[
            AppState::Idle,
            AppState::Inputting,
            AppState::Submitting,
            AppState::Processing,
            AppState::AwaitingApproval,
            AppState::Interrupted,
            AppState::Error,
            AppState::Exiting,
        ] {
            assert_eq!(
                state.transition(AppEvent::Error).unwrap(),
                AppState::Error,
                "Error event should work from {:?}",
                state
            );
        }
    }

    // ===================================================================
    // Wildcard transitions — Exit from any state
    // ===================================================================

    #[test]
    fn test_exit_from_all_states() {
        for state in &[
            AppState::Idle,
            AppState::Inputting,
            AppState::Submitting,
            AppState::Processing,
            AppState::AwaitingApproval,
            AppState::Interrupted,
            AppState::Error,
            AppState::Exiting,
        ] {
            assert_eq!(
                state.transition(AppEvent::Exit).unwrap(),
                AppState::Exiting,
                "Exit should work from {:?}",
                state
            );
        }
    }

    // ===================================================================
    // Invalid transitions — each state with events that should fail
    // ===================================================================

    #[test]
    fn test_idle_invalid_transitions() {
        let state = AppState::Idle;
        // StartInput is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Idle -> {:?} should be invalid", event);
            let err = result.unwrap_err();
            assert!(
                err.0.contains("Idle"),
                "Error message should mention the source state, got: {}",
                err.0
            );
        }
    }

    #[test]
    fn test_inputting_invalid_transitions() {
        let state = AppState::Inputting;
        // Submit is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Inputting -> {:?} should be invalid", event);
        }
    }

    #[test]
    fn test_submitting_invalid_transitions() {
        let state = AppState::Submitting;
        // Processing is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Submitting -> {:?} should be invalid", event);
        }
    }

    #[test]
    fn test_processing_invalid_transitions() {
        let state = AppState::Processing;
        // Completed, RequestApproval are valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::ApprovalDone,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Processing -> {:?} should be invalid", event);
        }
    }

    #[test]
    fn test_awaiting_approval_invalid_transitions() {
        let state = AppState::AwaitingApproval;
        // ApprovalDone is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "AwaitingApproval -> {:?} should be invalid", event);
        }
    }

    #[test]
    fn test_interrupted_invalid_transitions() {
        let state = AppState::Interrupted;
        // Resume is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Resume, // Already tested above — this is valid
        ] {
            let result = state.transition(*event);
            if *event == AppEvent::Resume {
                assert!(result.is_ok(), "Interrupted -> Resume should be valid");
            } else {
                assert!(result.is_err(), "Interrupted -> {:?} should be invalid", event);
            }
        }
    }

    #[test]
    fn test_error_invalid_transitions() {
        let state = AppState::Error;
        // Resume is valid, everything else (except wildcards) should fail
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Error -> {:?} should be invalid", event);
        }
    }

    #[test]
    fn test_exiting_invalid_transitions() {
        let state = AppState::Exiting;
        // Only wildcards (Interrupt, Error, Exit) should be valid from Exiting
        for event in &[
            AppEvent::StartInput,
            AppEvent::Submit,
            AppEvent::Processing,
            AppEvent::Completed,
            AppEvent::RequestApproval,
            AppEvent::ApprovalDone,
            AppEvent::Resume,
        ] {
            let result = state.transition(*event);
            assert!(result.is_err(), "Exiting -> {:?} should be invalid", event);
        }
    }

    // ===================================================================
    // Error message format
    // ===================================================================

    #[test]
    fn test_error_message_format() {
        // Idle → Completed is invalid (Submit is now valid from Idle)
        let result = AppState::Idle.transition(AppEvent::Completed);
        let err = result.unwrap_err();
        assert!(
            err.0.contains("Idle") && err.0.contains("Completed"),
            "Error message should contain both source state and event, got: {}",
            err.0
        );

        let result = AppState::Processing.transition(AppEvent::StartInput);
        let err = result.unwrap_err();
        assert!(
            err.0.contains("Processing") && err.0.contains("StartInput"),
            "Error message should contain both source state and event, got: {}",
            err.0
        );
    }

    // ===================================================================
    // Full lifecycle simulation
    // ===================================================================

    #[test]
    fn test_full_lifecycle() {
        // Simulate a complete user session: idle -> input -> submit -> process -> idle
        let state = AppState::Idle;
        assert_eq!(state.transition(AppEvent::StartInput).unwrap(), AppState::Inputting);
        assert_eq!(AppState::Inputting.transition(AppEvent::Submit).unwrap(), AppState::Submitting);
        assert_eq!(AppState::Submitting.transition(AppEvent::Processing).unwrap(), AppState::Processing);
        assert_eq!(AppState::Processing.transition(AppEvent::Completed).unwrap(), AppState::Idle);
    }

    #[test]
    fn test_approval_lifecycle() {
        // idle -> input -> submit -> process -> approval -> process -> idle
        let state = AppState::Idle
            .transition(AppEvent::StartInput).unwrap();
        assert_eq!(state, AppState::Inputting);

        let state = state.transition(AppEvent::Submit).unwrap();
        assert_eq!(state, AppState::Submitting);

        let state = state.transition(AppEvent::Processing).unwrap();
        assert_eq!(state, AppState::Processing);

        let state = state.transition(AppEvent::RequestApproval).unwrap();
        assert_eq!(state, AppState::AwaitingApproval);

        let state = state.transition(AppEvent::ApprovalDone).unwrap();
        assert_eq!(state, AppState::Processing);

        let state = state.transition(AppEvent::Completed).unwrap();
        assert_eq!(state, AppState::Idle);
    }

    #[test]
    fn test_interrupt_resume_lifecycle() {
        // idle -> interrupt -> resume
        let state = AppState::Idle
            .transition(AppEvent::Interrupt).unwrap();
        assert_eq!(state, AppState::Interrupted);

        let state = state.transition(AppEvent::Resume).unwrap();
        assert_eq!(state, AppState::Idle);
    }

    #[test]
    fn test_error_recovery_lifecycle() {
        // processing -> error -> resume
        let state = AppState::Processing
            .transition(AppEvent::Error).unwrap();
        assert_eq!(state, AppState::Error);

        let state = state.transition(AppEvent::Resume).unwrap();
        assert_eq!(state, AppState::Idle);
    }

    #[test]
    fn test_exit_from_anywhere() {
        // Exit should work from any state during any phase
        for state in &[
            AppState::Idle,
            AppState::Inputting,
            AppState::Submitting,
            AppState::Processing,
            AppState::AwaitingApproval,
            AppState::Interrupted,
            AppState::Error,
            AppState::Exiting,
        ] {
            assert_eq!(
                state.transition(AppEvent::Exit).unwrap(),
                AppState::Exiting,
                "Exit should work from {:?}",
                state
            );
        }
    }

    // ===================================================================
    // Edge cases: chaining many transitions
    // ===================================================================

    #[test]
    fn test_rapid_interrupt_cycle() {
        // Interrupt from any state repeatedly
        let mut state = AppState::Processing;
        for _ in 0..10 {
            state = state.transition(AppEvent::Interrupt).unwrap();
            assert_eq!(state, AppState::Interrupted);
            state = state.transition(AppEvent::Resume).unwrap();
            assert_eq!(state, AppState::Idle);
        }
    }

    #[test]
    fn test_rapid_error_cycle() {
        let mut state = AppState::Processing;
        for _ in 0..10 {
            state = state.transition(AppEvent::Error).unwrap();
            assert_eq!(state, AppState::Error);
            state = state.transition(AppEvent::Resume).unwrap();
            assert_eq!(state, AppState::Idle);
        }
    }

    // ===================================================================
    // Round-trip: each valid transition returns the exact expected state
    // ===================================================================

    #[test]
    fn test_all_valid_transitions_round_trip() {
        // Verify every explicit valid transition (not wildcard covered above)
        let cases: Vec<(AppState, AppEvent, AppState)> = vec![
            (AppState::Idle, AppEvent::StartInput, AppState::Inputting),
            (AppState::Inputting, AppEvent::Submit, AppState::Submitting),
            (AppState::Submitting, AppEvent::Processing, AppState::Processing),
            (AppState::Processing, AppEvent::Completed, AppState::Idle),
            (AppState::Processing, AppEvent::RequestApproval, AppState::AwaitingApproval),
            (AppState::AwaitingApproval, AppEvent::ApprovalDone, AppState::Processing),
            (AppState::Interrupted, AppEvent::Resume, AppState::Idle),
            (AppState::Error, AppEvent::Resume, AppState::Idle),
        ];

        for (state, event, expected) in &cases {
            let result = state.transition(*event).unwrap();
            assert_eq!(
                result,
                *expected,
                "{:?} -> {:?} should yield {:?}, got {:?}",
                state, event, expected, result
            );
        }
    }
}