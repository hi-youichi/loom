//! Invoke config: thread_id, checkpoint_id, checkpoint_ns, user_id.
//!
//! config["configurable"]. Used by CompiledStateGraph::invoke
//! and Checkpointer.

/// Config for a single invoke. Identifies the thread and optional checkpoint.
///
/// config["configurable"] (thread_id, checkpoint_id, checkpoint_ns).
/// When using a checkpointer, invoke must provide at least thread_id.
///
/// **Interaction**: Passed to `CompiledStateGraph::invoke(state, config)` and
/// `Checkpointer::put` / `get_tuple` / `list`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RunnableConfig {
    /// Unique id for this conversation/thread. Required when using a checkpointer.
    pub thread_id: Option<String>,
    /// If set, load state from this checkpoint instead of the latest (time travel / branch).
    pub checkpoint_id: Option<String>,
    /// Optional namespace for checkpoints (e.g. subgraph). Default is empty.
    pub checkpoint_ns: String,
    /// Optional user id; used by Store for cross-thread memory (namespace).
    pub user_id: Option<String>,
    /// When set, the graph starts from this node instead of the first (e.g. resume after Interrupt at "act").
    pub resume_from_node_id: Option<String>,
    /// Current sub-agent nesting depth. Used by `AgentTool` to prevent
    /// infinite recursion. `None` or `Some(0)` means top-level.
    pub depth: Option<u32>,
    /// ACP session_id from the IDE client.
    ///
    /// When running under ACP (Zed, JetBrains), this carries the real `sessionId`
    /// assigned by the IDE. Propagated through `RunContext` → `ToolCallContext`
    /// so that `AcpBridgeCommandExecutor` and ACP terminal tools can route
    /// terminal operations to the correct session.
    ///
    /// `None` in non-ACP contexts (CLI, Telegram, etc.).
    pub acp_session_id: Option<String>,
    /// Default resume value used when resuming a single pending interrupt.
    pub resume_value: Option<serde_json::Value>,
    /// Resume values keyed by checkpoint namespace.
    #[serde(default)]
    pub resume_values_by_namespace: std::collections::HashMap<String, serde_json::Value>,
    /// Resume values keyed by interrupt id.
    #[serde(default)]
    pub resume_values_by_interrupt_id: std::collections::HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: RunnableConfig::default() has all optionals None and checkpoint_ns empty.
    #[test]
    fn runnable_config_default_all_optionals_none_or_empty() {
        let c = RunnableConfig::default();
        assert!(c.thread_id.is_none());
        assert!(c.checkpoint_id.is_none());
        assert!(c.checkpoint_ns.is_empty());
        assert!(c.user_id.is_none());
        assert!(c.resume_value.is_none());
    }

    /// **Scenario**: After setting fields and cloning, cloned values match.
    #[test]
    fn runnable_config_clone() {
        let c = RunnableConfig {
            thread_id: Some("t1".into()),
            checkpoint_id: Some("cp1".into()),
            checkpoint_ns: "ns".into(),
            user_id: Some("u1".into()),
            resume_from_node_id: None,
            depth: None,
            acp_session_id: None,
            resume_value: None,
            resume_values_by_namespace: Default::default(),
            resume_values_by_interrupt_id: Default::default(),
        };
        let c2 = c.clone();
        assert_eq!(c.thread_id, c2.thread_id);
        assert_eq!(c.checkpoint_id, c2.checkpoint_id);
        assert_eq!(c.checkpoint_ns, c2.checkpoint_ns);
        assert_eq!(c.user_id, c2.user_id);
        assert_eq!(c.resume_from_node_id, c2.resume_from_node_id);
        assert_eq!(c.resume_value, c2.resume_value);
    }
}
