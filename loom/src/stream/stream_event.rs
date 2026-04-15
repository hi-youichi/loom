use super::super::{CheckpointEvent, MessageChunk, StreamMetadata};
use serde_json::Value;
use std::fmt::Debug;

/// Streamed event emitted while running a graph.
#[derive(Clone, Debug)]
pub enum StreamEvent<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Full state snapshot after a node finishes.
    Values(S),
    /// Incremental update with the node id and state after that node.
    Updates {
        node_id: String,
        state: S,
        namespace: Option<String>,
    },
    /// Message chunk emitted by a node (e.g. ThinkNode streaming LLM output).
    Messages {
        chunk: MessageChunk,
        metadata: StreamMetadata,
    },
    /// Custom JSON payload for arbitrary streaming data.
    Custom(Value),
    /// Checkpoint event emitted when a checkpoint is created.
    Checkpoint(CheckpointEvent<S>),
    /// Task start event emitted when a node begins execution.
    TaskStart {
        /// Node ID that is starting execution.
        node_id: String,
        /// Optional namespace for subgraph events.
        namespace: Option<String>,
    },
    /// Task end event emitted when a node finishes execution.
    TaskEnd {
        /// Node ID that finished execution.
        node_id: String,
        /// Result of the task: Ok(()) for success, Err(message) for failure.
        result: Result<(), String>,
        /// Optional namespace for subgraph events.
        namespace: Option<String>,
    },
    /// ToT (Tree of Thoughts): expand node produced multiple candidates.
    TotExpand {
        /// Short summaries of each candidate thought for display.
        candidates: Vec<String>,
    },
    /// ToT: evaluate node chose one candidate and assigned scores.
    TotEvaluate {
        /// Index of the chosen candidate.
        chosen: usize,
        /// Score per candidate (same order as candidates).
        scores: Vec<f32>,
    },
    /// ToT: backtrack node is returning to a previous depth.
    TotBacktrack {
        /// Human-readable reason for backtracking.
        reason: String,
        /// Depth we are backtracking to.
        to_depth: u32,
    },
    /// GoT (Graph of Thoughts): plan_graph node produced a DAG.
    GotPlan {
        /// Number of nodes in the task graph.
        node_count: usize,
        /// Number of edges (dependencies).
        edge_count: usize,
        /// Optional summary of node ids for display.
        node_ids: Vec<String>,
    },
    /// GoT: execute_graph started executing a task node.
    GotNodeStart {
        /// Task node id.
        node_id: String,
    },
    /// GoT: execute_graph completed a task node.
    GotNodeComplete {
        /// Task node id.
        node_id: String,
        /// Short summary of result (e.g. first 200 chars).
        result_summary: String,
    },
    /// GoT: execute_graph marked a task node as failed.
    GotNodeFailed {
        /// Task node id.
        node_id: String,
        /// Error message.
        error: String,
    },
    /// AGoT: a node was expanded into a subgraph (dynamic DAG extension).
    GotExpand {
        /// Node id that triggered the expansion.
        node_id: String,
        /// Number of new nodes added.
        nodes_added: usize,
        /// Number of new edges added.
        edges_added: usize,
    },
    /// LLM token usage for the last completion (e.g. after think node).
    /// Emitted when the provider returns usage (e.g. OpenAI); consumers can print when verbose.
    Usage {
        /// Tokens in the prompt (input).
        prompt_tokens: u32,
        /// Tokens in the completion (output).
        completion_tokens: u32,
        /// Total tokens (prompt + completion).
        total_tokens: u32,
        /// Time from LLM call start to first token received (prefill phase).
        /// `None` in non-streaming mode where the two phases cannot be separated.
        prefill_duration: Option<std::time::Duration>,
        /// Time from first token received to generation complete (decode phase).
        /// `None` in non-streaming mode.
        decode_duration: Option<std::time::Duration>,
    },
    /// LLM streaming tool call argument delta (Think node, per chunk).
    ToolCallChunk {
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    /// LLM decided to call a tool (Think node, complete arguments).
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },
    /// Tool execution started (Act node, before calling tool).
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    /// Tool incremental output during execution (Act node).
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    /// Tool execution finished (Act node, after tool returns).
    ToolEnd {
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
        /// Full un-normalized result. When set, ACP layer uses this for `raw_output`
        /// instead of `result` (which may be a head-tail excerpt or file reference).
        raw_result: Option<String>,
    },
    /// Tool requires user approval before execution (Act node).
    ToolApproval {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },
}
