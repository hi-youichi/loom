//! Pregel graph and node definitions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::cli_run::RunCancellation;
use crate::error::AgentError;
use crate::graph::{GraphInterrupt, Interrupt};
use crate::memory::RunnableConfig;
use crate::pregel::channel::ChannelSpec;
use crate::pregel::runtime::PregelRuntime;
use crate::pregel::subgraph::{SubgraphInvocation, SubgraphResult};
use crate::pregel::types::{
    ChannelName, ChannelValue, InterruptRecord, ManagedValues, NodeName, PregelScratchpad,
    ResumeMap,
};
use crate::stream::{
    ChunkToStreamSender, MessageChunk, StreamEvent, StreamMetadata, StreamMode, StreamWriter,
};

/// Input passed to a Pregel node for one task execution.
#[derive(Debug, Clone, Default)]
pub struct PregelNodeInput {
    pub step: u64,
    pub trigger_values: HashMap<ChannelName, ChannelValue>,
    pub read_values: HashMap<ChannelName, ChannelValue>,
    pub managed_values: ManagedValues,
    pub local_read_values: HashMap<ChannelName, ChannelValue>,
    pub scratchpad: PregelScratchpad,
}

/// Output produced by a Pregel node.
#[derive(Debug, Clone, Default)]
pub struct PregelNodeOutput {
    pub writes: Vec<(ChannelName, ChannelValue)>,
}

/// Runtime context available to Pregel nodes.
#[derive(Clone, Debug, Default)]
pub struct PregelNodeContext {
    pub cancellation: Option<RunCancellation>,
    pub stream_tx: Option<tokio::sync::mpsc::Sender<StreamEvent<ChannelValue>>>,
    pub stream_mode: Vec<StreamMode>,
    pub managed_values: ManagedValues,
    pub pending_interrupts: Vec<InterruptRecord>,
    pub resume_map: ResumeMap,
    pub run_config: RunnableConfig,
    pub parent_runtime: Option<Arc<PregelRuntime>>,
    pub subgraph_links: Arc<Mutex<HashMap<String, Vec<String>>>>,
    pub runtime: ChannelValue,
}

impl PregelNodeContext {
    /// Creates a stream writer from this context.
    pub fn stream_writer(&self) -> StreamWriter<ChannelValue> {
        StreamWriter::new(
            self.stream_tx.clone(),
            self.stream_mode.iter().copied().collect::<HashSet<_>>(),
        )
    }

    /// Emits a custom JSON payload when `StreamMode::Custom` is enabled.
    pub async fn emit_custom(&self, value: ChannelValue) -> bool {
        self.stream_writer().emit_custom(value).await
    }

    /// Emits a message chunk when `StreamMode::Messages` is enabled.
    pub async fn emit_message(
        &self,
        content: impl Into<String>,
        node_id: impl Into<String>,
    ) -> bool {
        self.stream_writer().emit_message(content, node_id).await
    }

    /// Emits a specific message chunk, preserving the message kind.
    pub async fn emit_message_chunk(
        &self,
        chunk: MessageChunk,
        node_id: impl Into<String>,
    ) -> bool {
        if !self.stream_mode.contains(&StreamMode::Messages) {
            return false;
        }
        let Some(tx) = &self.stream_tx else {
            return false;
        };
        tx.send(StreamEvent::Messages {
            chunk,
            metadata: StreamMetadata {
                loom_node: node_id.into(),
                namespace: if self.run_config.checkpoint_ns.is_empty() {
                    None
                } else {
                    Some(self.run_config.checkpoint_ns.clone())
                },
            },
        })
        .await
        .is_ok()
    }

    /// Creates a chunk-forwarding adapter for nodes that stream token-by-token output.
    pub fn chunk_stream_sender(
        &self,
        node_id: impl Into<String>,
    ) -> Option<ChunkToStreamSender<ChannelValue>> {
        if !self.stream_mode.contains(&StreamMode::Messages) {
            return None;
        }
        let namespace = if self.run_config.checkpoint_ns.is_empty() {
            None
        } else {
            Some(self.run_config.checkpoint_ns.clone())
        };
        self.stream_tx
            .clone()
            .map(|tx| ChunkToStreamSender::new_with_namespace(tx, node_id, namespace))
    }

    /// Returns whether a specific stream mode is enabled.
    pub fn is_streaming_mode(&self, mode: StreamMode) -> bool {
        self.stream_mode.contains(&mode)
    }

