//! Static validation for Pregel graph definitions.
//!
//! Validation is intentionally structural: it checks graph topology, reserved
//! names, channel references, and interrupt configuration before a run starts.
//! It does not try to prove that node logic is correct.

use std::collections::HashSet;

use crate::channel::ChannelKind;
use crate::config::PregelConfig;
use crate::node::{PregelGraph, PregelNode};
use crate::types::{ReservedWrite, TASKS_CHANNEL};
use anureo_graph_core::GraphError;

impl PregelGraph {
    /// Validates graph topology, channel references, and configured interrupts.
    ///
    /// This is the same validation used by [`crate::PregelRuntime`]
    /// before executing or exporting a graph.
    pub fn validate_with_config(&self, config: &PregelConfig) -> Result<(), GraphError> {
        for (name, spec) in &self.channels {
            validate_channel_name(name, &spec.kind)?;
        }

        let known_channels = self.channels.keys().cloned().collect::<HashSet<_>>();
        let mut subscribed_channels = HashSet::new();

        for (name, node) in &self.nodes {
            validate_node_name(name)?;
            validate_node_channels(
                name,
                node.as_ref(),
                &known_channels,
                &mut subscribed_channels,
            )?;
        }

        validate_input_channels(&self.input_channels, &known_channels, &subscribed_channels)?;
        validate_output_channels(&self.output_channels, &known_channels)?;
        validate_interrupt_nodes(&config.interrupt_before, "interrupt_before", &self.nodes)?;
        validate_interrupt_nodes(&config.interrupt_after, "interrupt_after", &self.nodes)?;
        Ok(())
    }
}

fn validate_channel_name(name: &str, kind: &ChannelKind) -> Result<(), GraphError> {
    if name == TASKS_CHANNEL {
        if matches!(kind, ChannelKind::Tasks) {
            return Ok(());
        }
        return Err(GraphError::ExecutionFailed(format!(
            "reserved channel {TASKS_CHANNEL} must use ChannelKind::Tasks"
        )));
    }

    if matches_reserved_name(name) {
        return Err(GraphError::ExecutionFailed(format!(
            "channel name {name} is reserved"
        )));
    }

    if matches!(kind, ChannelKind::Tasks) {
        return Err(GraphError::ExecutionFailed(format!(
            "ChannelKind::Tasks must use reserved channel name {TASKS_CHANNEL}"
        )));
    }

    Ok(())
}

fn validate_node_name(name: &str) -> Result<(), GraphError> {
    if matches_reserved_name(name) {
        return Err(GraphError::ExecutionFailed(format!(
            "node name {name} is reserved"
        )));
    }
    Ok(())
}

fn validate_node_channels(
    node_name: &str,
    node: &dyn PregelNode,
    known_channels: &HashSet<String>,
    subscribed_channels: &mut HashSet<String>,
) -> Result<(), GraphError> {
    let reads_push_payload = node
        .triggers()
        .iter()
        .any(|trigger| trigger == TASKS_CHANNEL);

    for trigger in node.triggers() {
        if !known_channels.contains(trigger) {
            return Err(GraphError::ExecutionFailed(format!(
                "node {node_name} subscribes to unknown channel {trigger}"
            )));
        }
        subscribed_channels.insert(trigger.clone());
    }

    for read in node.reads() {
        if reads_push_payload && !known_channels.contains(read) {
            continue;
        }
        if !known_channels.contains(read) {
            return Err(GraphError::ExecutionFailed(format!(
                "node {node_name} reads unknown channel {read}"
            )));
        }
    }

    Ok(())
}

fn validate_input_channels(
    input_channels: &[String],
    known_channels: &HashSet<String>,
    _subscribed_channels: &HashSet<String>,
) -> Result<(), GraphError> {
    for channel in input_channels {
        if !known_channels.contains(channel) {
            return Err(GraphError::ExecutionFailed(format!(
                "input channel {channel} is not defined"
            )));
        }
    }
    Ok(())
}

fn validate_output_channels(
    output_channels: &[String],
    known_channels: &HashSet<String>,
) -> Result<(), GraphError> {
    for channel in output_channels {
        if !known_channels.contains(channel) {
            return Err(GraphError::ExecutionFailed(format!(
                "output channel {channel} is not defined"
            )));
        }
    }
    Ok(())
}

fn validate_interrupt_nodes(
    nodes: &[String],
    label: &str,
    known_nodes: &std::collections::HashMap<String, std::sync::Arc<dyn PregelNode>>,
) -> Result<(), GraphError> {
    for node in nodes {
        if !known_nodes.contains_key(node) {
            return Err(GraphError::ExecutionFailed(format!(
                "{label} references unknown node {node}"
            )));
        }
    }
    Ok(())
}

