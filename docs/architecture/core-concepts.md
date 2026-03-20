# Core Concepts

This document covers the main concepts in Loom: state management, graph structure, execution, the tool system, and checkpointing.

## State Management

### State-in, state-out

A single state type `S` flows through the graph. There are no separate Input/Output types:

- Each **node** receives state `S` and returns `(S, Next)`.
- The **graph** applies the node's output to the current state (by default, **replace**), then routes to the next node or ends.

State must be `Clone + Send + Sync + Debug + 'static`. Typical state (e.g. for ReAct) holds messages, tool calls, and tool results.

### Merging node output into state

By default, the graph uses **ReplaceUpdater**: the node's returned state **replaces** the current state. To append or aggregate instead (e.g. append messages), set a custom **StateUpdater** when building the graph:

```rust
use loom::channels::FieldBasedUpdater;

graph.with_state_updater(Arc::new(
    FieldBasedUpdater::new(|current, update| {
        current.messages.extend(update.messages.iter().cloned());
    }),
));
```

See [State Graphs](state-graphs.md) and [Advanced Patterns](advanced-patterns.md) for more on channels and updaters.

### Key types

- **State type `S`**: Your struct (e.g. `ReActState`) used as the single state for the graph.
- **StateUpdater**: Defines how a node's return value is merged into the current state (default: replace).
- **Channels**: Higher-level strategies (LastValue, Topic, BinaryOperatorAggregate, etc.) used to build updaters; see `channels` module.

---

## Graph Structure

### Nodes and edges

- **StateGraph&lt;S&gt;** is the builder. You add **nodes** and **edges**.
- **Node**: Implements `Node<S>` — `id()` and `run(state) -> Result<(S, Next), AgentError>` (or `run_with_context` for streaming/config).
- **Edges**: Define the path. Use constants **START** and **END** for entry and exit:
  - `add_edge(START, "first_node")`
  - `add_edge("last_node", END)`
- A node may have **either** one outgoing `add_edge` **or** `add_conditional_edges`, not both.

### Conditional routing

From a node, you can route by state with `add_conditional_edges(source, path, path_map)`:

- **path**: `Arc<dyn Fn(&S) -> String>` — called with current state; return value is the next node id or a key.
- **path_map**: Optional `HashMap<String, String>` — if provided, the key from `path` is looked up to get the next node id; otherwise the key is used as the node id.

All targets must be valid node ids or `END`.

### Compilation

- **compile()** → `CompiledStateGraph<S>`: immutable graph, no checkpointer.
- **compile_with_checkpointer(checkpointer)** / **compile_with_middleware** / **compile_with_checkpointer_and_middleware**: variants that attach a checkpointer and/or node middleware.

After compilation you only **invoke** or **stream**; you cannot add nodes or edges.

### Key types

- **StateGraph&lt;S&gt;** — builder: nodes, edges, conditional edges, state updater, middleware, retry, interrupt handler.
- **CompiledStateGraph&lt;S&gt;** — immutable executable graph.
- **Node&lt;S&gt;** — trait: `id()`, `run(state)` → `(S, Next)`.
- **Next** — `Continue` (follow edge order), `Node(id)` (jump), or `End` (stop).

---

## Execution Model

### RunContext

**RunContext&lt;S&gt;** is passed into nodes (when using `run_with_context` or when the runner builds it). It holds:

- **config**: `RunnableConfig` — thread_id, user_id, checkpoint_ns, resume_from_node_id, etc.
- **stream_tx** / **stream_mode**: For streaming events (Values, Updates, Messages, Checkpoints, Tasks, Debug).
- **store**: Optional long-term store (e.g. for memory tools).
- **previous**: Previous state when resuming from a checkpoint.
- **runtime_context**: Custom JSON for user_id, db_conn, etc.
- **managed_values**: e.g. `IsLastStep` — runtime info for nodes.

You can build a context and call **invoke_with_context** instead of **invoke** to pass store, previous state, or runtime context.

### Runtime

**Runtime** is a legacy/supplementary bundle of context, store, stream writer, and previous state. Execution is driven by **CompiledStateGraph** and **RunContext**; Runtime is used where a single context object is convenient.

### Stepping and invocation

- **invoke(state, config)**  
  Runs the graph from the first node (or from `config.resume_from_node_id` if set and valid). After each node:
  - The node's returned state is merged (via StateUpdater).
  - Next node is chosen by **conditional router** (if present) or by the node's **Next** (Continue / Node(id) / End).
  - If a checkpointer is set and `config.thread_id` is present, the final state is checkpointed on normal end; on **interrupt**, state is checkpointed then the interrupt is propagated.

- **stream(state, config, stream_mode)**  
  Same run loop, but events are sent on a channel: Values, Updates, TaskStart/TaskEnd, Checkpoints (if enabled and checkpointer present), etc.

