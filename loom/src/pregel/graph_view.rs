//! Stable graph-export structures for Pregel definitions.
//!
//! These view types provide a serialization-friendly representation of a
//! [`PregelGraph`]. They are meant for tooling, visualization, tests, and
//! debugging rather than execution.

use std::collections::BTreeSet;

use crate::pregel::channel::ChannelKind;
use crate::pregel::node::PregelGraph;
use crate::pregel::types::TASKS_CHANNEL;

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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    use crate::error::AgentError;
    use crate::pregel::channel::ChannelSpec;
    use crate::pregel::node::{
        PregelGraph, PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput,
    };
    use crate::pregel::subgraph::PregelSubgraph;
    use crate::pregel::PregelRuntime;

    #[derive(Debug)]
    struct DummyNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        subgraphs: Vec<PregelSubgraph>,
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

        fn subgraphs(&self) -> Vec<PregelSubgraph> {
            self.subgraphs.clone()
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
    fn graph_view_contains_nodes_channels_and_edges() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("memory", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "worker".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string(), "memory".to_string()],
                subgraphs: Vec::new(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let view = PregelGraphView::from_graph(&graph);
        assert_eq!(view.nodes.len(), 1);
        assert_eq!(view.channels.len(), 3);
        assert_eq!(view.edges.len(), 2);
        assert!(view.nodes[0].subgraphs.is_empty());
        assert_eq!(view.edges[0].kind, PregelGraphEdgeKind::Trigger);
        assert_eq!(view.edges[1].kind, PregelGraphEdgeKind::Read);
    }

    #[test]
    fn graph_view_renders_mermaid() {
        let view = PregelGraphView {
            nodes: vec![PregelGraphNodeView {
                name: "worker".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["memory".to_string()],
                subgraphs: Vec::new(),
            }],
            channels: vec![
                PregelGraphChannelView {
                    name: "in".to_string(),
                    kind: "LastValue".to_string(),
                    is_input: true,
                    is_output: false,
                    is_internal: false,
                },
                PregelGraphChannelView {
                    name: "memory".to_string(),
                    kind: "LastValue".to_string(),
                    is_input: false,
                    is_output: false,
                    is_internal: false,
                },
            ],
            input_channels: vec!["in".to_string()],
            output_channels: Vec::new(),
            edges: vec![
                PregelGraphEdgeView {
                    source: "in".to_string(),
                    target: "worker".to_string(),
                    kind: PregelGraphEdgeKind::Trigger,
                    label: Some("trigger".to_string()),
                },
                PregelGraphEdgeView {
                    source: "memory".to_string(),
                    target: "worker".to_string(),
                    kind: PregelGraphEdgeKind::Read,
                    label: Some("read".to_string()),
                },
            ],
            subgraphs: Vec::new(),
        };

        let mermaid = view.to_mermaid();
        assert!(mermaid.contains("flowchart TD"));
        assert!(mermaid.contains("channel_in"));
        assert!(mermaid.contains("node_worker"));
        assert!(mermaid.contains("-->"));
        assert!(mermaid.contains("-.->"));
    }

    #[test]
    fn graph_view_can_include_recursive_subgraphs() {
        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("child_in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("child_out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "child_worker".to_string(),
                triggers: vec!["child_in".to_string()],
                reads: vec!["child_in".to_string()],
                subgraphs: Vec::new(),
            }))
            .set_input_channels(vec!["child_in".to_string()])
            .set_output_channels(vec!["child_out".to_string()])
            .build_trigger_index();
        let child_runtime = Arc::new(PregelRuntime::new(child_graph));

        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(DummyNode {
                name: "worker".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                subgraphs: vec![PregelSubgraph {
                    name: "child".to_string(),
                    runtime: child_runtime,
                }],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let view = PregelGraphView::from_graph_with_subgraphs(&graph, true);
        assert_eq!(view.subgraphs.len(), 1);
        assert_eq!(view.subgraphs[0].path, "worker/child");
        assert_eq!(view.subgraphs[0].graph.nodes[0].name, "child_worker");
    }
}
