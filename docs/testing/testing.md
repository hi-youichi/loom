# Testing & Examples

This document summarizes testing strategies, mock implementations, example workflows, and troubleshooting for Loom.

## Testing strategies

- **Unit tests**: Test individual nodes (ThinkNode, ActNode, ObserveNode) with **MockLlm** and **MockToolSource**; assert on state and Next.
- **Graph tests**: Build a small **StateGraph**, compile, **invoke** or **stream** with fixed state; assert final state or event sequence. Use **MemorySaver** for checkpoint tests.
- **Integration tests**: Use **build_react_runner** with test config (e.g. MockLlm, in-memory store); run one or a few turns and assert messages/tool_results.
- **Stream tests**: Enable **StreamMode::Values** or **Updates**, collect **StreamEvent** from **stream()**, assert order and content.

## Mock implementations

- **MockLlm**: **with_get_time_call()**, **with_no_tool_calls(content)**; optional stateful (first call tool_calls, second none) and **stream_by_char** for stream tests.
- **MockToolSource**: **get_time_example()** or custom; **list_tools** and **call_tool** return fixed specs and results.
- **MemorySaver&lt;S&gt;** / **InMemoryStore**: No disk; use for checkpoint and store in tests so runs are fast and isolated.

## Example workflows

- **loom-examples** crate: **echo** (Agent trait), **react_linear** (Think/Act/Observe with mocks), **react_mcp** (MCP tools), **react_exa**, **react_memory**, **memory_checkpoint**, **memory_persistence**, **state_graph_echo**, **openai_embedding**. Run with `cargo run -p loom-examples --example <name> -- [args]`.
- See [Echo Agent](../examples/echo-agent.md), [ReAct Linear](../examples/react-linear.md), [ReAct MCP](../examples/react-mcp.md) for walkthroughs.

## Best practices

- Prefer **MockLlm** and **MockToolSource** so tests don’t call real APIs.
- Use **RunnableConfig** with a distinct **thread_id** per test when using a shared **MemorySaver** to avoid cross-test state.
- For conditional edges, test both branches (e.g. tool_calls present vs empty) with different initial state.
- When testing interrupts, assert that **invoke** returns **AgentError::Interrupted** and (if checkpointer is set) that a checkpoint was saved.

## Troubleshooting guide

- **CompilationError**: Check that every node id in edges and conditional path_map is added with **add_node**; that a node has either one outgoing edge or conditional_edges, not both.
- **Empty graph**: Ensure at least one node and edges from START to that node and from some node to END.
- **Tool not found**: Ensure **ToolSource::list_tools** includes the tool name the LLM returns; for MCP, ensure the server is running and **McpToolSource** is initialized.
- **Checkpoint not saved**: Require **config.thread_id** and a checkpointer attached via **compile_with_checkpointer**.
- **Stream no events**: Ensure **StreamMode** includes the variant you expect (e.g. Values, Updates) and that you consume the stream (e.g. collect or for_each).

## Summary

| Topic | Notes |
|-------|--------|
| Strategies | Unit (mocks), graph (invoke/stream), integration (runner) |
| Mocks | MockLlm, MockToolSource, MemorySaver, InMemoryStore |
| Examples | loom-examples (echo, react_linear, react_mcp, …) |
| Practices | Isolate thread_id, test both branches, assert interrupts/checkpoints |
| Troubleshooting | CompilationError, empty graph, tool not found, checkpoint, stream |

Next: See [Overview](../README.md) for the full documentation map.
