# Workflow Tool Usage

The workflow surface contains six focused tools. Tool responses are the only interface needed by the caller; do not inspect internal storage or use file-reading tools to follow an execution.

## `workflow_start`

Starts a Lua workflow in the background and returns immediately. It does not wait for agents to finish.

Parameters:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `script` | string | one of `script`/`workflow` | Inline Lua source |
| `workflow` | string | one of `script`/`workflow` | Workflow name or source path |
| `args` | object | no | Values exposed to the script as `_G._args` |
| `concurrency` | integer | no | Maximum concurrent agents, `1..=64`, default `4` |

Returns:

```json
{ "instance_dir": "loom-instance_1783783769", "status": "running" }
```

After this call, run a shell wait before checking status:

```text
sleep 5
```

On PowerShell, use `Start-Sleep -Seconds 5`. Then call `workflow_status` with the returned `instance_dir`. Repeat only while the status is `running`; do not issue the wait and status calls in parallel.

## `workflow_status`

The primary follow-up tool for `workflow_start`. It returns a small running response while execution is active and a complete bounded summary after termination.

Parameters:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `instance_dir` | string | yes | Identifier returned by `workflow_start` or `workflow_list` |

Running response:

```json
{ "instance_dir": "loom-instance_1783783769", "status": "running" }
```

Terminal response includes:

- `status`: `completed`, `failed`, or `cancelled`
- `workflow`: kind and name, without source paths
- `agents`: per-agent status, timing, token usage, and bounded output preview
- `phase_spans`: phase timing
- `event_stats`: event totals grouped by type
- `report`: inline content or a bounded preview, without internal references

## `workflow_list`

Lists completed workflow instances with pagination. Use it when the instance identifier is missing or when selecting a failed execution for investigation. Running executions are not listed; use the identifier returned by `workflow_start` and call `workflow_status`.

Parameters:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `limit` | integer | no | Page size, `1..=100`, default `20` |
| `cursor` | string | no | Cursor returned by the previous page |
| `status_filter` | string | no | `completed`, `failed`, or `cancelled` |

Each entry contains the instance identifier, status, workflow name, timestamps, token totals, and agent count. It does not contain source labels or filesystem paths.

## `workflow_events`

Returns a paginated event stream for detailed investigation after `workflow_status` identifies a problem.

Parameters:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `instance_dir` | string | yes | Workflow instance identifier |
| `offset` | integer | no | Number of matching events to skip, default `0` |
| `events_limit` | integer | no | Page size, `1..=500`, default `50` |
| `types` | array of strings | no | Restrict results to event types |
| `agent_id` | string | no | Restrict results to one agent |

Each event contains its type and event-specific fields. Common types include `agent_started`, `agent_done`, phase span events, and `run_done`. Invalid or blank event lines are skipped. Use pagination and filters instead of requesting the entire stream.

## `workflow_source`

Returns the captured Lua source for an instance as a bounded preview. It returns the content through the tool response and never returns a source reference or storage path.

Parameters:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `instance_dir` | string | yes | Workflow instance identifier |

Returns `instance_dir`, `workflow_source`, and `truncated`. When `truncated` is true, the response contains the available preview only.

## `workflow_files`

Lists available Lua workflow definitions that can be passed to `workflow_start`.

Parameters: none.

Returns:

```json
{
  "workflows": [
    { "name": "refactor.lua", "size_bytes": 1024, "first_line": "meta = {...}" }
  ],
  "count": 1
}
```

## Recommended flows

Start and poll:

```text
workflow_start
→ sleep 5
→ workflow_status
→ repeat while status == "running"
```

Investigate a failure:

```text
workflow_list(status_filter="failed")
→ workflow_status(instance_dir="...")
→ workflow_events(instance_dir="...", types=["agent_done", "run_done"])
```

Inspect the executed source only when the status summary indicates that source review is relevant:

```text
workflow_source(instance_dir="...")
```

Responses do not expose internal source references, output references, report references, absolute paths, or storage filenames. Large values are represented by bounded previews in the tool response.
