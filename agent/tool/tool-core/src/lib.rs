//! Tool Core: Tool trait, ToolRegistry, and core types for Loom agents.

mod context;
mod mock;
mod registry;
mod tool;
mod yaml_specs;

pub use context::ToolCallContext;
pub use mock::{mock_registry, MockTool};
pub use registry::{ArcTool, ToolRegistry, ToolRegistryLocked};
pub use tool::Tool;
pub use yaml_specs::{load_tool_specs, YamlSpecError};

pub use loom_llm::tool::{ToolOutputHint, ToolOutputStrategy, ToolSourceError, ToolSpec};
pub use loom_llm::message::ToolCallContent;
