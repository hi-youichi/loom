# Architecture Header

A header comment block at the very top of the workflow script. Use it to make
the design legible before any code is written.

## Format

```lua
--------------------------------------------
-- Goal:  <one-line objective>
-- Arch:
--   <indented arrow diagram (see below)>
-- Flow:  <single-line data flow chain>
--------------------------------------------
```

Two delimiter lines of 44 dashes wrap the block. The header goes **before**
schema locals or any code.

- **Goal** — single line stating what the workflow produces.
- **Arch** — read top-to-bottom; fan-out lines indent under their parent.
  Every `(for each X)` MUST eventually `<==` back. Show artifacts with `--> [name]`.
- **Flow** — single line showing global data flow as an artifact chain
  (e.g. `discover -> subsystems[] -> modules[] -> report`).

## Diagram notation

Indented arrows, **not** ASCII boxes.

| Notation            | Meaning                                                 |
| ------------------- | ------------------------------------------------------- |
| `==>`               | Sequential or fan-out flow between phases               |
| `<==`               | Fan-in: converge parallel branches back                 |
| `--> [name]`        | Artifact produced by a step (hangs off the right side)  |
| `(for each X)`      | Decomposition dimension (X = module, file, finding...) |
| `(retry <= N)`      | Bounded retry around a sub-chain                        |
| `(degrade on fail)` | Sub-chain that should degrade on failure, not abort     |
| `(parallel)`        | Branches run concurrently                               |
| `(pipeline)`        | Branches run as a staged pipeline                       |
| `\|`                | Optional visual link between a phase and its artifact   |

Indentation = 2 spaces per nesting level.

## Examples

Linear workflow:

```lua
--   discover ==> analyze ==> report
--     |              |
--     --> [targets]  --> [findings]
```

Parallel fan-out / fan-in:

```lua
--   plan ==> (parallel)
--     fetch --> [sources]
--     parse --> [docs]
--     index --> [chunks]
--   <== merge ==> report
```

Decomposed per-module with retry:

```lua
--   discover ==> (for each module)
--     analyze ==> change ==> verify --> [result]
--     (retry <= 2)        (degrade on fail)
--   <== report
```