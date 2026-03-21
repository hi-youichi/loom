# State Graphs Deep Dive

This document covers building state graphs from scratch, conditional routing, node middleware, checkpointing strategies, and interrupt handling.

## Building a state graph from scratch

### 1. Define state and nodes

State must be `Clone + Send + Sync + Debug + 'static`. Nodes implement `Node<S>`:

```rust
use async_trait::async_trait;
use loom::graph::{Node, Next};
use loom::error::AgentError;

#[derive(Clone, Debug)]
struct MyState { value: i32 }

struct AddNode { id: &'static str, delta: i32 }

#[async_trait]
impl Node<MyState> for AddNode {
    fn id(&self) -> &str { self.id }
    async fn run(&self, state: MyState) -> Result<(MyState, Next), AgentError> {
        Ok((MyState { value: state.value + self.delta }, Next::Continue))
    }
}
```

### 2. Add nodes and edges

Use **START** and **END** for entry and exit. Each node (other than START/END) must be added with `add_node` before you add edges that reference it.

```rust
use loom::graph::{StateGraph, START, END};
use std::sync::Arc;

let mut graph = StateGraph::<MyState>::new();
graph
    .add_node("a", Arc::new(AddNode { id: "a", delta: 1 }))
    .add_node("b", Arc::new(AddNode { id: "b", delta: 2 }));
graph.add_edge(START, "a");
graph.add_edge("a", "b");
graph.add_edge("b", END);
```

### 3. Compile and run

```rust
let compiled = graph.compile().expect("compile");
let initial = MyState { value: 0 };
let final_state = compiled.invoke(initial, None).await?;
// final_state.value == 3
```

To persist state, use a checkpointer and pass a config with `thread_id`:

```rust
let cp = Arc::new(loom::memory::MemorySaver::<MyState>::new());
let compiled = graph.compile_with_checkpointer(cp).expect("compile");
let config = loom::memory::RunnableConfig {
    thread_id: Some("thread-1".into()),
    ..Default::default()
};
let final_state = compiled.invoke(initial, Some(config)).await?;
```

---

## Conditional routing

A node may have **either** a single outgoing edge (**add_edge**) **or** conditional edges (**add_conditional_edges**), not both. With conditional edges, the next node is chosen at runtime from the **updated state** after the node runs.

### Router function

- **path**: `Arc<dyn Fn(&S) -> String + Send + Sync>`. Called with the current state; the return value is either the next node id or a key for the path map.
- **path_map**: Optional. If `Some(map)`, the key from `path` is looked up; the next node is `map[key]` or the key itself if not in the map. If `None`, the key is used directly as the node id.

All targets must be existing node ids or `END`.

### Example: ReAct-style branch

Route after "think": if there are tool calls go to "act", otherwise end.

```rust
use std::collections::HashMap;

graph.add_node("think", Arc::new(think_node));
graph.add_node("act", Arc::new(act_node));
graph.add_edge(START, "think");
graph.add_edge("act", END);

let path_map: HashMap<String, String> = [
    ("tools".into(), "act".into()),
    (END.into(), END.into()),
].into_iter().collect();
graph.add_conditional_edges(
    "think",
    Arc::new(|s: &MyState| {
        if s.has_tool_calls() { "tools".into() } else { END.into() }
    }),
    Some(path_map),
);
```

### Example: no path_map

If you don't need a map (router returns node ids directly):

```rust
graph.add_conditional_edges(
    "decide",
    Arc::new(|s: &MyState| {
        if s.value > 0 { "positive".into() } else { "negative".into() }
    }),
    None,
);
```

---

## Node middleware and hooks

### NodeMiddleware