### Interruption

A node can return `Err(AgentError::Interrupted(...))`. The runner then:

1. If checkpointer and thread_id are set, saves a checkpoint with the current state.
2. Calls the optional **InterruptHandler**.
3. Returns the interrupt error to the caller.

Use this for human-in-the-loop (e.g. approval steps). See [State Graphs](state-graphs.md) for interrupt handling.

### Key types

- **RunContext&lt;S&gt;** — config, stream, store, previous, runtime_context, managed_values.
- **Runtime** — optional bundle of context, store, stream writer, previous.
- **RunnableConfig** — thread_id, user_id, checkpoint_ns, checkpoint_id, resume_from_node_id.
- **CompiledStateGraph::invoke** / **invoke_with_context** / **stream**.

---

## Tool System

### ToolSource and ToolSpec

Agents (e.g. ReAct) depend on **ToolSource**, not a fixed registry:

- **list_tools()** → `Vec<ToolSpec>` — name, description, input_schema (JSON Schema). Used by the Think node to build prompts.
- **call_tool(name, arguments)** → `ToolCallContent` — executes the tool; Act node maps this to `ToolResult` and writes into state.
- **call_tool_with_context(name, arguments, ctx)** — optional per-step context (e.g. recent messages); default delegates to `call_tool`.
- **set_call_context(ctx)** — inject context before a round of tool calls (e.g. ActNode sets current messages).

### Implementations

- **MockToolSource** — for tests.
- **StoreToolSource** — remember / recall / search_memories / list_memories (long-term store).
- **ShortTermMemoryToolSource** — get_recent_messages (current conversation).
- **MemoryToolsSource** — composite of store + short-term memory tools.
- **WebToolsSource** — web_fetcher (HTTP).
- **BashToolsSource** — bash (shell); use with care and approval policies where needed.
- **McpToolSource** — MCP tools (list/call via MCP).

### Key types

- **ToolSource** — trait: list_tools, call_tool, call_tool_with_context, set_call_context.
- **ToolSpec** — name, description, input_schema.
- **ToolCallContent** — text result from a tool call.
- **ToolCallContext** — per-step context (e.g. recent_messages) passed to tools that need it.

See [Tool System](tool-system.md) for details and security considerations.

---

## Checkpointing

### Purpose

Checkpointing saves and restores graph state so you can:

- Resume a run after an interrupt or crash.
- Implement time-travel or history (if the checkpointer supports list/get by id).

### Checkpointer trait

**Checkpointer&lt;S&gt;**:

- **put(config, checkpoint)** — persist state; returns checkpoint id.
- **get_tuple(config)** — load latest checkpoint (or the one in config.checkpoint_id) for the thread.
- **list(config)** — list checkpoint ids for the thread (optional).

Checkpoints are scoped by **thread_id** (and optionally **checkpoint_ns**). The graph runner calls **put** when:

- The run ends normally (with checkpointer and config.thread_id set).
- An interrupt occurs (state is saved before propagating the error).

### RunnableConfig

- **thread_id** — required for checkpoint put/get (identifies the conversation/thread).
- **checkpoint_ns** — optional namespace within the thread.
- **checkpoint_id** — optional; when loading, which checkpoint to use (otherwise "latest").
- **resume_from_node_id** — optional; when resuming, start from this node instead of the graph's first node.

### Implementations

- **MemorySaver&lt;S&gt;** — in-memory; good for tests and single-process runs.
- **SqliteSaver** — persistent SQLite; for production persistence.

### Namespace and recovery

Use **checkpoint_ns** to separate different flows (e.g. "default" vs "compression") in the same thread. To resume:

1. Load checkpoint: `checkpointer.get_tuple(&config).await` and get state (and metadata if needed).
2. Build config with `resume_from_node_id` if you want to re-enter at a specific node.
3. Call `invoke(state, Some(config))` or `invoke_with_context(state, run_ctx)`.

See [Memory & Checkpointing](memory-checkpointing.md) for persistence, namespaces, and rollback.

---

## Summary

| Concept | Key types | Purpose |
|--------|------------|---------|
| State | `S`, StateUpdater, Channels | Single state type; how node output is merged |
| Graph | StateGraph, CompiledStateGraph, Node, Next | Build and run directed graphs with conditional routing |
| Execution | RunContext, Runtime, invoke, stream | Run loop, config, streaming, interrupts |
| Tools | ToolSource, ToolSpec, ToolCallContent | List and call tools; used by ReAct Act/Think |
| Checkpointing | Checkpointer, MemorySaver, SqliteSaver, RunnableConfig | Persist and resume state by thread/namespace |

Next: [State Graphs](state-graphs.md) for building graphs, conditional edges, middleware, and interrupt handling.
