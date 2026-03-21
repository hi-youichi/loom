# Example: ReAct Linear

The **react_linear** example builds a linear ReAct chain: **think → act → observe → END** with **MockLlm** and **MockToolSource**, one round only.

## What it does

- **ThinkNode** with **MockLlm::with_get_time_call()**: returns one assistant message and one tool call (get_time).
- **ActNode** with **MockToolSource::get_time_example()**: executes get_time and returns a fixed time string.
- **ObserveNode**: Merges tool result into messages, clears tool_calls/tool_results.
- Linear edges: START → think → act → observe → END (no conditional, no loop back to think).

So one user message leads to: think (LLM “output” + tool_calls) → act (tool result) → observe (append result to messages) → end.

## Run

```bash
cargo run -p loom-examples --example react_linear -- "What time is it?"
```

Output: System, User, Assistant, and User (tool result) messages printed.

## Code pattern

1. Build **StateGraph&lt;ReActState&gt;** with **ThinkNode**, **ActNode**, **ObserveNode**.
2. Add edges: **add_edge(START, "think")**, **add_edge("think", "act")**, **add_edge("act", "observe")**, **add_edge("observe", END)**.
3. **compile()** to get **CompiledStateGraph**.
4. Build initial **ReActState** (e.g. system prompt + user message, empty tool_calls/tool_results).
5. **compiled.invoke(state, None).await** and print or use **state.messages**.

## Takeaways

- ReAct in the minimal form: think (LLM + tool_calls) → act (execute tools) → observe (merge results). No conditional “tools vs end” here because the mock always returns one tool call and the graph is linear.
- For a loop (think → act → observe → think until no tool_calls), use conditional edges and a compress node as in **ReactRunner** (see [ReAct Pattern](react-pattern.md)).
