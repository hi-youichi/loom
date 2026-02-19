//! Context compression: prune tool results and compact conversation history.
//!
//! Used by the ReAct graph to stay within context limits via pruning and LLM summarization.

pub mod compact_node;
pub mod compaction;
pub mod config;
pub mod context_window;
pub mod graph;
pub mod prune_node;

pub use config::CompactionConfig;
pub use graph::{build_graph, CompressionGraphNode};
