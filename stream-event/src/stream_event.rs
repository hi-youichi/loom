use crate::{CheckpointEvent, StreamMetadata};
use loom_llm::traits::MessageChunk;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckpointEvent;
    use serde_json::json;

    #[derive(Clone, Debug)]
    struct TestState(String);

    #[test]
    fn values_variant() {
        let ev = StreamEvent::<TestState>::Values(TestState("snap".to_string()));
        if let StreamEvent::Values(s) = ev {
            assert_eq!(s.0, "snap");
        } else {
            panic!("expected Values");
        }
    }

    #[test]
    fn updates_variant() {
        let ev = StreamEvent::<TestState>::Updates {
            node_id: "think".to_string(),
            state: TestState("updated".to_string()),
            namespace: Some("ns".to_string()),
        };
        if let StreamEvent::Updates { node_id, state, namespace } = ev {
            assert_eq!(node_id, "think");
            assert_eq!(state.0, "updated");
            assert_eq!(namespace.as_deref(), Some("ns"));
        } else {
            panic!("expected Updates");
        }
    }

    #[test]
    fn custom_variant() {
        let ev = StreamEvent::<TestState>::Custom(json!({"key": "val"}));
        if let StreamEvent::Custom(v) = ev {
            assert_eq!(v["key"], "val");
        } else {
            panic!("expected Custom");
        }
    }

    #[test]
    fn checkpoint_variant() {
        let cp = CheckpointEvent {
            checkpoint_id: "cp-1".to_string(),
            timestamp: "2025-01-01".to_string(),
            step: 0,
            state: TestState("s".to_string()),
            thread_id: None,
            checkpoint_ns: None,
        };
        let ev = StreamEvent::Checkpoint(cp);
        if let StreamEvent::Checkpoint(c) = ev {
            assert_eq!(c.checkpoint_id, "cp-1");
        } else {
            panic!("expected Checkpoint");
        }
    }

    #[test]
    fn task_start_variant() {
        let ev = StreamEvent::<TestState>::TaskStart {
            node_id: "act".to_string(),
            namespace: None,
        };
        if let StreamEvent::TaskStart { node_id, .. } = ev {
            assert_eq!(node_id, "act");
        } else {
            panic!("expected TaskStart");
        }
    }

    #[test]
    fn task_end_variant() {
        let ev = StreamEvent::<TestState>::TaskEnd {
            node_id: "act".to_string(),
            result: Ok(()),
            namespace: None,
        };
        if let StreamEvent::TaskEnd { result, .. } = ev {
            assert!(result.is_ok());
        } else {
            panic!("expected TaskEnd");
        }
    }

    #[test]
    fn tot_expand_variant() {
        let ev = StreamEvent::<TestState>::TotExpand {
            candidates: vec!["a".to_string(), "b".to_string()],
        };
        if let StreamEvent::TotExpand { candidates } = ev {
            assert_eq!(candidates.len(), 2);
        } else {
            panic!("expected TotExpand");
        }
    }

    #[test]
    fn tot_evaluate_variant() {
        let ev = StreamEvent::<TestState>::TotEvaluate {
            chosen: 1,
            scores: vec![0.3, 0.9],
        };
        if let StreamEvent::TotEvaluate { chosen, scores } = ev {
            assert_eq!(chosen, 1);
            assert_eq!(scores.len(), 2);
        } else {
            panic!("expected TotEvaluate");
        }
    }

    #[test]
    fn usage_variant() {
        let ev = StreamEvent::<TestState>::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            prefill_duration: Some(std::time::Duration::from_millis(100)),
            decode_duration: None,
        };
        if let StreamEvent::Usage { total_tokens, .. } = ev {
            assert_eq!(total_tokens, 30);
        } else {
            panic!("expected Usage");
        }
    }

    #[test]
    fn tool_call_variant() {
        let ev = StreamEvent::<TestState>::ToolCall {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
            arguments: json!({"cmd": "ls"}),
        };
        if let StreamEvent::ToolCall { name, .. } = ev {
            assert_eq!(name, "bash");
        } else {
            panic!("expected ToolCall");
        }
    }

    #[test]
    fn debug_format() {
        let ev = StreamEvent::<TestState>::Values(TestState("x".to_string()));
        let s = format!("{:?}", ev);
        assert!(s.contains("Values"));
    }
}
