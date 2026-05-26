# Module Overview

## Project Structure

```
telegram/                    # Git worktree root
├── loom/                    # Core library (Rust crate)
├── cli/                     # CLI application
├── bot-runtime/            # Telegram bot runtime
└── docs/                   # Documentation
```

---

## loom/ — Core Library

### loom/src/lib.rs

**Purpose**: Public API surface for the `loom` crate.

**Key Exports**:
- Agent runners: `ReactRunner`, `GotRunner`, `DupRunner`, `TotRunner`
- Graph types: `CompiledStateGraph`, `Node`, `Next`
- LLM types: `LlmProvider`, `ModelConfig`
- State types: `ReActState`, `TotState`, `DupState`, `GotState`

**Issue**: 72 lines of flat re-exports (see REVIEW_FINDINGS.md M3)

---

### loom/src/agent/ — Agent Paradigms

#### react/ — ReAct (Think-Act-Observe)

**Entry**: `runner/runner.rs`

**State**: `ReActState`
```rust
pub struct ReActState {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub current_node: NodeId,
}
```

**Nodes**:
- `ThinkNode` — Generate reasoning
- `ActNode` — Execute tools or respond
- `ObserveNode` — Process tool results
- `TitleNode` — Generate conversation title
- `WithNodeLogging` — Logging middleware

**Files**:
```
agent/react/
├── mod.rs
├── runner/
│   ├── mod.rs
│   ├── runner.rs      # ReactRunner implementation
│   ├── error.rs       # RunError enum
│   ├── initial_state.rs
│   └── options.rs
├── think_node.rs
├── act_node.rs
├── observe_node.rs
├── title_node.rs
├── agent_tool.rs
├── config.rs
└── with_node_logging.rs
```

---

#### got/ — Graph of Tasks (Adaptive)

**Entry**: `runner.rs`

**State**: `GotState`
```rust
pub struct GotState {
    pub graph: PlanGraph,
    pub execution_status: ExecutionStatus,
    pub current_node: Option<NodeId>,
}
```

**Features**:
- Adaptive execution with `AdaptiveRunner`
- Plan node graph construction
- Parallel task execution

**Files**:
```
agent/got/
├── mod.rs
├── runner.rs          # GotRunner implementation
├── plan_node.rs
├── dag.rs
├── execute_engine.rs
├── prompt.rs
├── state.rs
└── adaptive.rs
```

---

#### dup/ — Deeply Understanding Problems

**Entry**: `runner.rs`

**State**: `DupState`
```rust
pub struct DupState {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub current_node: NodeId,
}
```

**Difference from ReAct**: Adds `UnderstandNode` before the main loop.

**Files**:
```
agent/dup/
├── mod.rs
├── runner.rs
├── adapter_nodes.rs
├── understand_node.rs
├── prompt.rs
└── state.rs
```

---

#### tot/ — Tree of Thought

**Entry**: `runner.rs`

**State**: `TotState`
```rust
pub struct TotState {
    pub messages: Vec<Message>,
    pub branches: Vec<ThoughtBranch>,
    pub current_branch: Option<usize>,
}
```

**Features**:
- Branch exploration
- Backtracking via `BacktrackNode`
- Branch evaluation

**Files**:
```
agent/tot/
├── mod.rs
├── runner.rs
├── adapter_nodes.rs
├── backtrack_node.rs
├── evaluate_node.rs
├── expand_node.rs
├── prompt.rs
└── state.rs
```

---

### loom/src/graph/ — Execution Engine

**Purpose**: Generic state graph execution runtime.

**Core Types**:
- `CompiledStateGraph<S>` — Compiled graph ready for execution
- `Node<S>` — Trait for graph nodes
- `Next` — Node traversal decisions
- `StreamMode` — Event streaming configuration

**Files**:
```
graph/
├── mod.rs
├── node.rs           # Node trait
├── compiled.rs       # Main execution engine (2091 lines)
├── runtime.rs       # Runtime context
├── state_graph.rs   # Graph compilation
├── conditional.rs   # Conditional node logic
├── retry.rs         # Retry middleware
├── interrupt.rs     # Interrupt handling
├── logging.rs       # Logging middleware
├── name_node.rs     # Named node wrapper
├── next.rs          # Next enum
├── visualization.rs # Debug visualization
├── cancellable.rs   # Cancellation support
└── node_middleware.rs
```

**Key Trait**:
```rust
pub trait Node<S>: Send + Sync {
    async fn execute(&self, state: &mut S, ctx: &RunContext<S>) -> Result<Next, NodeError>;
}
```

---

### loom/src/cli_run/ — CLI Integration

**Purpose**: High-level CLI orchestration and configuration.

**Files**:
```
cli_run/
├── mod.rs
├── agent.rs          # RunOptions, main orchestration
└── profile.rs        # Profile resolution (~866 lines)
```

**Key Types**:
- `RunOptions` — 25-field configuration struct
- `ProfileConfig` — Named profile with model/temperature settings
- `HelveConfig` — Helve prompt configuration

---

### loom/src/llm/ — LLM Provider Abstraction

**Purpose**: Abstract over different LLM backends.

**Files**:
```
llm/
├── mod.rs            # LlmProvider trait
├── model_registry.rs # Model registry
├── model_cache.rs   # Model caching
├── fixed_provider.rs # Fixed model provider
└── mock.rs           # Mock for testing
```