fn matches_reserved_name(name: &str) -> bool {
    [
        ReservedWrite::Error,
        ReservedWrite::Interrupt,
        ReservedWrite::Resume,
        ReservedWrite::Scheduled,
        ReservedWrite::Push,
        ReservedWrite::Return,
        ReservedWrite::NoWrites,
        ReservedWrite::Tasks,
    ]
    .iter()
    .any(|reserved| reserved.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{ChannelKind, ChannelSpec};
    use crate::config::PregelConfig;
    use crate::node::PregelNode;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct MockNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    impl MockNode {
        fn new(name: &str, triggers: &[&str], reads: &[&str]) -> Self {
            MockNode {
                name: name.to_string(),
                triggers: triggers.iter().map(|s| s.to_string()).collect(),
                reads: reads.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    #[async_trait]
    impl PregelNode for MockNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: crate::node::PregelNodeInput,
            _ctx: &crate::node::PregelNodeContext,
        ) -> Result<crate::node::PregelNodeOutput, anureo_graph_core::GraphError> {
            Ok(crate::node::PregelNodeOutput::default())
        }

        fn subgraphs(&self) -> Vec<crate::subgraph::PregelSubgraph> {
            vec![]
        }
    }

    fn create_valid_graph() -> PregelGraph {
        let mut nodes = HashMap::new();
        nodes.insert(
            "node1".to_string(),
            Arc::new(MockNode::new("node1", &["input1"], &["input1", "input2"]))
                as Arc<dyn PregelNode>,
        );

        let mut channels = HashMap::new();
        channels.insert(
            "input1".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );
        channels.insert(
            "input2".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );

        PregelGraph {
            nodes,
            channels,
            input_channels: vec!["input1".to_string()],
            output_channels: vec![],
            trigger_to_nodes: HashMap::new(),
        }
    }

    fn create_config() -> PregelConfig {
        PregelConfig::default()
    }

    #[test]
    fn test_validate_valid_graph_passes() {
        let graph = create_valid_graph();
        let config = create_config();

        let result = graph.validate_with_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_reserved_channel_name_fails() {
        let mut graph = create_valid_graph();
        graph.channels.insert(
            "__error__".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("reserved"));
    }

    #[test]
    fn test_validate_tasks_channel_with_correct_kind_passes() {
        let mut graph = create_valid_graph();
        graph.channels.insert(
            TASKS_CHANNEL.to_string(),
            ChannelSpec::new(ChannelKind::Tasks),
        );

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tasks_channel_with_wrong_kind_fails() {
        let mut graph = create_valid_graph();
        graph.channels.insert(
            TASKS_CHANNEL.to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Tasks"));
    }

    #[test]
    fn test_validate_channel_kind_tasks_requires_reserved_name() {
        let mut graph = create_valid_graph();
        graph.channels.insert(
            "regular_channel".to_string(),
            ChannelSpec::new(ChannelKind::Tasks),
        );

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Tasks"));
    }

    #[test]
    fn test_validate_node_subscribes_to_unknown_channel_fails() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "node1".to_string(),
            Arc::new(MockNode::new("node1", &["unknown_channel"], &[])) as Arc<dyn PregelNode>,
        );

        let channels = HashMap::new();

        let graph = PregelGraph {
            nodes,
            channels,
            input_channels: vec!["input1".to_string()],
            output_channels: vec![],
            trigger_to_nodes: HashMap::new(),
        };

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("unknown channel"));
    }
    #[test]
    fn test_validate_node_reads_unknown_channel_fails() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "node1".to_string(),
            Arc::new(MockNode::new(
                "node1",
                &["known_channel"],
                &["unknown_channel"],
            )) as Arc<dyn PregelNode>,
        );

        let mut channels = HashMap::new();
        channels.insert(
            "known_channel".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );

        let graph = PregelGraph {
            nodes,
            channels,
            input_channels: vec![],
            output_channels: vec![],
            trigger_to_nodes: HashMap::new(),
        };

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("reads unknown channel"));
    }

    #[test]
    fn test_validate_reserved_node_name_fails() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "__error__".to_string(),
            Arc::new(MockNode::new("__error__", &[], &[])) as Arc<dyn PregelNode>,
        );

        let channels = HashMap::new();

        let graph = PregelGraph {
            nodes,
            channels,
            input_channels: vec![],
            output_channels: vec![],
            trigger_to_nodes: HashMap::new(),
        };

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("reserved"));
    }

    #[test]
    fn test_validate_input_channel_not_defined_fails() {
        let mut graph = create_valid_graph();
        graph.input_channels.push("nonexistent_channel".to_string());

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not defined"));
    }

    #[test]
    fn test_validate_output_channel_not_defined_fails() {
        let mut graph = create_valid_graph();
        graph
            .output_channels
            .push("nonexistent_channel".to_string());

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not defined"));
    }

    #[test]
    fn test_validate_interrupt_before_unknown_node_fails() {
        let mut config = create_config();
        config.interrupt_before.push("unknown_node".to_string());

        let graph = create_valid_graph();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("interrupt_before"));
        assert!(error_msg.contains("unknown node"));
    }

    #[test]
    fn test_validate_interrupt_after_unknown_node_fails() {
        let mut config = create_config();
        config.interrupt_after.push("unknown_node".to_string());

        let graph = create_valid_graph();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("interrupt_after"));
        assert!(error_msg.contains("unknown node"));
    }

    #[test]
    fn test_validate_interrupt_before_known_node_passes() {
        let mut config = create_config();
        config.interrupt_before.push("node1".to_string());

        let graph = create_valid_graph();
        let result = graph.validate_with_config(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_interrupt_after_known_node_passes() {
        let mut config = create_config();
        config.interrupt_after.push("node1".to_string());

        let graph = create_valid_graph();
        let result = graph.validate_with_config(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_multiple_reserved_names_fail() {
        let mut graph = create_valid_graph();
        graph.channels.insert(
            "__interrupt__".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );
        graph.channels.insert(
            "__push__".to_string(),
            ChannelSpec::new(ChannelKind::LastValue),
        );

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_graph_passes() {
        let graph = PregelGraph {
            nodes: HashMap::new(),
            channels: HashMap::new(),
            input_channels: vec![],
            output_channels: vec![],
            trigger_to_nodes: HashMap::new(),
        };

        let config = create_config();
        let result = graph.validate_with_config(&config);

        assert!(result.is_ok());
    }
}
