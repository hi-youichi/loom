//! State graph: nodes + explicit edges (from → to) and optional conditional edges.
//!
//! Add nodes with `add_node`, define the chain with `add_edge(from, to)` using
//! `START` and `END` for graph entry/exit. Use `add_conditional_edges` to route
//! to the next node based on state. Then `compile`
//! or `compile_with_checkpointer` to get a `CompiledStateGraph`.
//!
//! # Conditional edges
//!
//! From a source node, a routing function `(state) -> key` is called; the key is
//! used as the next node id, or looked up in an optional path map. A node must have
//! either one outgoing `add_edge` or `add_conditional_edges`, not both.
//!
//! # State Updates
//!
//! By default, nodes return a new state that completely replaces the previous state.
//! To customize this behavior (e.g., append to lists, aggregate values), use
//! `with_state_updater` to provide a custom `StateUpdater` implementation.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use crate::channels::{boxed_updater, BoxedStateUpdater, ReplaceUpdater};
use crate::compile_error::CompilationError;
use crate::compiled::CompiledStateGraph;
use crate::conditional::{ConditionalRouter, ConditionalRouterFn, NextEntry};
use crate::interrupt::InterruptHandler;
use crate::node::Node;
use crate::node_middleware::NodeMiddleware;
use crate::retry::RetryPolicy;
use checkpoint::{Checkpointer, Store};

/// Type alias for metadata extractor closure used to extract checkpoint summaries from state.
pub type MetadataExtractorFn<S> = Arc<dyn Fn(&S) -> Option<String> + Send + Sync>;

/// Sentinel for graph entry: use as `from_id` in `add_edge(START, first_node_id)`.
pub const START: &str = "__start__";

/// Sentinel for graph exit: use as `to_id` in `add_edge(last_node_id, END)`.
pub const END: &str = "__end__";

/// State graph: nodes plus explicit edges and optional conditional edges.
///
/// Generic over state type `S`. Build with `add_node` / `add_edge(from, to)` (use
/// `START` and `END` for entry/exit), and optionally `add_conditional_edges` for
/// state-based routing. Then `compile()` or `compile_with_middleware()` to obtain
/// an executable graph.
///
/// **Interaction**: Accepts `Arc<dyn Node<S>>`; produces `CompiledStateGraph<S>`.
/// Middleware can be set via `with_middleware` for fluent API or passed to `compile_with_middleware`.
/// External crates can extend the chain via extension traits (methods that take `self` and return `Self`).
///
/// **State Updates**: By default, node outputs replace the entire state. Use `with_state_updater`
/// to customize how updates are merged (e.g., append to lists, aggregate values).
pub struct StateGraph<S> {
    nodes: HashMap<String, Arc<dyn Node<S>>>,
    /// Edges (from_id, to_id). A node may have one outgoing edge or conditional_edges, not both.
    edges: Vec<(String, String)>,
    /// Conditional edges: source node id -> (router, path_map). Next node is resolved from state at runtime.
    conditional_edges: HashMap<String, ConditionalRouter<S>>,
    /// Optional long-term store; when set, compiled graph holds it for nodes (e.g. via config or node construction).
    store: Option<Arc<dyn Store>>,
    /// Optional node middleware; when set, `compile()` uses it (fluent API). See `with_middleware`.
    middleware: Option<Arc<dyn NodeMiddleware<S>>>,
    /// Optional state updater; when set, controls how node outputs are merged into state.
    /// Default is `ReplaceUpdater` which fully replaces the state.
    state_updater: Option<BoxedStateUpdater<S>>,
    /// Retry policy for node execution. Default is `RetryPolicy::None`.
    retry_policy: RetryPolicy,
    /// Optional interrupt handler for human-in-the-loop scenarios.
    interrupt_handler: Option<Arc<dyn InterruptHandler>>,
    metadata_extractor: Option<MetadataExtractorFn<S>>,
}