**Key Trait**:
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, LlmError>;
    async fn stream(&self, request: &CompletionRequest, callback: StreamCallback) -> Result<(), LlmError>;
}
```

---

### loom/src/compress/ — Context Compression

**Purpose**: Manage context windows and compaction.

**Files**:
```
compress/
├── mod.rs
├── compaction.rs    # Compaction logic
├── context_window.rs # Window management
├── config.rs        # CompactionConfig
├── compact_node.rs  # Compaction node
├── prune_node.rs    # Pruning node
└── graph.rs         # Compression graph
```

---

### loom/src/goal_runner/ — Autonomous Goal Execution

**Purpose**: Run autonomous goal loops with task persistence.

**Files**:
```
goal_runner/
├── runner.rs        # GoalRunner implementation
├── state.rs        # Goal state management
├── message.rs      # Message handling
├── tool.rs         # Tool abstraction
└── tests.rs        # Unit tests
```

**Key Types**:
- `GoalRunner` — Main orchestrator
- `GoalOutcome` — `Achieved | Error(String)`
- `ToolError` — `Aborted | Timeout | ExecutionFailed`

---

### loom/src/channels/ — Inter-task Communication

**Purpose**: Async channels for task coordination.

**Files**:
```
channels/
├── mod.rs
├── topic.rs         # Topic-based pub/sub
├── binop.rs         # Binary operation channels
├── last_value.rs    # Last-value caching channel
├── named_barrier.rs # Named barriers
├── ephemeral_value.rs
├── updater.rs       # State update channel
└── error.rs
```

---

### loom/src/config/ — Configuration Summary

**Purpose**: Display-oriented configuration types.

**Files**:
```
config/
├── mod.rs           # ConfigSection trait
└── summary/
    ├── embedding.rs # Embedding config summary
    ├── llm.rs      # LLM config summary
    ├── memory.rs   # Memory config summary
    ├── mod.rs
    └── tools.rs    # Tools config summary
```

**Note**: Renaming suggested to `config_summary/` for accuracy.

---

## cli/ — Command-Line Interface

### cli/src/main.rs

**Purpose**: CLI entry point with subcommand dispatch.

**Subcommands**:
- `react`, `dup`, `tot`, `got` — Run respective agents
- `serve` — Start server mode
- `tool` — Tool management
- `session` — Session management
- `models` — List available models
- `mcp` — MCP server management
- `agent` — Agent management
- `goal` — Goal runner commands
- `skills` — Skills management
- `evolve` — Evolution triggers
- `curator` — Content curation
- `memory` — Memory management
- `review-skill` — Skill review
- `review` — Code review
- `task` — Task management

---

### cli/src/run/ — Execution Runtime

**Purpose**: Core agent execution loop and display formatting.

**Files**:
```
run/
├── mod.rs              # Module exports
├── agent.rs           # Main agent orchestration (1334 lines)
├── display.rs         # Terminal display formatting
├── panel_format.rs     # Panel line formatting (colors, truncation)
├── repl.rs             # REPL loop
├── run_flow.rs        # Run flow construction
├── spinner.rs         # Spinner display
├── memory.rs          # Memory integration
├── security.rs        # Security checks
├── session_store.rs   # Session persistence
├── skill_registry.rs  # Skill registry
├── observability.rs   # Observability hooks
├── background_review.rs # Async review
├── review/             # Review sub-system
│   ├── agent_loop.rs
│   ├── prompts.rs
│   └── tools.rs
└── event_handlers/     # (Proposed reorganization)
```

**Key Functions**:
- `run_agent_wrapper` — Orchestrates agent execution
- `log_tools_used` — Logs tool calls with panel formatting
- `format_panel_line` — Formats stderr output lines

---

### cli/src/review_cmd.rs

**Purpose**: CLI `review` subcommand implementation.

**Entry**: `handle_review_command()`

---

## bot-runtime/ — Telegram Bot Runtime

**Purpose**: Runtime for hosting multiple Telegram bot instances.

**Bots**:
- `assistant` — General purpose assistant
- `crypto-dev-bot` — Crypto development bot
- `dev-bot` — Development bot
- `loom-dev-bot` — Loom development bot
- `mcp-dev-bot` — MCP development bot
- `twitter-bot` — Twitter integration bot

**Structure** (per bot):
```
bots/{bot-name}/
├── bot.toml           # Bot configuration
├── config.toml        # Runtime configuration
└── .env               # Environment variables
```

---

## Module Dependencies

```
                    ┌─────────────┐
                    │    lib.rs   │
                    └──────┬──────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
        ▼                  ▼                  ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│    agent/     │  │    graph/    │  │   cli_run/    │
│ react/got/   │  │  compiled.rs │◄─┤  agent.rs     │
│ dup/tot/     │  │  node.rs     │  │  profile.rs  │
└───────────────┘  └───────┬───────┘  └───────────────┘
                           │                  ▲
                           │ uses            │ uses
                           ▼                  │
                    ┌───────────────┐        │
                    │  stream/      │◄───────┘
                    │(proposed)     │
                    └───────────────┘
```

---

## Public API Surface

### loom crate public items:
```rust
// Core types
pub use crate::agent::{ReactRunner, GotRunner, DupRunner, TotRunner};
pub use crate::graph::{CompiledStateGraph, Node, Next, StreamMode};
pub use crate::llm::{LlmProvider, ModelConfig};
pub use crate::cli_run::{RunOptions, RunCmd, HelveConfig};

// State types
pub use crate::agent::react::ReActState;
pub use crate::agent::tot::TotState;
pub use crate::agent::dup::DupState;
pub use crate::agent::got::GotState;

// Tool types
pub use crate::agent::ToolCall;
pub use crate::agent::ToolResult;

// Error types
pub use crate::error::{RunError, CompilationError, LlmError};
```

---

## Feature Flags

Current features (in `Cargo.toml`):
- `default` — Core functionality
- `llm-openai` — OpenAI provider
- `llm-anthropic` — Anthropic provider
- `compress` — Context compression
- `goal` — Goal runner

**Suggested**: Add feature flags for:
- `cli` — CLI-specific run types
- `stream` — Streaming support