//! Static validation for Pregel graph definitions.
//!
//! Validation is intentionally structural: it checks graph topology, reserved
//! names, channel references, and interrupt configuration before a run starts.
//! It does not try to prove that node logic is correct.

use std::collections::HashSet;

use loom_llm::AgentError;
use crate::channel::ChannelKind;
use crate::config::PregelConfig;
use crate::node::{PregelGraph, PregelNode};
use crate::types::{ReservedWrite, TASKS_CHANNEL};

impl PregelGraph {
    /// Validates graph topology, channel references, and configured interrupts.
    ///
    /// This is the same validation used by [`crate::PregelRuntime`]
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
