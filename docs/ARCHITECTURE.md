# Architecture Design

## Overview

Loom is a Rust-based agent runtime framework that supports multiple execution paradigms (ReAct, Dup, Tot, Got) for building autonomous AI agents.

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI Layer                             │
│  cli/src/main.rs → subcommands.rs → run_flow.rs → repl.rs    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Core Library                             │
│                      loom/src/lib.rs                         │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────┐│
│  │ agent/      │ │ graph/      │ │ cli_run/               ││
│  │ react/      │ │ node.rs     │ │ agent.rs               ││
│  │ got/        │ │ compiled.rs │ │ profile.rs             ││
│  │ dup/        │ │ runtime.rs │ │ mod.rs                 ││
│  │ tot/        │ └─────────────┘ └─────────────────────────┘│
│  └─────────────┘                                            │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────┐│
│  │ llm/        │ │ compress/   │ │ config/                ││
│  │ mod.rs      │ │ compaction  │ │ summary/               ││
│  │ model_*    │ │ context_*   │ └─────────────────────────┘│
│  └─────────────┘ └─────────────┘                            │
│  ┌─────────────┐ ┌─────────────┐                            │
│  │ channels/   │ │ goal_runner/│                            │
│  │ topic.rs    │ │ runner.rs   │                            │
│  │ binop.rs    │ │ state.rs    │                            │
│  └─────────────┘ └─────────────┘                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   External Services                          │
│         LLM Providers │ MCP Servers │ Task DB                │
└─────────────────────────────────────────────────────────────┘
```

## Module Boundaries

### Agent Module (`loom/src/agent/`)

Contains four execution paradigms:

| Module | Paradigm | Description |
|--------|----------|-------------|
| `react/` | ReAct | Think → Act → Observe loop |
| `dup/` | DUP | Adds Understand node before plan/act/observe |
| `got/` | Got | Graph of Tasks with adaptive execution |
| `tot/` | Tot | Tree of Thought with backtracking |

**Problem**: Each agent has its own `runner.rs` with identical `RunError` variants. No shared `trait GraphRunner<S>` interface.

### Graph Module (`loom/src/graph/`)

Core execution engine for state graph evaluation.

- `compiled.rs` — Main execution engine (2091 lines, too large)
- `node.rs` — Node definitions and execution
- `runtime.rs` — Runtime context management
- `state_graph.rs` — State graph compilation

**Problem**: Depends on `cli_run` module (violates dependency direction).

### CLI Run Module (`loom/src/cli_run/`)

High-level CLI orchestration:

- `agent.rs` — `RunOptions` (25 fields), main agent orchestration
- `profile.rs` — Profile resolution (~866 lines)

### LLM Module (`loom/src/llm/`)

LLM provider abstraction:

- `mod.rs` — `LlmProvider` trait
- `model_registry.rs` — Model registry
- `model_cache.rs` — Model caching

## Dependency Issues

### Critical: Circular-like Dependency

```
graph/compiled.rs ──uses──► cli_run/AnyStreamEvent, RunCancellation
                                     │
                                     ▲
                            (cli_run is higher-level)
```

The `graph` module is a low-level abstraction but imports from `cli_run` (a higher-level CLI module). This creates dependency inversion.

**Fix**: Extract shared types to `loom::stream` module:
- `AnyStreamEvent`
- `RunCancellation`
- `StreamMode`

## Key Design Patterns

### 1. Runner Pattern

Each agent implements:
```rust
pub struct ReactRunner { ... }
impl ReactRunner {
    pub fn new(...) -> Result<Self, CompilationError>
    pub fn invoke(&self, state: ReActState) -> Result<TotState, RunError>
    pub fn stream_with_callback(&self, state: ReActState, callback: F) -> Result<TotState, RunError>
    pub fn with_cancellation(self, token: CancellationToken) -> Self
}
```

**Problem**: No shared `trait GraphRunner<S>` for polymorphic dispatch.

### 2. Checkpoint Pattern

State persistence via `Checkpointer<S>` trait:
```rust
pub trait Checkpointer<S>: Send + Sync {
    fn save(&self, state: &S) -> Result<CheckpointId, CheckpointError>;
    fn load(&self, id: CheckpointId) -> Result<S, CheckpointError>;
}
```

Checkpoint logic duplicated in `compiled.rs` (2x identical patterns).

### 3. Stream Event Pattern

Current (verbose):
```rust
if let Some(ctx) = run_ctx {
    if let Some(tx) = &ctx.stream_tx {
        if ctx.stream_mode.contains(&StreamMode::Tasks) {
            let _ = tx.send(StreamEvent::TaskEnd { ... }).await;
        }
    }
}
```

**Problem**: Repeated 7+ times with 5-7 levels of nesting.

## Execution Flow

```
User Input
    │
    ▼
cli/src/main.rs → subcommands.rs::handle()
    │
    ▼
run_flow.rs::build_and_run() → run_cli_turn()
    │
    ├─► build_helve_config()  ── Build HelveConfig
    │
    ├─► profile.rs::resolve()  ── Load profile
    │
    ▼
run_agent_wrapper(agent.rs:177)
    │
    ├─► JSON mode: stream JSON events
    │
    └─► Display mode: format and print to stderr
            │
            ▼
        agent_loop()
            │
            ▼
        ReactRunner::invoke() / ::stream_with_callback()
            │
            ▼
        CompiledStateGraph::run_loop_inner()  (compiled.rs)
            │
            ├─► Node execution with retry
            ├─► Checkpoint save/load
            └─► Stream event emission
```

## State Management

### ReAct State
```rust
pub struct ReActState {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub current_node: NodeId,
}
```

### Checkpoint State Graph
```rust
pub struct CompiledStateGraph<S> {
    nodes: HashMap<NodeId, Box<dyn Node<S>>>,
    edges: HashMap<NodeId, Vec<NextEntry>>,
    state_updater: Arc<dyn StateUpdater<S>>,
    checkpointer: Option<Arc<dyn Checkpointer<S>>>,
}
```

## Configuration Hierarchy

```
RunOptions (25 fields)
    │
    ├─► HelveConfig
    │       └─► system_prompt, context_window, ...
    │
    ├─► ProfileConfig
    │       └─► model, temperature, max_tokens, ...
    │
    └─► RunnableConfig
            └─► max_iterations, timeout, ...
```

## Error Handling

All errors defined with `thiserror`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("stream ended without reaching terminal state")]
    StreamEndedWithoutState,
}
```

**Problem**: Identical error types in 4 separate agent modules.