**NodeMiddleware&lt;S&gt;** wraps every node execution. You get the node id, incoming state, and an `inner` future that runs the actual node. Use it for logging, metrics, retries (or use the graph's **RetryPolicy**), or approval checks.

Set via **with_middleware** before compile, or pass to **compile_with_middleware** / **compile_with_checkpointer_and_middleware**:

```rust
use loom::graph::LoggingNodeMiddleware;

let middleware = Arc::new(LoggingNodeMiddleware::new());
let compiled = graph
    .with_middleware(middleware)
    .compile()
    .expect("compile");
```

Custom middleware:

```rust
#[async_trait]
impl NodeMiddleware<MyState> for MyMiddleware {
    async fn around_run(
        &self,
        node_id: &str,
        state: MyState,
        inner: Box<dyn FnOnce(MyState) -> Pin<Box<dyn Future<Output = Result<(MyState, Next), AgentError>> + Send>> + Send>,
    ) -> Result<(MyState, Next), AgentError> {
        // pre: log, check approval, etc.
        let result = inner(state).await;
        // post: log, metrics
        result
    }
}
```

### Retry policy

Attach a retry policy at graph level (applied to every node run):

```rust
use loom::graph::RetryPolicy;
use std::time::Duration;

graph.with_retry_policy(
    RetryPolicy::fixed(3, Duration::from_millis(100))
);
// or exponential backoff: RetryPolicy::exponential(...)
```

---

## Checkpointing strategies

### When checkpoints are written

- **Normal end**: If the graph was compiled with a checkpointer and `config.thread_id` is set, the final state is saved when the run reaches END or `Next::End`.
- **Interrupt**: If a node returns `AgentError::Interrupted`, the runner saves the current state (with the same conditions), then calls the interrupt handler and returns the error.

### Namespace and thread

- **thread_id**: Identifies the conversation/thread; required for put/get.
- **checkpoint_ns**: Optional namespace within the thread (e.g. `"default"` vs `"compression"`).

Use different namespaces when the same thread runs different subgraphs (e.g. main loop vs compression) and you want separate checkpoint chains.

### Resuming

1. Load the checkpoint: `checkpointer.get_tuple(&config).await` and take the state (and metadata if needed).
2. Optionally set **resume_from_node_id** in config to re-enter at a specific node.
3. Call `invoke(state, Some(config))` or `invoke_with_context(state, run_ctx)`.

Resume is useful after an interrupt: user approves or provides input, you update state (or leave it), then invoke again with the same thread_id and optional resume_from_node_id.

### In-memory vs SQLite

- **MemorySaver**: No disk; lost when process exits. Good for tests and single-process demos.
- **SqliteSaver**: Persistent; use for production so threads survive restarts.

See [Memory & Checkpointing](memory-checkpointing.md) for store layout and rollback.

---

## Interrupt handling and human-in-the-loop

### Raising an interrupt

From a node, return an error:

```rust
use loom::error::AgentError;
use loom::graph::{GraphInterrupt, Interrupt};

Err(AgentError::Interrupted(GraphInterrupt(Interrupt::new(
    serde_json::json!({ "action": "approve", "item_id": "123" })
))))
```

The runner will:

1. Save a checkpoint (if checkpointer + thread_id are set).
2. Call the optional **InterruptHandler**.
3. Return `Err(AgentError::Interrupted(...))` to the caller.

### InterruptHandler

Attach a handler with **with_interrupt_handler**:

```rust
use loom::graph::{InterruptHandler, Interrupt, DefaultInterruptHandler};

// Default: just returns the interrupt value
graph.with_interrupt_handler(Arc::new(DefaultInterruptHandler));

// Custom: e.g. prompt user, log, or enqueue for approval
struct MyHandler;
impl InterruptHandler for MyHandler {
    fn handle_interrupt(&self, interrupt: &Interrupt) -> Result<serde_json::Value, AgentError> {
        // e.g. send to UI, wait for approval, then return updated value
        Ok(interrupt.value.clone())
    }
}
```

The handler's return value is not automatically merged into state; the caller decides how to use it (e.g. update state and resume).

### Human-in-the-loop flow

1. Run the graph with checkpointer and thread_id.
2. If `invoke` returns `Err(AgentError::Interrupted(interrupt))`, present the interrupt to the user (e.g. approval dialog).
3. Optionally load the checkpoint to show or modify state.
4. Update state if the user provided input, then call `invoke` again with the same thread_id (and optional resume_from_node_id) to continue.

---

## Summary

| Topic | Notes |
|-------|--------|
| Building | Add nodes, add edges START→…→END, then compile (or compile_with_checkpointer). |
| Conditional edges | add_conditional_edges(source, path, path_map); path(state) → key; path_map maps key → node id. |
| Middleware | NodeMiddleware wraps each node run; use for logging, approval, or custom retry. |
| Retry | with_retry_policy on the graph; applies to all node executions. |
| Checkpointing | Set checkpointer and config.thread_id; checkpoints on end and on interrupt. |
| Interrupt | Return AgentError::Interrupted; runner checkpoints then calls InterruptHandler and returns error. |

Next: [ReAct Pattern](react-pattern.md) for Think → Act → Observe and `ReactRunner`.
