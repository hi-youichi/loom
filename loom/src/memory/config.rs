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
#[derive(Debug, Clone, Default)]
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
    /// Used when resuming after an approval_required interrupt: load checkpoint state, set state.approval_result, set this to "act".
    pub resume_from_node_id: Option<String>,
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
        };
        let c2 = c.clone();
        assert_eq!(c.thread_id, c2.thread_id);
        assert_eq!(c.checkpoint_id, c2.checkpoint_id);
        assert_eq!(c.checkpoint_ns, c2.checkpoint_ns);
        assert_eq!(c.user_id, c2.user_id);
        assert_eq!(c.resume_from_node_id, c2.resume_from_node_id);
    }
}
