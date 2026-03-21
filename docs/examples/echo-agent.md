# Example: Echo Agent

The **echo** example demonstrates the minimal **Agent** trait: state-in, state-out with a single step.

## What it does

- **AgentState**: Holds `messages: Vec<Message>`.
- **EchoAgent**: If the last message is `User(s)`, appends `Message::Assistant(s)`; otherwise leaves messages unchanged.
- No graph, no tools, no LLM — just one agent run.

## Run

```bash
cargo run -p loom-examples --example echo -- "Hello"
```

Output: the same string (e.g. `Hello`).

## Code pattern

1. Define state: `struct AgentState { messages: Vec<Message> }`.
2. Implement **Agent**: `name()`, `type State = AgentState`, `run(state) -> Result<State, AgentError>`.
3. Build initial state (e.g. one `Message::User(input)`), call **agent.run(state).await**, then read the last message from the result.

## Takeaways

- **Agent** is the minimal interface for a single-step, state-in/state-out handler.
- To compose multiple steps or add routing, use **StateGraph** and **Node** (see [State Graphs](state-graphs.md) and [react-linear](react-linear.md)).
