# Tool System Architecture

The tool system lets ReAct (and other agents) list and execute tools. Think gets tool descriptions for the LLM; Act executes tool calls and writes results into state. This document covers the **ToolSource** and **ToolSpec** patterns, built-in sources (MCP, web, bash, memory, store), and security considerations.

## ToolSource and ToolSpec

### ToolSource trait

Agents depend on **ToolSource**, not a concrete registry:

- **list_tools()** → `Vec<ToolSpec>`: name, description, input_schema (JSON Schema). Used by ThinkNode to build prompts and by LLM clients (e.g. ChatOpenAI) when tools are set.
- **call_tool(name, arguments)** → **ToolCallContent**: runs the tool; ActNode maps this to **ToolResult** and appends to state.
- **call_tool_with_context(name, arguments, ctx)**: optional per-step **ToolCallContext** (e.g. recent_messages). Default implementation ignores `ctx` and calls `call_tool`.
- **set_call_context(ctx)**: inject context before a round of tool calls. ActNode calls this with current messages (and optional stream_writer, thread_id, user_id); tools that need context (e.g. get_recent_messages) override and use it in `call_tool_with_context`.

### ToolSpec

- **name**: Tool identifier (e.g. used in MCP tools/call).
- **description**: Human-readable description for the LLM.
- **input_schema**: JSON Schema for arguments (MCP inputSchema).

### ToolCallContent and ToolCallContext

- **ToolCallContent**: `text` — result content (e.g. from MCP result.content[].text). ActNode wraps it in **ToolResult** (call_id, name, content, is_error).
- **ToolCallContext**: Per-step data set by ActNode: **recent_messages**, optional **stream_writer** (ToolStreamWriter for custom events), **thread_id**, **user_id**, **depth** (for nested agents). Tools that need current conversation use `call_tool_with_context` and read `ctx.recent_messages`.

## MCP tool integration

**McpToolSource** connects to an MCP server and implements ToolSource via `tools/list` and `tools/call`.

- **Stdio**: `McpToolSource::new(command, args, stderr_verbose)` or `new_with_env(..., env, ...)` — spawns the server process; use for local MCP servers.
- **HTTP**: `McpToolSource::new_http(url, ...)` — use when the server is exposed over HTTP (e.g. Exa at https://mcp.exa.ai/mcp).

Tool names and argument schemas come from the server; call results are mapped to **ToolCallContent**. Used by ActNode and by examples that pass tools to ChatOpenAI. See **register_mcp_tools** and **McpToolAdapter** in the `tools` module for registration helpers.

## Web tool implementation

**WebToolsSource** exposes one tool: **web_fetcher** (HTTP GET/POST). Built with `WebToolsSource::new()` or `WebToolsSource::new_with_client(client)` for custom timeouts/proxies. Use with ActNode when the agent needs to fetch URLs or call HTTP APIs.

## Bash tool security considerations

**BashToolsSource** exposes one tool: **bash** — runs shell commands. Because it executes arbitrary commands:

- **Restrict access**: Enable only when necessary; prefer MCP or web tools for constrained operations.
- **Approval**: Use **ApprovalPolicy** (e.g. **tools_requiring_approval**) so sensitive tools (e.g. bash) require user approval before execution; ActNode can raise an interrupt for approval and resume with **approval_result**.
- **Sandboxing**: Loom does not sandbox shell execution; run the process in a restricted environment (e.g. container, restricted user) if you need isolation.
- **Dry run**: **DryRunToolSource** wraps another ToolSource and logs/returns placeholder results without executing; use for testing or "what would run" flows.

## Memory tools (remember / recall / search / list_memories)

- **StoreToolSource**: Exposes **remember**, **recall**, **search_memories**, **list_memories** against an **Arc&lt;dyn Store&gt;** and a fixed **Namespace**. Use with ActNode for long-term key-value memory.
- **ShortTermMemoryToolSource**: One tool **get_recent_messages** — returns the current conversation (from **ToolCallContext.recent_messages**). Use when the agent needs to re-read or summarize recent messages; most flows can omit it.
- **MemoryToolsSource**: Composite of Store + short-term: all five tools (remember, recall, search_memories, list_memories, get_recent_messages). Build with `MemoryToolsSource::new(store, namespace).await` and pass to ActNode for one-line setup.

ActNode sets **ToolCallContext** (including recent_messages) via **set_call_context** before executing tools, so get_recent_messages receives the current state.

## Store tool operations

Store-backed tools (StoreToolSource / MemoryToolsSource) use **Store** for persistence:

- **remember**: Put key-value (and optional metadata) in the store under the namespace.
- **recall**: Get value by key.
- **search_memories**: Query by text (when Store supports search, e.g. vector store).
- **list_memories**: List keys in the namespace.

Namespace typically includes **user_id** (and optionally thread_id) so storage is per-user or per-conversation. **RunnableConfig::user_id** is passed through RunContext and can be used to build the namespace when constructing the tool source.

## Combining sources (AggregateToolSource)

Multiple tools can be combined via **AggregateToolSource**: register sync/async tools, then use the aggregate as the single ToolSource for ActNode. **MemoryToolsSource**, **StoreToolSource**, **WebToolsSource**, and **BashToolsSource** use it internally. For custom compositions, build an **AggregateToolSource**, register each tool or sub-source, and pass the aggregate to ActNode.

## Dry run and testing

- **DryRunToolSource**: Wraps a ToolSource; **list_tools** delegates, **call_tool** does not call the inner tool — returns a placeholder result (or logs and returns empty). Use for testing or to see which tools would be called without executing them.
- **MockToolSource**: For unit tests; implement or mock **list_tools** and **call_tool** to return fixed specs and results.

## Summary

| Component | Purpose |
|-----------|---------|
| ToolSource | list_tools, call_tool, call_tool_with_context, set_call_context |
| ToolSpec | name, description, input_schema |
| ToolCallContext | recent_messages, stream_writer, thread_id, user_id, depth |
| McpToolSource | MCP server via stdio or HTTP |
| WebToolsSource | web_fetcher (HTTP GET/POST) |
| BashToolsSource | bash (shell) — use with approval/sandboxing |
| StoreToolSource | remember, recall, search_memories, list_memories |
| MemoryToolsSource | Store tools + get_recent_messages |
| DryRunToolSource | No-op execution for testing |

Next: [Memory & Checkpointing](memory-checkpointing.md) for checkpointer and store backends.
