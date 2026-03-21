# ReAct Pattern Implementation

ReAct (Reasoning + Acting) is a loop: **Think** → **Act** → **Observe** → (back to Think or End). Loom implements this as a state graph with three nodes and conditional routing.

## Think → Act → Observe loop

1. **ThinkNode**: Sends current `messages` to the LLM; appends assistant message and optional `tool_calls` to state.
2. **ActNode**: For each `tool_calls` entry, calls `ToolSource::call_tool`; fills `tool_results`.
3. **ObserveNode**: Merges `tool_results` into `messages` (as user messages), clears `tool_calls` and `tool_results`, increments `turn_count`; then routes back to Think or End.

Routing after Think is conditional: if `tool_calls` is non-empty, go to Act; otherwise go to END. After Observe, the graph always goes to a **compress** node (message pruning/compaction when over context limit), then back to Think. So the loop is: **think → (act → observe → compress) → think** until Think returns no tool calls.

## State: ReActState

All three nodes use the same state type **ReActState**:

| Field | Written by | Description |
|-------|------------|-------------|
| `messages` | Think (append assistant), Observe (append tool result messages) | Conversation history |
| `last_reasoning_content` | Think | Optional reasoning/thinking from LLM |
| `tool_calls` | Think | Current round tool invocations from LLM |
| `tool_results` | Act | Results of executing tool_calls |
| `turn_count` | Observe | Number of observe rounds (for max_turns) |
| `approval_result` | Caller (after interrupt) | User approval for pending tool |
| `usage` / `total_usage` | Think | Token usage |

**ToolCall** (name, arguments, id) is produced by Think from LLM output; **ToolResult** (call_id, name, content, is_error) is produced by Act from ToolSource and consumed by Observe.

## StateUpdater and ReAct

The default ReAct graph uses **ReplaceUpdater**: each node returns a full `ReActState` that replaces the current state. Think/Act/Observe each build the full state (messages, tool_calls, tool_results, etc.) so no custom StateUpdater is required for the standard loop. For custom graphs you can use **FieldBasedUpdater** or channel-based updaters to append or aggregate (see [Advanced Patterns](advanced-patterns.md)).

## Building a ReAct runner

### Config-driven: ReactBuildConfig and build_react_runner

Use **ReactBuildConfig** (from env or programmatic) and **build_react_runner** to get a **ReactRunner** with checkpointer, store, tool source, and LLM wired:

```rust
use loom::agent::react::{ReactBuildConfig, build_react_runner};

let config = ReactBuildConfig::from_env()?;  // or build manually
let runner = build_react_runner(&config, None, false, None).await?;
let state = runner.invoke("What is the weather in Paris?").await?;
```

**build_react_run_context** builds only the run context (checkpointer, store, runnable_config, tool_source); you can then construct the LLM and call **ReactRunner::new** yourself for full control.

### Manual: ReactRunner::new

To build the graph yourself:

1. **ThinkNode::new(Arc&lt;dyn LlmClient&gt;)**
2. **ActNode::new(Box&lt;dyn ToolSource&gt;)** — optionally `.with_handle_tool_errors(...)` and `.with_approval_policy(...)`
3. **ObserveNode::with_loop()** (or `with_loop_max_turns(n)`)
4. Optional compression subgraph: **build_graph(CompactionConfig, llm)** → **CompressionGraphNode**
5. **StateGraph**: add nodes think, act, observe, compress; START → think; conditional_edges(think, tools_condition, path_map); act → observe → compress → think
6. **compile** or **compile_with_checkpointer**; wrap in **ReactRunner** with initial state builder **build_react_initial_state**

**tools_condition** is the routing function: `tools_condition(state)` returns `Tools` (go to act) or `End` (go to END) based on `state.tool_calls.is_empty()`.

## Initial state and resume

**build_react_initial_state(user_message, checkpointer, runnable_config, system_prompt)**:

- If checkpointer and runnable_config with thread_id are set, loads the latest checkpoint and resumes (e.g. after interrupt).
- Otherwise builds fresh state: one User message with `user_message`, optional system prompt in messages.

The runner's **invoke** / **invoke_with_config** and **stream_with_config** use this so that with a thread_id you get persistence and resume automatically.

## Integration with LLMs

ThinkNode calls **LlmClient::invoke(&state.messages)**. The LLM returns content and optional tool_calls; Think maps them to **ToolCall** and appends an Assistant message. The **LlmClient** trait supports streaming (Think can stream tokens and tool_call deltas) and usage. See [LLM Integration](../guides/llm-integration.md).

## Error handling and retries

- **ActNode** tool errors: Use **HandleToolErrors** on ActNode:
  - **Never**: errors propagate (graph fails).
  - **Always(template?)**: catch errors and push a ToolResult with `is_error: true` and a message (default template or custom).
  - **Custom(fn)**: your function `(ToolSourceError, name, args) -> String` to format the error message.
- **Graph-level retry**: Attach **RetryPolicy** to the StateGraph (e.g. retry Think or Act on transient failures). ReAct runner does not set this by default; you can build the graph with `with_retry_policy` if you construct it manually.

## Building ReAct from examples

The **loom-examples** crate includes:

- **react_linear**: Minimal ReAct with a simple tool source.
- **react_mcp**: ReAct with MCP tools.
- **react_exa**: ReAct with Exa web search.
- **react_memory**: ReAct with memory tools (remember/recall/search).

Pattern: load **ReactBuildConfig** (or equivalent), call **build_react_runner** or **build_react_run_context**, then **runner.invoke(user_message)** or **run_agent** with **AgentOptions**. For streaming, use **runner.stream_with_config** or **run_react_graph_stream**.

## Summary

| Component | Role |
|-----------|------|
| ThinkNode | LLM call; append assistant message and tool_calls |
| ActNode | Execute tool_calls via ToolSource; fill tool_results; optional approval and error handling |
| ObserveNode | Merge tool_results into messages; clear tool_calls/tool_results; loop back or end |
| tools_condition | Route think → act vs end based on tool_calls |
| ReactRunner | Compiled graph + checkpointer + store + invoke/stream API |
| ReactBuildConfig / build_react_runner | Config-driven construction of ReactRunner |

Next: [LLM Integration](../guides/llm-integration.md) for LlmClient, ChatOpenAI, MockLlm, and streaming.