    /// Invokes a child Pregel runtime inside the current parent run.
    pub async fn invoke_subgraph(
        &self,
        child_runtime: &PregelRuntime,
        invocation: SubgraphInvocation,
    ) -> Result<SubgraphResult, AgentError> {
        let Some(parent_runtime) = &self.parent_runtime else {
            return Err(AgentError::ExecutionFailed(
                "pregel subgraph invocation requires a parent runtime".to_string(),
            ));
        };

        let mut config = self.run_config.clone();
        // Subgraphs restore by namespace lineage, not by inheriting the parent's checkpoint id.
        config.checkpoint_id = None;
        config.resume_from_node_id = None;
        let child_namespace = invocation.child_namespace.0.clone();
        let result = parent_runtime
            .invoke_subgraph_with_stream(
                child_runtime,
                config.clone(),
                invocation,
                self.stream_tx.clone(),
            )
            .await?;

        if let Some(state) = child_runtime
            .get_state(RunnableConfig {
                checkpoint_ns: child_namespace.clone(),
                checkpoint_id: None,
                ..config
            })
            .await?
        {
            self.record_subgraph_checkpoint(child_namespace, state.checkpoint_id);
        }

        Ok(result)
    }

    /// Invokes a child Pregel runtime and maps the result back into normal node semantics.
    pub async fn run_subgraph(
        &self,
        child_runtime: &PregelRuntime,
        invocation: SubgraphInvocation,
    ) -> Result<ChannelValue, AgentError> {
        match self.invoke_subgraph(child_runtime, invocation).await? {
            SubgraphResult::Completed(value) => Ok(value),
            SubgraphResult::Interrupted(record) => Err(AgentError::Interrupted(GraphInterrupt(
                Interrupt::with_id(interrupt_value_from_record(&record), record.interrupt_id),
            ))),
            SubgraphResult::Cancelled => Err(AgentError::Cancelled),
            SubgraphResult::Failed(error) => Err(AgentError::ExecutionFailed(error)),
        }
    }

    /// Records a child checkpoint id under its namespace for later parent persistence.
    pub fn record_subgraph_checkpoint(
        &self,
        child_namespace: impl Into<String>,
        checkpoint_id: impl Into<String>,
    ) {
        let child_namespace = child_namespace.into();
        let checkpoint_id = checkpoint_id.into();
        if let Ok(mut guard) = self.subgraph_links.lock() {
            let entry = guard.entry(child_namespace).or_default();
            if entry.last() != Some(&checkpoint_id) {
                entry.push(checkpoint_id);
            }
        }
    }

    /// Returns a snapshot of subgraph checkpoint links recorded during this run.
    pub fn subgraph_links(&self) -> HashMap<String, Vec<String>> {
        self.subgraph_links
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

fn interrupt_value_from_record(record: &InterruptRecord) -> ChannelValue {
    match record.value.clone() {
        ChannelValue::Object(mut map) => {
            map.entry("kind".to_string())
                .or_insert_with(|| serde_json::json!("subgraph_interrupt"));
            map.entry("namespace".to_string())
                .or_insert_with(|| serde_json::json!(record.namespace.clone()));
            map.entry("task_id".to_string())
                .or_insert_with(|| serde_json::json!(record.task_id.clone()));
            map.entry("node_name".to_string())
                .or_insert_with(|| serde_json::json!(record.node_name.clone()));
            map.entry("step".to_string())
                .or_insert_with(|| serde_json::json!(record.step));
            ChannelValue::Object(map)
        }
        other => serde_json::json!({
            "kind": "subgraph_interrupt",
            "namespace": record.namespace,
            "task_id": record.task_id,
            "node_name": record.node_name,
            "step": record.step,
            "value": other,
        }),
    }
}

/// Runtime node contract for Pregel execution.
#[async_trait]
pub trait PregelNode: Send + Sync {
    /// Stable node name.
    fn name(&self) -> &str;

    /// Channels that trigger this node.
    fn triggers(&self) -> &[ChannelName];

    /// Channels this node reads when building input.
    fn reads(&self) -> &[ChannelName];

    /// Executes the node once for the prepared input.
    async fn run(
        &self,
        input: PregelNodeInput,
        ctx: &PregelNodeContext,
    ) -> Result<PregelNodeOutput, AgentError>;
}

/// Static graph definition for a Pregel runtime.
#[derive(Default)]
pub struct PregelGraph {
    pub nodes: HashMap<NodeName, Arc<dyn PregelNode>>,
    pub channels: HashMap<ChannelName, ChannelSpec>,
    pub input_channels: Vec<ChannelName>,
    pub output_channels: Vec<ChannelName>,
    pub trigger_to_nodes: HashMap<ChannelName, Vec<NodeName>>,
}

impl std::fmt::Debug for PregelGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PregelGraph")
            .field("nodes", &self.nodes.keys().collect::<Vec<_>>())
            .field("channels", &self.channels)
            .field("input_channels", &self.input_channels)
            .field("output_channels", &self.output_channels)
            .field("trigger_to_nodes", &self.trigger_to_nodes)
            .finish()
    }
}

impl PregelGraph {
    /// Creates an empty Pregel graph definition.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a channel definition to the graph.
    pub fn add_channel(&mut self, name: impl Into<String>, spec: ChannelSpec) -> &mut Self {
        self.channels.insert(name.into(), spec);
        self
    }

