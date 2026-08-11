---
name: workflow
description: Lua DSL reference for writing multi-agent workflows
triggers:
  - workflow
  - multi-agent
  - lua script
  - workflow_list
  - workflow_status
  - workflow_cancel
  - workflow_events
  - debug workflow
  - workflow failed
  - workflow status
  - resume workflow
  - crashed instance
  - stop workflow
metadata:
  conditions:
    requires_tools:
      - workflow_start
      - workflow_status
      - workflow_cancel
tags:
  - workflow
  - orchestration
  - lua
  - instance
  - resume
  - cancellation
---

# Workflow DSL Reference

Use this when writing or debugging Lua workflow scripts passed to `workflow_start`, **or when resuming a crashed instance**.

## 1. Which tool to use

The workflow surface has six focused tools:

| Intent | Tool | Minimum args |
| --- | --- | --- |
| Start a new multi-agent task | `workflow_start` | `script` or `workflow` |
| Resume a crashed / interrupted instance | `workflow_start` | `resume_from_id` |
| Cancel a running instance | `workflow_cancel` | `instance` |
| Find a completed instance | `workflow_list` | optional `limit`, `cursor`, `status_filter` |
| Check one instance | `workflow_status` | `instance_dir` |
| Inspect detailed execution events | `workflow_events` | `instance_dir` |
| View the captured Lua source | `workflow_source` | `instance_dir` |
| Find available workflow definitions | `workflow_files` | none |

After `workflow_start`, use this exact sequence:

```text
workflow_start
→ sleep 5
→ workflow_status
→ repeat only while status == "running"
```

Use a shell wait between status calls. On PowerShell use `Start-Sleep -Seconds 5`; on shells with the standard command use `sleep 5`. Do not poll in a tight loop or issue the wait and status calls in parallel.

### Resuming after a crash

`workflow_start` itself is the entry point for resume. Provide `resume_from_id` instead of `script` / `workflow`:

```json
{
  "resume_from_id": "workflow_1783957281"
}
```

Resuming requires no Lua source — the prior instance's checkpoint and sub-agent conversation history are loaded automatically. Already-completed phases are skipped via the journal cache (zero LLM cost). Agents that were mid-flight when the crash happened continue from their last successful turn via SqliteSaver.

The response echoes back the new run directory plus `resumed_from` for traceability:

```json
{
  "instance_dir": "workflow_1783958200",
  "resumed_from": "workflow_1783957281",
  "status": "running"
}
```

Then poll `workflow_status` on the **new** `instance_dir`. The two identifiers are linked via `resumed_from`; do not call `workflow_status` on the prior id (its state is now a snapshot of the pre-resume pause point).

Use `workflow_list` when the instance identifier is unavailable. Use `workflow_events` only after `workflow_status` identifies a failure or suspicious phase. Use `workflow_source` when reviewing the executed Lua is relevant. `workflow_files` lists definitions available to start; it is not a workflow-result inspection tool.

All workflow tools provide their results directly. Do not use a file-reading tool to follow execution, find reports, or inspect outputs.

### Cancelling a running instance

If a workflow needs to be stopped (wrong arguments, user changed mind, stuck on an agent), use `workflow_cancel`:

```json
{ "instance": "workflow_1783957281" }
```

The in-flight agent (if any) finishes its current turn and returns a `Cancelled` error; the checkpoint is then marked `"cancelled"` instead of `"completed"`. There is no way to resume a cancelled run — restart the workflow with `workflow_start({ script: "..." })` or `workflow_start({ resume_from_id: "<id>" })` after starting a fresh instance.

A `failed` run (status set when the script returned a script error or the runtime panicked) **is** resumable: any agents that completed before the failure wrote their results to the journal cache, so resuming re-runs only the failing agent and hits the cache for the rest. The only terminal statuses that block resume are `completed` and `cancelled`.

The lookup targets the same in-memory registry that `workflow_start` uses, so cancel works only for runs owned by the current process. After the run reaches a terminal state (or belongs to a different process), cancel returns `result="not_found_or_terminal"` — verify with `workflow_status` first if unsure.

## 2. Execution model

- The Lua script is a pure orchestrator. The sandbox disables `io`, `os`, and `require`.
- Real work happens inside `agent()` prompts; the script does not access tools directly.
- `report(value)` is the workflow result.
- `workflow_start` returns before agents finish; `workflow_status` is the source of truth for the public lifecycle state.
- On resume, the Lua script is re-executed from scratch — but agents at previously completed checkpoints return their cached results without re-running.

## 3 Minimal skeleton

```lua
meta = {
  reasoning = "...",
  phases = {
    { label = "discover" },
    { label = "process", dynamic = true },
    { label = "report" },
  },
}
local SCHEMA = { ... }
function main()
  phase("discover")
  phase("process")
  phase("report")
  report({ result = ... })
end
```

For the full Lua DSL reference, see `references/dsl-reference.md`. For schemas, return shapes, and diagnostics, see `references/tool-usage.md`. Load these through the skill tool when needed.

## Additional resources

- `references/architecture-header.md`
- `references/agent-prompts.md`
- `references/task-decomposition.md`
- `references/adversarial-verification.md`
- `references/examples.md`
