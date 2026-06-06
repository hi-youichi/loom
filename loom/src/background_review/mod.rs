//! Background review system for Loom (re-export shell)
//!
//! This module re-exports all types from the `loom-background-review` crate.
//! The actual implementation has been extracted to the independent crate.

// Re-export everything from loom-background-review
pub use loom_background_review::*;

// Note: If you need to add loom-specific extensions or adapters,
// you can add them here as additional modules.
