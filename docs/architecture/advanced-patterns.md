# Advanced Patterns

This document covers DUP (Dual Use Processing), GoT (Graph of Thoughts), ToT (Tree of Thoughts), and **StateUpdater** strategies (LastValue, Topic, BinaryOperatorAggregate, FieldBasedUpdater).

## DUP (Dual Use Processing)

**DUP** adds an **Understand** step before the plan/act/observe loop: the agent first “understands” the problem, then runs a ReAct-style loop.

- **DupRunner**, **DupState**, **UnderstandNode**: UnderstandNode calls the LLM to produce **UnderstandOutput**; state carries this and then flows into the think/act/observe cycle.
- **build_dup_runner**, **build_dup_initial_state**: Config-driven construction analogous to **build_react_runner**.
- Use when you want an explicit understanding phase before tool use.

## GoT (Graph of Thoughts)

**GoT** runs a DAG of tasks: plan (produce **TaskGraph**: nodes and edges), then execute nodes in topological order, merging results.

- **GotRunner**, **GotState**, **TaskGraph**, **TaskNode**, **TaskNodeState**, **TaskStatus**.
- **PlanGraphNode** produces the task graph from LLM output; **ExecuteGraphNode** runs each node (e.g. via ReAct or a single tool call) and writes **node_states**.
- **build_got_runner**, **build_got_initial_state**, **GotRunnerConfig**: Build and run GoT from config.
- Use for parallel or dependency-ordered sub-tasks.

## ToT (Tree of Thoughts)

**ToT** explores a tree of candidates: extend (generate candidates), evaluate (score), backtrack (prune/select).

- **TotRunner**, **TotState**, **TotCandidate**, **TotExtension**.
- **build_tot_runner**, **build_tot_initial_state**, **TotRunnerConfig**.
- Use for branching exploration with evaluation and backtracking.

## StateUpdater strategies

By default the graph uses **ReplaceUpdater** (node output replaces state). For append or aggregate semantics, use a custom **StateUpdater**:

- **LastValue**: Keep only the last value per “channel” (field or key). Used when each node overwrites a single logical value.
- **Topic**: Broadcast one value to multiple fields (e.g. one message to several lists). See **channels::Topic**.
- **BinaryOperatorAggregate**: Combine values with an operator (e.g. sum, append). See **channels::BinaryOperatorAggregate**.
- **FieldBasedUpdater**: Custom per-field merge: `FieldBasedUpdater::new(|current, update| { ... })`. Use to append messages, merge maps, or implement other domain logic.
- **NamedBarrierValue**, **EphemeralValue**: Synchronization and temporary values; see **channels** module.

Attach with **StateGraph::with_state_updater(Arc::new(updater))** before compile.

## Summary

| Pattern | Key types | Purpose |
|---------|-----------|---------|
| DUP | DupRunner, UnderstandNode, DupState | Understand then ReAct loop |
| GoT | GotRunner, TaskGraph, PlanGraphNode, ExecuteGraphNode | DAG of tasks |
| ToT | TotRunner, TotCandidate, evaluate/backtrack | Tree exploration with scoring |
| StateUpdater | LastValue, Topic, BinaryOperatorAggregate, FieldBasedUpdater | Custom state merge |

Next: [Workspace](../guides/workspace.md) for workspace and thread management.
