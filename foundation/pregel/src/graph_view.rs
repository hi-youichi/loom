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
        channels.sort_by_key(|a| a.name.clone());

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
        nodes.sort_by_key(|a| a.name.clone());

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
        edges.sort_by_cached_key(|a| (a.source.clone(), a.target.clone(), edge_kind_rank(a.kind)));

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
            subgraphs.sort_by_key(|a| a.path.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pregel_graph_node_view_fields() {
        let node = PregelGraphNodeView {
            name: "test-node".to_string(),
            triggers: vec!["trigger1".to_string(), "trigger2".to_string()],
            reads: vec!["read1".to_string()],
            subgraphs: vec!["subgraph1".to_string()],
        };

        assert_eq!(node.name, "test-node");
        assert_eq!(node.triggers.len(), 2);
        assert_eq!(node.reads.len(), 1);
        assert_eq!(node.subgraphs.len(), 1);
    }

    #[test]
    fn test_pregel_graph_node_view_equality() {
        let node1 = PregelGraphNodeView {
            name: "test-node".to_string(),
            triggers: vec!["trigger1".to_string()],
            reads: vec!["read1".to_string()],
            subgraphs: vec![],
        };

        let node2 = PregelGraphNodeView {
            name: "test-node".to_string(),
            triggers: vec!["trigger1".to_string()],
            reads: vec!["read1".to_string()],
            subgraphs: vec![],
        };

        assert_eq!(node1, node2);
    }

    #[test]
    fn test_pregel_graph_node_view_serialization_roundtrip() {
        let node = PregelGraphNodeView {
            name: "test-node".to_string(),
            triggers: vec!["trigger1".to_string()],
            reads: vec!["read1".to_string()],
            subgraphs: vec!["subgraph1".to_string()],
        };

        let serialized = serde_json::to_string(&node).unwrap();
        let deserialized: PregelGraphNodeView = serde_json::from_str(&serialized).unwrap();

        assert_eq!(node, deserialized);
    }

    #[test]
    fn test_pregel_graph_channel_view_fields() {
        let channel = PregelGraphChannelView {
            name: "test-channel".to_string(),
            kind: "LastValue".to_string(),
            is_input: true,
            is_output: false,
            is_internal: false,
        };

        assert_eq!(channel.name, "test-channel");
        assert_eq!(channel.kind, "LastValue");
        assert!(channel.is_input);
        assert!(!channel.is_output);
        assert!(!channel.is_internal);
    }

    #[test]
    fn test_pregel_graph_channel_view_equality() {
        let channel1 = PregelGraphChannelView {
            name: "test-channel".to_string(),
            kind: "Topic".to_string(),
            is_input: false,
            is_output: true,
            is_internal: false,
        };

        let channel2 = PregelGraphChannelView {
            name: "test-channel".to_string(),
            kind: "Topic".to_string(),
            is_input: false,
            is_output: true,
            is_internal: false,
        };

        assert_eq!(channel1, channel2);
    }

    #[test]
    fn test_pregel_graph_channel_view_serialization_roundtrip() {
        let channel = PregelGraphChannelView {
            name: "test-channel".to_string(),
            kind: "Ephemeral".to_string(),
            is_input: true,
            is_output: true,
            is_internal: false,
        };

        let serialized = serde_json::to_string(&channel).unwrap();
        let deserialized: PregelGraphChannelView = serde_json::from_str(&serialized).unwrap();

        assert_eq!(channel, deserialized);
    }

    #[test]
    fn test_pregel_graph_edge_kind_variants() {
        assert_eq!(PregelGraphEdgeKind::Trigger, PregelGraphEdgeKind::Trigger);
        assert_eq!(PregelGraphEdgeKind::Read, PregelGraphEdgeKind::Read);
        assert_ne!(PregelGraphEdgeKind::Trigger, PregelGraphEdgeKind::Read);
    }

    #[test]
    fn test_pregel_graph_edge_kind_serialization_roundtrip() {
        let kind = PregelGraphEdgeKind::Trigger;
        let serialized = serde_json::to_string(&kind).unwrap();
        let deserialized: PregelGraphEdgeKind = serde_json::from_str(&serialized).unwrap();

        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_pregel_graph_edge_view_fields() {
        let edge = PregelGraphEdgeView {
            source: "channel-1".to_string(),
            target: "node-1".to_string(),
            kind: PregelGraphEdgeKind::Trigger,
            label: Some("trigger".to_string()),
        };

        assert_eq!(edge.source, "channel-1");
        assert_eq!(edge.target, "node-1");
        assert_eq!(edge.kind, PregelGraphEdgeKind::Trigger);
        assert_eq!(edge.label, Some("trigger".to_string()));
    }

    #[test]
    fn test_pregel_graph_edge_view_with_none_label() {
        let edge = PregelGraphEdgeView {
            source: "channel-1".to_string(),
            target: "node-1".to_string(),
            kind: PregelGraphEdgeKind::Read,
            label: None,
        };

        assert!(edge.label.is_none());
    }

    #[test]
    fn test_pregel_graph_edge_view_serialization_roundtrip() {
        let edge = PregelGraphEdgeView {
            source: "channel-1".to_string(),
            target: "node-1".to_string(),
            kind: PregelGraphEdgeKind::Trigger,
            label: Some("test-label".to_string()),
        };

        let serialized = serde_json::to_string(&edge).unwrap();
        let deserialized: PregelGraphEdgeView = serde_json::from_str(&serialized).unwrap();

        assert_eq!(edge, deserialized);
    }

    #[test]
    fn test_pregel_named_graph_view_fields() {
        let graph_view = Box::new(PregelGraphView {
            nodes: vec![],
            channels: vec![],
            input_channels: vec![],
            output_channels: vec![],
            edges: vec![],
            subgraphs: vec![],
        });

        let named = PregelNamedGraphView {
            path: "parent/child".to_string(),
            graph: graph_view,
        };

        assert_eq!(named.path, "parent/child");
    }

    #[test]
    fn test_pregel_named_graph_view_serialization_roundtrip() {
        let named = PregelNamedGraphView {
            path: "parent/child".to_string(),
            graph: Box::new(PregelGraphView {
                nodes: vec![],
                channels: vec![],
                input_channels: vec![],
                output_channels: vec![],
                edges: vec![],
                subgraphs: vec![],
            }),
        };

        let serialized = serde_json::to_string(&named).unwrap();
        let deserialized: PregelNamedGraphView = serde_json::from_str(&serialized).unwrap();

        assert_eq!(named.path, deserialized.path);
    }

    #[test]
    fn test_mermaid_id_sanitization() {
        assert_eq!(
            mermaid_id("channel", "test-channel"),
            "channel_test_channel"
        );
        assert_eq!(mermaid_id("node", "node/1"), "node_node_1");
        assert_eq!(
            mermaid_id("channel", "channel with spaces"),
            "channel_channel_with_spaces"
        );
        assert_eq!(
            mermaid_id("node", "node@special#chars"),
            "node_node_special_chars"
        );
    }

    #[test]
    fn test_edge_kind_rank() {
        assert_eq!(edge_kind_rank(PregelGraphEdgeKind::Trigger), 0);
        assert_eq!(edge_kind_rank(PregelGraphEdgeKind::Read), 1);
    }

    #[test]
    fn test_channel_kind_name() {
        use crate::channel::ChannelKind;

        let last_value = ChannelKind::LastValue;
        assert_eq!(channel_kind_name(&last_value), "LastValue");

        let ephemeral = ChannelKind::Ephemeral;
        assert_eq!(channel_kind_name(&ephemeral), "Ephemeral");

        let topic = ChannelKind::Topic { accumulate: true };
        assert_eq!(channel_kind_name(&topic), "Topic");

        let tasks = ChannelKind::Tasks;
        assert_eq!(channel_kind_name(&tasks), "Tasks");

        let binary_aggregate = ChannelKind::BinaryAggregate {
            reducer: std::sync::Arc::new(|_, _| serde_json::json!(null)),
        };
        assert_eq!(channel_kind_name(&binary_aggregate), "BinaryAggregate");

        let named_barrier = ChannelKind::NamedBarrier {
            expected: vec!["node1".to_string()],
        };
        assert_eq!(channel_kind_name(&named_barrier), "NamedBarrier");
    }

    #[test]
    fn test_pregel_graph_view_empty() {
        let view = PregelGraphView {
            nodes: vec![],
            channels: vec![],
            input_channels: vec![],
            output_channels: vec![],
            edges: vec![],
            subgraphs: vec![],
        };

        assert!(view.nodes.is_empty());
        assert!(view.channels.is_empty());
        assert!(view.input_channels.is_empty());
        assert!(view.output_channels.is_empty());
        assert!(view.edges.is_empty());
        assert!(view.subgraphs.is_empty());
    }

    #[test]
    fn test_pregel_graph_view_clone() {
        let view = PregelGraphView {
            nodes: vec![PregelGraphNodeView {
                name: "node-1".to_string(),
                triggers: vec![],
                reads: vec![],
                subgraphs: vec![],
            }],
            channels: vec![],
            input_channels: vec![],
            output_channels: vec![],
            edges: vec![],
            subgraphs: vec![],
        };

        let cloned = view.clone();
        assert_eq!(view.nodes.len(), cloned.nodes.len());
        assert_eq!(view.nodes[0].name, cloned.nodes[0].name);
    }

    #[test]
    fn test_pregel_graph_view_serialization_roundtrip() {
        let view = PregelGraphView {
            nodes: vec![PregelGraphNodeView {
                name: "node-1".to_string(),
                triggers: vec!["channel-1".to_string()],
                reads: vec![],
                subgraphs: vec![],
            }],
            channels: vec![PregelGraphChannelView {
                name: "channel-1".to_string(),
                kind: "LastValue".to_string(),
                is_input: true,
                is_output: false,
                is_internal: false,
            }],
            input_channels: vec!["channel-1".to_string()],
            output_channels: vec![],
            edges: vec![PregelGraphEdgeView {
                source: "channel-1".to_string(),
                target: "node-1".to_string(),
                kind: PregelGraphEdgeKind::Trigger,
                label: Some("trigger".to_string()),
            }],
            subgraphs: vec![],
        };

        let serialized = serde_json::to_string(&view).unwrap();
        let deserialized: PregelGraphView = serde_json::from_str(&serialized).unwrap();

        assert_eq!(view.nodes.len(), deserialized.nodes.len());
        assert_eq!(view.channels.len(), deserialized.channels.len());
        assert_eq!(view.input_channels, deserialized.input_channels);
    }
}
