//! Static validation for Pregel graph definitions.
//!
//! Validation is intentionally structural: it checks graph topology, reserved
//! names, channel references, and interrupt configuration before a run starts.
//! It does not try to prove that node logic is correct.

use std::collections::HashSet;

use crate::error::AgentError;
use crate::pregel::channel::ChannelKind;
use crate::pregel::config::PregelConfig;
use crate::pregel::node::{PregelGraph, PregelNode};
use crate::pregel::types::{ReservedWrite, TASKS_CHANNEL};

impl PregelGraph {
    /// Validates graph topology, channel references, and configured interrupts.
    ///
    /// This is the same validation used by [`crate::pregel::PregelRuntime`]
    /// before executing or exporting a graph.
    pub fn validate_with_config(&self, config: &PregelConfig) -> Result<(), AgentError> {
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

fn validate_channel_name(name: &str, kind: &ChannelKind) -> Result<(), AgentError> {
    if name == TASKS_CHANNEL {
        if matches!(kind, ChannelKind::Tasks) {
            return Ok(());
        }
        return Err(AgentError::ExecutionFailed(format!(
            "reserved channel {TASKS_CHANNEL} must use ChannelKind::Tasks"
        )));
    }

    if matches_reserved_name(name) {
        return Err(AgentError::ExecutionFailed(format!(
            "channel name {name} is reserved"
        )));
    }

    if matches!(kind, ChannelKind::Tasks) {
        return Err(AgentError::ExecutionFailed(format!(
            "ChannelKind::Tasks must use reserved channel name {TASKS_CHANNEL}"
        )));
    }

    Ok(())
}

fn validate_node_name(name: &str) -> Result<(), AgentError> {
    if matches_reserved_name(name) {
        return Err(AgentError::ExecutionFailed(format!(
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
) -> Result<(), AgentError> {
    let reads_push_payload = node
        .triggers()
        .iter()
        .any(|trigger| trigger == TASKS_CHANNEL);

    for trigger in node.triggers() {
        if !known_channels.contains(trigger) {
            return Err(AgentError::ExecutionFailed(format!(
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
            return Err(AgentError::ExecutionFailed(format!(
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
) -> Result<(), AgentError> {
    for channel in input_channels {
        if !known_channels.contains(channel) {
            return Err(AgentError::ExecutionFailed(format!(
                "input channel {channel} is not defined"
            )));
        }
    }
    Ok(())
}

fn validate_output_channels(
    output_channels: &[String],
    known_channels: &HashSet<String>,
) -> Result<(), AgentError> {
    for channel in output_channels {
        if !known_channels.contains(channel) {
            return Err(AgentError::ExecutionFailed(format!(
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
) -> Result<(), AgentError> {
    for node in nodes {
        if !known_nodes.contains_key(node) {
            return Err(AgentError::ExecutionFailed(format!(
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
    use async_trait::async_trait;
    use std::sync::Arc;

    use crate::pregel::channel::ChannelSpec;
    use crate::pregel::node::{PregelNodeContext, PregelNodeInput, PregelNodeOutput};

    #[derive(Debug)]
    struct DummyNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[async_trait]
    impl PregelNode for DummyNode {
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
            _input: PregelNodeInput,
            _ctx: &PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput::default())
        }
    }

    #[test]
    fn validate_rejects_unknown_input_channel() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "node".to_string(),
                triggers: vec!["out".to_string()],
                reads: vec!["out".to_string()],
            }))
            .set_input_channels(vec!["missing".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let err = graph
            .validate_with_config(&PregelConfig::default())
            .expect_err("graph should be invalid");
        assert!(err
            .to_string()
            .contains("input channel missing is not defined"));
    }

    #[test]
    fn validate_rejects_unknown_interrupt_node() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "node".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let config = PregelConfig {
            interrupt_before: vec!["missing".to_string()],
            ..PregelConfig::default()
        };
        let err = graph
            .validate_with_config(&config)
            .expect_err("graph should be invalid");
        assert!(err
            .to_string()
            .contains("interrupt_before references unknown node missing"));
    }

    #[test]
    fn validate_allows_reserved_tasks_channel_with_tasks_kind() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel(TASKS_CHANNEL, ChannelSpec::new(ChannelKind::Tasks))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "worker".to_string(),
                triggers: vec![TASKS_CHANNEL.to_string()],
                reads: vec![TASKS_CHANNEL.to_string()],
            }))
            .set_input_channels(vec![TASKS_CHANNEL.to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        graph
            .validate_with_config(&PregelConfig::default())
            .expect("tasks channel is a supported internal channel");
    }

    #[test]
    fn validate_rejects_reserved_tasks_channel_with_wrong_kind() {
        let mut graph = PregelGraph::new();
        graph.add_channel(TASKS_CHANNEL, ChannelSpec::new(ChannelKind::LastValue));

        let err = graph
            .validate_with_config(&PregelConfig::default())
            .expect_err("graph should be invalid");
        assert!(err
            .to_string()
            .contains("reserved channel __tasks__ must use ChannelKind::Tasks"));
    }
}
