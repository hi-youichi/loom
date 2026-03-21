# Compression & Memory Management

When conversation history exceeds the model’s context window, Loom can prune and compact messages via a **compression subgraph**. This document covers the compression workflow, pruning, compaction, and memory limits.

## Compression subgraph workflow

The ReAct runner inserts a **compress** node after **observe**: **observe → compress → think**. The compress node runs a subgraph:

- **PruneNode**: Removes or truncates old messages when over a token (or message) limit (see **CompactionConfig**).
- **CompactNode**: Optionally merges consecutive messages into summaries using the LLM (e.g. turn a long stretch of tool results into one summary message).
- Flow: **prune → compact → END**; the subgraph is built by **compress::build_graph(CompactionConfig, llm)** and wrapped as **CompressionGraphNode**.

So one “compress” step runs: prune then compact on the current state; the result is passed back to the main graph (then to think).

## Message pruning strategies

- **PruneNode** uses **CompactionConfig** (e.g. **max_tokens** or **max_messages**). When the current messages exceed the limit, it removes or truncates from the beginning (or according to config) so that the remaining context fits.
- Pruning is in-memory for that run; the checkpointer stores the state after compress (so the next run resumes with already-pruned state if loading from checkpoint).

## Message compaction algorithms

- **CompactNode** uses the LLM to summarize a range of messages (e.g. several user/assistant/tool turns) into one or fewer messages. This reduces token count while preserving semantic content. The exact algorithm (which range to compact, how many summaries) is defined in the **compact** module (e.g. **CompactionConfig** and the node implementation).

## Memory limits and cleanup

- **CompactionConfig** carries limits (e.g. **with_max_context_tokens**). The build pipeline (e.g. **build_react_runner**) can infer a default from **models.dev** when the model id is in “provider/model” form; otherwise defaults are used.
- There is no automatic “cleanup” of old checkpoints or store entries in the core library; applications can call **checkpointer.list** and delete old checkpoints, or prune store namespaces, as needed.

## Summary

| Topic | Notes |
|-------|--------|
| Subgraph | prune → compact → END; wrapped as CompressionGraphNode |
| Prune | PruneNode; remove/truncate messages over limit (CompactionConfig) |
| Compact | CompactNode; LLM summarizes message ranges |
| Limits | CompactionConfig (max_tokens / max_messages); optional models.dev resolution |

Next: [Configuration](../guides/configuration.md) for HelveConfig and profiles.