    /// Adds a node definition to the graph.
    pub fn add_node(&mut self, node: Arc<dyn PregelNode>) -> &mut Self {
        self.nodes.insert(node.name().to_string(), node);
        self
    }

    /// Sets input channels used to seed the first step.
    pub fn set_input_channels(&mut self, names: Vec<String>) -> &mut Self {
        self.input_channels = names;
        self
    }

    /// Sets output channels used to materialize run output.
    pub fn set_output_channels(&mut self, names: Vec<String>) -> &mut Self {
        self.output_channels = names;
        self
    }

    /// Rebuilds the trigger index from registered nodes.
    pub fn build_trigger_index(&mut self) -> &mut Self {
        let mut trigger_to_nodes: HashMap<ChannelName, Vec<NodeName>> = HashMap::new();
        for (node_name, node) in &self.nodes {
            for trigger in node.triggers() {
                trigger_to_nodes
                    .entry(trigger.clone())
                    .or_default()
                    .push(node_name.clone());
            }
        }
        self.trigger_to_nodes = trigger_to_nodes;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pregel::channel::{ChannelKind, ChannelSpec};
    use crate::stream::{MessageChunk, StreamEvent, StreamMode};
    use serde_json::json;
    use tokio::sync::mpsc;

    #[derive(Debug)]
    struct DummyNode {
        name: String,
        triggers: Vec<ChannelName>,
        reads: Vec<ChannelName>,
    }

    #[async_trait]
    impl PregelNode for DummyNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[ChannelName] {
            &self.triggers
        }

        fn reads(&self) -> &[ChannelName] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            _ctx: &PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![("out".to_string(), json!(1))],
            })
        }
    }

    #[test]
    fn graph_builds_trigger_index() {
        let mut graph = PregelGraph::new();
        graph.add_channel("in", ChannelSpec::new(ChannelKind::LastValue));
        graph.add_node(Arc::new(DummyNode {
            name: "n1".to_string(),
            triggers: vec!["in".to_string()],
            reads: vec!["in".to_string()],
        }));
        graph.build_trigger_index();

        assert_eq!(
            graph.trigger_to_nodes.get("in"),
            Some(&vec!["n1".to_string()])
        );
    }

    #[tokio::test]
    async fn context_can_emit_custom_and_message_events() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<ChannelValue>>(8);
        let ctx = PregelNodeContext {
            stream_tx: Some(tx),
            stream_mode: vec![StreamMode::Custom, StreamMode::Messages],
            ..Default::default()
        };

        assert!(ctx.emit_custom(json!({"progress": 50})).await);
        assert!(ctx.emit_message("hello", "node-a").await);
        assert!(
            ctx.emit_message_chunk(MessageChunk::thinking("plan"), "node-a")
                .await
        );

        match rx.recv().await.expect("custom event") {
            StreamEvent::Custom(value) => assert_eq!(value["progress"], json!(50)),
            other => panic!("expected custom event, got {other:?}"),
        }

        match rx.recv().await.expect("message event") {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "hello");
                assert_eq!(metadata.loom_node, "node-a");
            }
            other => panic!("expected messages event, got {other:?}"),
        }

        match rx.recv().await.expect("thinking chunk") {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "plan");
                assert_eq!(chunk.kind, crate::stream::MessageChunkKind::Thinking);
                assert_eq!(metadata.loom_node, "node-a");
            }
            other => panic!("expected thinking event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn context_chunk_stream_sender_forwards_chunks() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<ChannelValue>>(8);
        let ctx = PregelNodeContext {
            stream_tx: Some(tx),
            stream_mode: vec![StreamMode::Messages],
            ..Default::default()
        };

        let adapter = ctx
            .chunk_stream_sender("node-b")
            .expect("messages mode should create adapter");
        let (chunk_tx, chunk_rx) = adapter.channel();
        chunk_tx
            .send(MessageChunk::thinking("first"))
            .await
            .expect("send first chunk");
        chunk_tx
            .send(MessageChunk::message("second"))
            .await
            .expect("send second chunk");
        drop(chunk_tx);

        let (forwarded, _) = adapter.forward(chunk_rx).await;
        assert_eq!(forwarded, 2);

        match rx.recv().await.expect("first forwarded event") {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "first");
                assert_eq!(chunk.kind, crate::stream::MessageChunkKind::Thinking);
                assert_eq!(metadata.loom_node, "node-b");
            }
            other => panic!("expected first forwarded message, got {other:?}"),
        }

        match rx.recv().await.expect("second forwarded event") {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "second");
                assert_eq!(chunk.kind, crate::stream::MessageChunkKind::Message);
                assert_eq!(metadata.loom_node, "node-b");
            }
            other => panic!("expected second forwarded message, got {other:?}"),
        }
    }
}
