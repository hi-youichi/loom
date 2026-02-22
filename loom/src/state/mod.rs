//! State types for graph-based agents.
//!
//! This module provides the state and tool types used by the minimal ReAct agent
//! (Think → Act → Observe). The graph state flows through [`StateGraph`](crate::graph::StateGraph)
//! and is read/written by [`ThinkNode`](crate::ThinkNode), [`ActNode`](crate::ActNode), and
//! [`ObserveNode`](crate::ObserveNode).
//!
//!
//! # Main types
//!
//! - [`ReActState`]: Conversation messages plus per-round `tool_calls` and `tool_results`;
//!   use [`ReActState::last_assistant_reply`] for the final assistant message.
//! - [`ToolCall`]: A single tool invocation from the LLM; consumed by Act to call
//!   [`ToolSource::call_tool`](crate::tool_source::ToolSource::call_tool).
//! - [`ToolResult`]: Result of one tool execution; written by Act, merged in Observe.
//!
//! # Example
//!
//! ```rust
//! use loom::{ReActState, Message};
//!
//! let mut state = ReActState::default();
//! state.messages.push(Message::system("You are a helpful assistant."));
//! state.messages.push(Message::user("What is 2+2?"));
//! // ... pass state to run_agent or StateGraph::invoke
//! ```

pub mod react_state;

pub use react_state::{ReActState, ToolCall, ToolResult};
