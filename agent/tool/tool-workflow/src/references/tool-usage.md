# Workflow Tool Usage

Per-action reference for the `workflow` tool. For action selection see
`workflow_skill.md` §1; for Lua DSL primitives see
`references/dsl-reference.md`. Inline-vs-file thresholds
(`REPORT_INLINE_LIMIT=800`, `AGENT_OUTPUT_INLINE_LIMIT=2048`,
`SOURCE_INLINE_LIMIT=32768`) are documented in **Big payloads** below.

---

## `execute` — run a new workflow instance

Starts a workflow, waits for completion, and writes per-instance artifacts
under `.loom/instances/<instance_dir>/` (checkpoint, events,
`workflow.lua`, plus a curated `instance.json` summary).

### Params

| Field         | Type   | Required | Description                                                  |
| ------------- | ------ | -------- | ------------------------------------------------------------ |
| `script`      | string | one-of   | Inline Lua source                                            |
| `workflow`    | string | one-of   | Workflow name or path (resolved via `workflow_resolver`)     |
| `args`        | object | no       | Injected as `_G._args` inside the script                     |
| `concurrency` | int    | no       | Max concurrent agents (`1..=64`, default `4`)               |

Exactly one of `script` / `workflow` is required.

### Return

| Field              | Type    | Description                                                       |
| ------------------ | ------- | ----------------------------------------------------------------- |
| `instance_id`      | string  | Internal run id (from checkpoint)                                 |
| `instance_dir`     | string  | e.g. `"loom-instance_1783783769"`                                 |
| `status`           | string  | `"completed"` \| `"failed"` \| `"cancelled"`                      |
| `workflow`         | object  | `{kind:"file"\|"inline", name, path?}`                            |
| `agent_count`      | int     | Agents that ran                                                   |
| `total_tokens`     | int     | Sum across agents                                                 |
| `total_elapsed_ms` | int     | Sum of agent durations                                            |
| `report_ref`       | string? | Path under `instance_dir` when report exceeded inline limit       |
| `report_preview`   | string? | First 800 chars of serialised report                              |

### Example

```json
{ "workflow": "refactor", "args": { "module": "auth" } }
```

```json
{ "instance_dir": "loom-instance_1783783769", "status": "completed",
  "agent_count": 3, "total_tokens": 41181, "report_preview": "{\"refactored\":3}" }
```

---

## `list-workflows` — available Lua scripts

Lists `.lua` files under `.loom/workflows/`. Read-only; cheap.

### Params

| Field    | Type   | Required | Description        |
| -------- | ------ | -------- | ------------------ |
| `action` | string | yes      | `"list-workflows"` |

### Return

| Field       | Type   | Description                                              |
| ----------- | ------ | -------------------------------------------------------- |
| `workflows` | array  | `{name, size_bytes, modified, preview}` per file         |
| `directory` | string | Resolved absolute path                                   |
| `count`     | int    | Length of `workflows`                                    |

### Example

```json
{ "action": "list-workflows" }
```

```json
{ "workflows": [{ "name": "refactor", "size_bytes": 2410 }], "count": 1 }
```

---

## `list-instances` — past executions

Paginated, filterable. Each entry is a small summary (no event timeline).
Start here when debugging.

### Params

| Field           | Type    | Required | Description                                                  |
| --------------- | ------- | -------- | ------------------------------------------------------------ |
| `limit`         | int     | no       | Page size (`1..=100`, default `20`)                          |
| `cursor`        | string? | no       | From previous page's `next_cursor`                           |
| `status_filter` | string? | no       | `"completed"` \| `"failed"` \| `"cancelled"`                |

### Return

| Field         | Type    | Description                                                  |
| ------------- | ------- | ------------------------------------------------------------ |
| `instances`   | array   | `{instance_id, instance_dir, status, workflow:{kind,name}, created_at, completed_at, total_tokens, agent_count, source}` |
| `count`       | int     | Entries returned                                             |
| `next_cursor` | string? | Pass to next call; `null` when last page                     |

`source` is currently `"current"` for `.loom/instances/`.

### Example

```json
{ "action": "list-instances", "status_filter": "failed", "limit": 5 }
```

```json
{ "instances": [{
    "instance_dir": "loom-instance_1783783500", "status": "failed",
    "workflow": { "kind": "file", "name": "refactor" },
    "agent_count": 3, "source": "current"
  }], "count": 1, "next_cursor": null }
```

---

## `instance-summary` — curated per-instance view

Read this **before** `instance-events`. Status, agent roll-up, phase
spans, event-type counts — **no raw event stream**.

### Params

| Field          | Type   | Required | Description           |
| -------------- | ------ | -------- | --------------------- |
| `instance_dir` | string | yes      | From `list-instances` |

### Return

Full `InstanceMeta` payload (`schema_version: 1`).

| Field             | Type    | Description                                                       |
| ----------------- | ------- | ----------------------------------------------------------------- |
| `status`          | string  | `"completed"` \| `"failed"` \| `"cancelled"`                      |
| `agents`          | array   | `AgentSummary[]` — `status`, `tokens`, `output_preview`, `output_ref?` |
| `phase_spans`     | array   | Replayed from events; nested via `parent_id` / `depth`            |
| `event_stats`     | object  | `{total, by_type}` counts per event type                          |
| `report`          | object  | Inline value, `{ref, preview, ...}` file ref, or `null`           |
| `checkpoint_hash` | string  | sha256 of `checkpoint.json` (audit aid)                           |

