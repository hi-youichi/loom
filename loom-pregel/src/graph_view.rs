//! Stable graph-export structures for Pregel definitions.
//!
//! These view types provide a serialization-friendly representation of a
//! [`PregelGraph`]. They are meant for tooling, visualization, tests, and
//! debugging rather than execution.

use std::collections::BTreeSet;

use crate::channel::ChannelKind;
use crate::node::PregelGraph;
use crate::types::TASKS_CHANNEL;

/// One node in the exported graph view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PregelGraphNodeView {
    /// Stable node identifier from the graph definition.
    pub name: String,
    /// Channels that trigger the node when updated.
    pub triggers: Vec<String>,
    /// Channels the node may read in addition to its triggers.
    pub reads: Vec<String>,
    /// Names of directly attached child subgraphs.
    pub subgraphs: Vec<String>,
}

/// One channel in the exported graph view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PregelGraphChannelView {
    /// Stable channel identifier from the graph definition.
    pub name: String,
    /// Human-readable channel kind name.
    pub kind: String,
    /// Whether this channel is listed as a graph input.
    pub is_input: bool,
    /// Whether this channel is listed as a graph output.
    pub is_output: bool,
    /// Whether this channel is reserved for runtime-internal bookkeeping.
    pub is_internal: bool,
}

/// Edge relationship in the exported graph view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PregelGraphEdgeKind {
    /// The source channel can schedule the target node.
    Trigger,
    /// The source channel is readable by the target node but does not trigger it.
    Read,
}

/// One edge in the exported graph view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PregelGraphEdgeView {
    /// Source channel name.
    pub source: String,
    /// Target node name.
    pub target: String,
    /// Relationship between the source channel and target node.
    pub kind: PregelGraphEdgeKind,
    /// Optional label used by downstream renderers.
    pub label: Option<String>,
}

/// Serializable static view of a Pregel graph.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PregelGraphView {
    /// All nodes in the graph, sorted by name.
    pub nodes: Vec<PregelGraphNodeView>,
    /// All channels in the graph, sorted by name.
    pub channels: Vec<PregelGraphChannelView>,
    /// Channel names designated as graph inputs.
    pub input_channels: Vec<String>,
    /// Channel names designated as graph outputs.
    pub output_channels: Vec<String>,
    /// Derived edges from channel-to-node relationships.
    pub edges: Vec<PregelGraphEdgeView>,
    /// Recursively exported child graphs when requested.
    pub subgraphs: Vec<PregelNamedGraphView>,
}

/// Named recursive child graph view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PregelNamedGraphView {
    /// Path from the parent graph to the child graph.
    pub path: String,
    /// Exported child graph view.
    pub graph: Box<PregelGraphView>,
}

impl PregelGraphView {
    /// Builds an exported view from a graph definition.
    ///
    /// Child subgraphs are not expanded recursively. Use
    /// [`Self::from_graph_with_subgraphs`] when tooling needs a recursive xray.
    pub fn from_graph(graph: &PregelGraph) -> Self {
        Self::from_graph_with_subgraphs(graph, false)
    }

