# Memory & Checkpointing

Loom’s memory module provides **Checkpointer** (per-thread state snapshots) and **Store** (cross-session key-value and optional vector search). This document covers implementations, namespaces, and recovery.

## Checkpointer: purpose and trait

**Checkpointer&lt;S&gt;** saves and loads graph state by **(thread_id, checkpoint_ns, checkpoint_id)**:

- **put(config, checkpoint)** — persist state; returns checkpoint id.
- **get_tuple(config)** — load latest checkpoint for the thread (or the one in config.checkpoint_id).
- **list(config)** — list checkpoint ids for the thread (e.g. for time-travel UI).

The graph runner calls **put** when the run ends normally or when an interrupt occurs (if checkpointer and config.thread_id are set). Use **StateGraph::compile_with_checkpointer** to attach a checkpointer.

## Checkpointer implementations

| Type | Persistence | Use case |
|------|-------------|----------|
| **MemorySaver&lt;S&gt;** | In-memory | Dev, tests; lost on process exit |
| **SqliteSaver** | SQLite file | Single-node, production; survives restarts |

**SqliteSaver** requires a **Serializer** (e.g. **JsonSerializer**); state must be `Serialize + DeserializeOwned`. Create with `SqliteSaver::new(path, serializer)`.

## RunnableConfig and namespaces

- **thread_id**: Required for put/get; identifies the conversation/thread.
- **checkpoint_ns**: Optional namespace within the thread (e.g. `"default"` vs `"compression"`). Use when the same thread runs different subgraphs and you want separate checkpoint chains.
- **checkpoint_id**: When loading, which checkpoint to use; omit for “latest”.
- **resume_from_node_id**: When resuming, start from this node instead of the graph’s first node.

## Checkpoint comparison and rollback

- **list(config)** returns checkpoint ids (and metadata when available) so you can implement time-travel or history UI.
- To rollback: load a specific checkpoint with **config.checkpoint_id** set, then **invoke(state, Some(config))** or **invoke_with_context**. The runner does not compare checkpoints; comparison/rollback logic is up to the caller.

## Store: long-term memory

**Store** is for cross-session key-value (and optional vector) storage, isolated by **Namespace** (e.g. `[user_id, "memories"]`).

- **put(namespace, key, value)** — write; **get(namespace, key)** — read.
- **search(namespace, query, options)** — when the backend supports it (e.g. vector store), semantic or filter-based search.
- **list_namespaces(options)** — list namespaces; **batch(ops)** — batch get/put/search/delete.

**Namespace** is `Vec<String>`. Use **RunnableConfig::user_id** (and optionally thread_id) to build namespaces for multi-tenant or per-conversation isolation.

## Store implementations

| Type | Persistence | Search | Feature |
|------|-------------|--------|---------|
| **InMemoryStore** | In-memory | String filter | — |
| **SqliteStore** | SQLite file | String filter | — |
| **SqliteVecStore** | SQLite file | Vector similarity | Requires **Embedder** |
| **LanceStore** | LanceDB | Vector similarity | `lance`; requires Embedder |
| **InMemoryVectorStore** | In-memory | Vector similarity | — |

Vector stores require an **Embedder** (e.g. **OpenAIEmbedder**) for indexing; **search** uses semantic similarity. Memory tools (remember/recall/search_memories/list_memories) use Store; see [Tool System](tool-system.md).

## Namespace organization

- **Checkpoints**: Scope by thread_id and optionally checkpoint_ns. One chain per (thread_id, checkpoint_ns).
- **Store**: Scope by Namespace (e.g. `[user_id, "memories"]`). Use consistent naming so memory tools and other consumers share the same namespace when intended.

## Summary

| Concept | Key types | Purpose |
|---------|-----------|---------|
| Checkpointer | MemorySaver, SqliteSaver, RunnableConfig | Per-thread state snapshots; resume and time-travel |
| Store | InMemoryStore, SqliteStore, LanceStore, Namespace | Long-term key-value and vector search |
| Namespace | Vec&lt;String&gt; | Isolate store data (e.g. per user) |

Next: [Streaming](../guides/streaming.md) for state streaming, StreamMode, and events.
