//! Memory module — re-exported from `loom-memory` crate.
//!
//! All memory implementations (checkpointers, stores, embedders, serializers)
//! are now in the `loom-memory` crate. This module re-exports everything
//! for backward compatibility.

pub use loom_memory::*;