    /// Builds an exported view from a graph definition.
    ///
    /// When `recurse` is `true`, any subgraphs attached to nodes are exported
    /// into [`PregelNamedGraphView`] entries using `node/subgraph` paths.
    pub fn from_graph_with_subgraphs(graph: &PregelGraph, recurse: bool) -> Self {
        let input_channels = graph.input_channels.clone();
        let output_channels = graph.output_channels.clone();
        let input_set = input_channels.iter().cloned().collect::<BTreeSet<_>>();
        let output_set = output_channels.iter().cloned().collect::<BTreeSet<_>>();

        let mut channels = graph
            .channels
            .iter()
            .map(|(name, spec)| PregelGraphChannelView {
                name: name.clone(),
                kind: channel_kind_name(&spec.kind).to_string(),
                is_input: input_set.contains(name),
                is_output: output_set.contains(name),
                is_internal: name == TASKS_CHANNEL,
            })
            .collect::<Vec<_>>();
        channels.sort_by(|a, b| a.name.cmp(&b.name));

        let mut nodes = graph
            .nodes
            .iter()
            .map(|(name, node)| {
                let mut triggers = node.triggers().to_vec();
                triggers.sort();
                let mut reads = node.reads().to_vec();
                reads.sort();
                let mut subgraphs = node
                    .subgraphs()
                    .into_iter()
                    .map(|subgraph| subgraph.name)
                    .collect::<Vec<_>>();
                subgraphs.sort();
                PregelGraphNodeView {
                    name: name.clone(),
                    triggers,
                    reads,
                    subgraphs,
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));

        let mut edges = Vec::new();
        for node in &nodes {
            for trigger in &node.triggers {
                edges.push(PregelGraphEdgeView {
                    source: trigger.clone(),
                    target: node.name.clone(),
                    kind: PregelGraphEdgeKind::Trigger,
                    label: Some("trigger".to_string()),
                });
            }
            for read in &node.reads {
                if node.triggers.iter().any(|trigger| trigger == read) {
                    continue;
                }
                edges.push(PregelGraphEdgeView {
                    source: read.clone(),
                    target: node.name.clone(),
                    kind: PregelGraphEdgeKind::Read,
                    label: Some("read".to_string()),
                });
            }
        }
        edges.sort_by(|a, b| {
            (a.source.as_str(), a.target.as_str(), edge_kind_rank(a.kind)).cmp(&(
                b.source.as_str(),
                b.target.as_str(),
                edge_kind_rank(b.kind),
            ))
        });

        let mut subgraphs = Vec::new();
        if recurse {
            for (node_name, node) in &graph.nodes {
                for child in node.subgraphs() {
                    let path = format!("{node_name}/{}", child.name);
                    subgraphs.push(PregelNamedGraphView {
                        path,
                        graph: Box::new(Self::from_graph_with_subgraphs(
                            child.runtime.graph().as_ref(),
                            true,
                        )),
                    });
                }
            }
            subgraphs.sort_by(|a, b| a.path.cmp(&b.path));
        }

        Self {
            nodes,
            channels,
            input_channels,
            output_channels,
            edges,
            subgraphs,
        }
    }

    /// Renders the static graph view as a Mermaid flowchart.
    ///
    /// Trigger edges are rendered as solid arrows and read-only edges as dotted
    /// arrows. Recursive subgraphs are currently emitted as comments so the
    /// top-level flow remains stable even when renderers do not support nested
    /// diagrams.
    pub fn to_mermaid(&self) -> String {
        let mut lines = vec!["flowchart TD".to_string()];

        for channel in &self.channels {
            lines.push(format!(
                "    {}([\"channel: {}\"])",
                mermaid_id("channel", &channel.name),
                channel.name
            ));
        }
        for node in &self.nodes {
            lines.push(format!(
                "    {}[\"node: {}\"]",
                mermaid_id("node", &node.name),
                node.name
            ));
        }
        for edge in &self.edges {
            let source = mermaid_id("channel", &edge.source);
            let target = mermaid_id("node", &edge.target);
            match edge.kind {
                PregelGraphEdgeKind::Trigger => {
                    lines.push(format!("    {source} --> {target}"));
                }
                PregelGraphEdgeKind::Read => {
                    lines.push(format!("    {source} -.-> {target}"));
                }
            }
        }

        for subgraph in &self.subgraphs {
            lines.push(format!("    %% subgraph {}", subgraph.path));
        }

        lines.join("\n")
    }
}

fn channel_kind_name(kind: &ChannelKind) -> &'static str {
    match kind {
        ChannelKind::LastValue => "LastValue",
        ChannelKind::Ephemeral => "Ephemeral",
        ChannelKind::Topic { .. } => "Topic",
        ChannelKind::Tasks => "Tasks",
        ChannelKind::BinaryAggregate { .. } => "BinaryAggregate",
        ChannelKind::NamedBarrier { .. } => "NamedBarrier",
    }
}

fn edge_kind_rank(kind: PregelGraphEdgeKind) -> u8 {
    match kind {
        PregelGraphEdgeKind::Trigger => 0,
        PregelGraphEdgeKind::Read => 1,
    }
}

fn mermaid_id(prefix: &str, raw: &str) -> String {
    let sanitized = raw
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    format!("{prefix}_{sanitized}")
}