impl<S> Default for StateGraph<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> StateGraph<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            conditional_edges: HashMap::new(),
            store: None,
            middleware: None,
            state_updater: None,
            retry_policy: RetryPolicy::None,
            interrupt_handler: None,
            metadata_extractor: None,
        }
    }

    /// Attaches a long-term store to the graph. When compiled, the graph holds `Option<Arc<dyn Store>>`;
    /// nodes can use it for cross-thread memory (e.g. namespace from `RunnableConfig::user_id`).
    pub fn with_store(self, store: Arc<dyn Store>) -> Self {
        Self {
            store: Some(store),
            ..self
        }
    }

    /// Attaches node middleware for fluent API. When set, `compile()` will use it.
    /// Chain with `compile()`: `graph.with_middleware(m).compile()?`.
    pub fn with_middleware(self, middleware: Arc<dyn NodeMiddleware<S>>) -> Self {
        Self {
            middleware: Some(middleware),
            ..self
        }
    }

    /// Attaches a custom state updater to the graph.
    ///
    /// The state updater controls how node outputs are merged into the current state.
    /// By default (`ReplaceUpdater`), the node's output completely replaces the state.
    ///
    /// Use `FieldBasedUpdater` for custom per-field update logic (e.g., append to lists).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use loom_graph_core::StateGraph;
    /// use loom_graph_core::channels::FieldBasedUpdater;
    /// use std::sync::Arc;
    ///
    /// #[derive(Clone, Debug)]
    /// struct MyState { messages: Vec<String>, count: i32 }
    ///
    /// let updater = FieldBasedUpdater::new(|current: &mut MyState, update: &MyState| {
    ///     current.messages.extend(update.messages.iter().cloned());
    ///     current.count = update.count;
    /// });
    ///
    /// let graph = StateGraph::<MyState>::new()
    ///     .with_state_updater(Arc::new(updater));
    /// ```
    pub fn with_state_updater(self, updater: BoxedStateUpdater<S>) -> Self {
        Self {
            state_updater: Some(updater),
            ..self
        }
    }

    /// Attaches a retry policy for node execution.
    ///
    /// When a node execution fails, the retry policy determines if and how
    /// the execution should be retried. Default is `RetryPolicy::None` (no retries).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use loom_graph_core::{StateGraph, RetryPolicy};
    /// use std::time::Duration;
    ///
    /// let graph = StateGraph::<String>::new()
    ///     .with_retry_policy(RetryPolicy::exponential(
    ///         3,
    ///         Duration::from_millis(100),
    ///         Duration::from_secs(5),
    ///         2.0,
    ///     ));
    /// ```
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self {
        Self {
            retry_policy,
            ..self
        }
    }

    /// Attaches an interrupt handler for human-in-the-loop scenarios.
    ///
    /// The interrupt handler is called when a node raises an interrupt.
    /// This is useful for scenarios where execution needs to pause for
    /// user input or approval.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use loom_graph_core::{StateGraph, DefaultInterruptHandler};
    /// use std::sync::Arc;
    ///
    /// let graph = StateGraph::<String>::new()
    ///     .with_interrupt_handler(Arc::new(DefaultInterruptHandler));
    /// ```
    pub fn with_interrupt_handler(self, handler: Arc<dyn InterruptHandler>) -> Self {
        Self {
            interrupt_handler: Some(handler),
            ..self
        }
    }

    pub fn with_metadata_extractor(self, extractor: MetadataExtractorFn<S>) -> Self {
        Self {
            metadata_extractor: Some(extractor),
            ..self
        }
    }

    /// Adds a node; id must be unique. Replaces if same id.
    ///
    /// Returns `&mut Self` for method chaining. The node is stored as
    /// `Arc<dyn Node<S>>`; use `add_edge` to include it in the chain.
    pub fn add_node(&mut self, id: impl Into<String>, node: Arc<dyn Node<S>>) -> &mut Self {
        self.nodes.insert(id.into(), node);
        self
    }

    /// Adds an edge from `from_id` to `to_id`.
    ///
    /// Use `START` for graph entry and `END` for graph exit. Both ids (except
    /// START/END) must be registered via `add_node` before `compile()`.
    /// A node may have either one outgoing edge or `add_conditional_edges`, not both.
    /// With conditional edges, the graph may branch; otherwise edges form a single linear chain.
    pub fn add_edge(&mut self, from_id: impl Into<String>, to_id: impl Into<String>) -> &mut Self {
        self.edges.push((from_id.into(), to_id.into()));
        self
    }

    /// Adds conditional edges from `source` node: next node is determined by `path(state)`.
    ///
    /// Adds conditional edges: `add_conditional_edges(source, path, path_map)`.
    /// After the source node runs, `path` is called with the updated state; its return value
    /// is used as the next node id, or looked up in `path_map` when provided.
    ///
    /// - When `path_map` is `None`, the return value of `path` is the next node id (or END).
    /// - When `path_map` is `Some(map)`, the return value is the key; next node is
    ///   `map[key]` if present, otherwise the key itself.
    ///
    /// The source node must not have an outgoing `add_edge`; it must have either
    /// one edge or conditional edges. All path_map values (and direct keys when no map)
    /// must be valid node ids or `END`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use loom_graph_core::{StateGraph, END};
    /// use std::collections::HashMap;
    /// use std::sync::Arc;
    ///
    /// let mut graph = StateGraph::<MyState>::new();
    /// graph.add_node("think", think_node);
    /// graph.add_node("act", act_node);
    /// graph.add_edge(START, "think");
    /// graph.add_edge("act", END);
    /// graph.add_conditional_edges(
    ///     "think",
    ///     Arc::new(|s| if s.has_tool_calls() { "tools".into() } else { END.into() }),
    ///     Some([("tools".into(), "act".into()), (END.into(), END.into())].into_iter().collect()),
    /// );
    /// ```
    pub fn add_conditional_edges(
        &mut self,
        source: impl Into<String>,
        path: ConditionalRouterFn<S>,
        path_map: Option<HashMap<String, String>>,
    ) -> &mut Self {
        self.conditional_edges
            .insert(source.into(), ConditionalRouter::new(path, path_map));
        self
    }

    /// Builds the executable graph: validates that all edge node ids exist and
    /// edges form a single linear chain from START to END.
    /// If middleware was set via `with_middleware`, it is used; otherwise no middleware.
    ///
    /// Returns `CompilationError` if any edge references an unknown node or
    /// the chain is invalid. On success, the graph is immutable and ready for `invoke`.
    pub fn compile(self) -> Result<CompiledStateGraph<S>, CompilationError> {
        let middleware = self.middleware.clone();
        self.compile_internal(None, middleware)
    }

    /// Builds the executable graph with a checkpointer for persistence (thread_id in config).
    ///
    /// Compiles with optional checkpointer. When `invoke(state, config)`
    /// is called with `config.thread_id`, the final state is saved after the run.
    pub fn compile_with_checkpointer(
        self,
        checkpointer: Arc<dyn Checkpointer<S>>,
    ) -> Result<CompiledStateGraph<S>, CompilationError> {
        self.compile_internal(Some(checkpointer), None)
    }

    /// Builds the executable graph with node middleware. The middleware wraps each node.run in invoke.
    pub fn compile_with_middleware(
        self,
        middleware: Arc<dyn NodeMiddleware<S>>,
    ) -> Result<CompiledStateGraph<S>, CompilationError> {
        self.compile_internal(None, Some(middleware))
    }

    /// Builds the executable graph with both checkpointer and node middleware.
    pub fn compile_with_checkpointer_and_middleware(
        self,
        checkpointer: Arc<dyn Checkpointer<S>>,
        middleware: Arc<dyn NodeMiddleware<S>>,
    ) -> Result<CompiledStateGraph<S>, CompilationError> {
        self.compile_internal(Some(checkpointer), Some(middleware))
    }

    fn compile_internal(
        self,
        checkpointer: Option<Arc<dyn Checkpointer<S>>>,
        middleware: Option<Arc<dyn NodeMiddleware<S>>>,
    ) -> Result<CompiledStateGraph<S>, CompilationError> {
        for (from, to) in &self.edges {
            if from != START && !self.nodes.contains_key(from) {
                return Err(CompilationError::NodeNotFound(from.clone()));
            }
            if to != END && !self.nodes.contains_key(to) {
                return Err(CompilationError::NodeNotFound(to.clone()));
            }
        }
        for (source, router) in &self.conditional_edges {
            if !self.nodes.contains_key(source) {
                return Err(CompilationError::NodeNotFound(source.clone()));
            }
            if let Some(ref path_map) = router.path_map {
                for target in path_map.values() {
                    if target != END && !self.nodes.contains_key(target) {
                        return Err(CompilationError::InvalidConditionalPathMap(target.clone()));
                    }
                }
            }
        }

        let start_edges: Vec<_> = self
            .edges
            .iter()
            .filter(|(f, _)| f == START)
            .map(|(_, t)| t.clone())
            .collect();
        let first = match start_edges.len() {
            0 => return Err(CompilationError::MissingStart),
            1 => start_edges.into_iter().next().unwrap(),
            _ => {
                return Err(CompilationError::InvalidChain(
                    "multiple edges from START (branch)".into(),
                ))
            }
        };

        let has_end = self.edges.iter().any(|(_, t)| t == END)
            || self.conditional_edges.values().any(|r| {
                r.path_map
                    .as_ref()
                    .is_none_or(|m| m.values().any(|v| v == END))
            });
        if !has_end {
            return Err(CompilationError::MissingEnd);
        }

        let edge_froms: HashSet<_> = self
            .edges
            .iter()
            .filter(|(f, _)| f.as_str() != START)
            .map(|(f, _)| f.clone())
            .collect();
        if edge_froms.len()
            != self
                .edges
                .iter()
                .filter(|(f, _)| f.as_str() != START)
                .count()
        {
            return Err(CompilationError::InvalidChain(
                "duplicate from (branch)".into(),
            ));
        }
        for source in self.conditional_edges.keys() {
            if edge_froms.contains(source) {
                return Err(CompilationError::NodeHasBothEdgeAndConditional(
                    source.clone(),
                ));
            }
        }

        let mut next_map: HashMap<String, NextEntry<S>> = self
            .edges
            .iter()
            .filter(|(f, _)| f.as_str() != START)
            .map(|(f, t)| (f.clone(), NextEntry::Unconditional(t.clone())))
            .collect();
        for (source, router) in &self.conditional_edges {
            next_map.insert(source.clone(), NextEntry::Conditional(router.clone()));
        }

        let mut edge_order = vec![first.clone()];
        if self.conditional_edges.is_empty() {
            let linear_next: HashMap<String, String> = self
                .edges
                .iter()
                .filter(|(f, _)| f.as_str() != START)
                .map(|(f, t)| (f.clone(), t.clone()))
                .collect();
            let mut current = first.clone();
            let mut visited = HashSet::new();
            visited.insert(current.clone());
            while let Some(next) = linear_next.get(&current) {
                if next == END {
                    break;
                }
                if visited.contains(next) {
                    return Err(CompilationError::InvalidChain("cycle detected".into()));
                }
                visited.insert(next.clone());
                edge_order.push(next.clone());
                current = next.clone();
            }
        }

        let state_updater = self
            .state_updater
            .unwrap_or_else(|| boxed_updater(ReplaceUpdater));

        Ok(CompiledStateGraph {
            nodes: self.nodes,
            first_node_id: first,
            edge_order,
            next_map,
            checkpointer,
            store: self.store,
            middleware,
            state_updater,
            retry_policy: self.retry_policy,
            interrupt_handler: self.interrupt_handler,
            metadata_extractor: self.metadata_extractor,
        })
    }
}
