//! Context compression for LLM interactions
//!
//! This module provides functionality for compressing conversation history
//! and managing context windows efficiently.

pub mod compact_node;
pub mod compaction;
pub mod config;
pub mod context_window;
pub mod graph;
pub mod prune_node;

pub use config::CompactionConfig;
pub use graph::{build_graph, CompressionGraphNode};
