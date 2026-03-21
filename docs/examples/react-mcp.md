# Example: ReAct with MCP

The **react_mcp** example runs ReAct with **McpToolSource**: think → act → observe → END, where tools are provided by an MCP server (e.g. filesystem).

## What it does

- **McpToolSource**: Spawns an MCP server process (default: `cargo run -p mcp-filesystem-server --quiet`) or uses env **MCP_SERVER_COMMAND** / **MCP_SERVER_ARGS**. Tools (e.g. list_directory) come from the server’s **tools/list**.
- **ThinkNode** with a **MockLlm** that returns one tool call (list_directory with path).
- **ActNode** with the **McpToolSource**; **call_tool** is forwarded to MCP **tools/call**.
- **ObserveNode** merges the tool result into messages. Linear graph: START → think → act → observe → END.

## Prerequisites

Build the MCP filesystem server:

```bash
cargo build -p mcp-filesystem-server
```

## Run

```bash
cargo run -p loom-examples --example react_mcp
cargo run -p loom-examples --example react_mcp -- "List files in current directory"
```

## Environment

- **MCP_SERVER_COMMAND**: default `cargo`.
- **MCP_SERVER_ARGS**: default `run -p mcp-filesystem-server --quiet`.

To use another MCP server, set both to the desired command and args.

## Code pattern

1. Create **McpToolSource::new(command, args, stderr_verbose)** (or **new_with_env** if the server needs env vars).
2. Build the graph with **ThinkNode**, **ActNode** (with the MCP tool source), **ObserveNode**; same linear edges as react_linear.
3. Build initial **ReActState** (user message, empty tool_calls/tool_results).
4. **compiled.invoke(state, None).await** and print **result.messages**.

## Takeaways

- **McpToolSource** adapts any MCP server (stdio or HTTP) to **ToolSource**; Think gets tool specs from **list_tools**, Act runs **call_tool**.
- For production, replace **MockLlm** with **ChatOpenAI** (or another **LlmClient**) and use **build_react_runner** with config that points to the same MCP server.