### Example

```json
{ "action": "instance-summary", "instance_dir": "loom-instance_1783783769" }
```

```json
{ "status": "failed", "agent_count": 3,
  "agents": [
    { "agent_id": "a1", "status": "ok",    "tokens": 12000 },
    { "agent_id": "a2", "status": "error", "tokens":  8400 }
  ],
  "event_stats": { "total": 312, "by_type": { "agent_done": 3, "run_done": 1 } } }
```

---

## `instance-events` — paginated raw event stream

Last resort. Use after `instance-summary` has narrowed the search.

### Params

| Field          | Type           | Required | Description                                            |
| -------------- | -------------- | -------- | ------------------------------------------------------ |
| `instance_dir` | string         | yes      | From `list-instances`                                  |
| `offset`       | int            | no       | Skip first N matching events (default `0`)             |
| `events_limit` | int            | no       | Page size (`1..=500`, default `50`)                    |
| `types`        | array<string>? | no       | Filter by event type                                   |
| `agent_id`     | string?        | no       | Restrict to one agent                                  |

### Return

| Field            | Type   | Description                                                  |
| ---------------- | ------ | ------------------------------------------------------------ |
| `total_matching` | int    | Full filtered count                                          |
| `next_offset`    | int?   | `offset + returned_count` if more remain, else `null`        |
| `events`         | array  | One parsed JSONL object per event                            |

### Example

```json
{ "action": "instance-events", "instance_dir": "loom-instance_1783783769",
  "types": ["agent_done", "run_done"], "events_limit": 10 }
```

```json
{ "total_matching": 3, "next_offset": null,
  "events": [
    { "type": "agent_done", "agent_id": "a1", "status": "ok",    "tokens": 12000 },
    { "type": "agent_done", "agent_id": "a2", "status": "error", "tokens":  8400 },
    { "type": "run_done",   "status": "failed", "total_tokens": 20400 }
  ] }
```

---

## `instance-source` — the Lua script that ran

Returns the `workflow.lua` copy that the instance executed (snapshot,
not the live file on disk).

### Params

| Field          | Type   | Required | Description           |
| -------------- | ------ | -------- | --------------------- |
| `instance_dir` | string | yes      | From `list-instances` |

### Return

| Field            | Type    | Description                                                |
| ---------------- | ------- | ---------------------------------------------------------- |
| `workflow`       | object  | `{kind:"file"\|"inline", name, path?}`                     |
| `source`         | string? | Full text when ≤ `SOURCE_INLINE_LIMIT` (32 KB); else null  |
| `source_ref`     | string? | Relative path under `instance_dir` when over the limit     |
| `source_preview` | string? | First 4 KB when over the limit                             |

### Example

```json
{ "action": "instance-source", "instance_dir": "loom-instance_1783783769" }
```

```json
{ "workflow": { "kind": "file", "name": "refactor" }, "source": "meta = { ... }", "source_ref": null }
```

---

## Three-step diagnostic flow

When a workflow fails (or behaves oddly), follow this order. It keeps
token cost predictable and gives a structured narrowing path.

1. **`list-instances` with `{"status_filter":"failed"}`** — pick the most
   recent `instance_dir`. If older than the default page, raise `"limit"`
   (up to 100) or follow `next_cursor`.
2. **`instance-summary` with `{"instance_dir": "..."}`** — check three
   things: top-level `status`; `agents[].status` for the failing
   agent(s); `event_stats.by_type` for anomalies (an `error` count, or
   `agent_done` totals that don't match `total_tokens`).
3. **`instance-events` with `{"instance_dir": "...", "types":
   ["agent_done", "run_done", "parallel_done"]}`** — inspect the failure
   points. Widen `types` only if needed (add `"phase_span_*"` for span
   timing, or drop the filter).

Never start at step 3. The summary tells you **which** agent and
**which** event type failed, so you almost never need the full event
stream.

---

## Big payloads

Three thresholds decide inline-vs-file. When a value exceeds its limit,
the inline field is `null` and a `*_ref` points at a file under
`.loom/instances/<instance_dir>/`.

| Constant                    | Limit (bytes) | Field (action)                                                   |
| --------------------------- | ------------- | ---------------------------------------------------------------- |
| `REPORT_INLINE_LIMIT`       | 800           | `report_ref` / `report_preview` (`execute`, `instance-summary`)  |
| `AGENT_OUTPUT_INLINE_LIMIT` | 2 048         | `output_ref` per agent (`instance-summary`)                      |
| `SOURCE_INLINE_LIMIT`       | 32 768        | `source_ref` / `source_preview` (`instance-source`)              |

Fetch the full content with the `read` tool at
`.loom/instances/<instance_dir>/<ref>` — **do not** re-call the workflow
tool, which would only re-run the workflow. Example: if
`summary.agents[1].output_ref` is `"agent-outputs/a2.txt"`, read
`.loom/instances/loom-instance_1783783769/agent-outputs/a2.txt`.
