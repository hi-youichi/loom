# Skills

This document describes the Loom **Skills** system: how skills are discovered, injected into the system prompt, and loaded at runtime by the agent.

---

## 1. Overview

**Skills** are reusable instruction bundles that extend the agent’s capabilities for specific tasks (e.g. code review, commit messages, deployment). They are stored as Markdown files with optional YAML front matter and are:

1. **Discovered** at startup from project and user directories (and optional profile-defined dirs).
2. **Summarized** in the system prompt so the agent knows which skills exist and when to use them.
3. **Loaded on demand** via the `skill` tool when the agent decides a task matches a skill.

The flow is:

- **Startup** — Loom scans `.loom/skills/` (and optionally `~/.loom/skills/` and profile `skills.dirs`), parses each skill’s metadata, and builds a **skill registry**.
- **Prompt** — A short `<available_skills>` block (name + description per skill) is injected into the system prompt after the role and AGENTS.md, before the base ReAct prompt.
- **Runtime** — The agent can call the `skill` tool with a skill name to load the full instructions, or use `skill` with `name: "list"` to see all available skills.

Skills do **not** register new tools; they only provide instructions and reference material. The format is compatible with Cursor-style SKILL.md (front matter + body).

---

## 2. Skill Format

### 2.1 Directory Layout

Skills can be either **directory-based** (recommended) or **single-file**.

**Directory-based** — A folder whose name is the skill name, with a required `SKILL.md`:

```
.loom/skills/
├── code-review/
│   ├── SKILL.md           # Required: front matter + instructions
│   ├── standards.md       # Optional: reference files (agent can read via read tool)
│   └── examples.md
├── git-workflow/
│   └── SKILL.md
└── deploy.md              # Single-file skill (see below)
```

**Single-file** — A `.md`, `.txt`, or `.markdown` file directly under `.loom/skills/`. The filename (without extension) is used as the skill name. Front matter is optional; without it, the description is empty but the skill is still discoverable.

### 2.2 SKILL.md Front Matter

Use YAML front matter at the top of the file, followed by the instruction body:

```markdown
---
name: code-review
description: >
  Review code for quality, security, and maintainability.
  Use when reviewing PRs or when the user asks for code review.
---

# Code Review

## Instructions
1. Check for correctness and edge cases.
2. Verify security best practices.
3. Assess readability and maintainability.

## Review Checklist
- [ ] Logic correct, edge cases handled
- [ ] No security vulnerabilities
- [ ] Tests cover changes
```

| Field        | Type   | Required | Description |
|-------------|--------|----------|-------------|
| `name`      | string | Yes      | Skill identifier (e.g. kebab-case). Should match the directory name for directory-based skills. |
| `description` | string | No (default: empty) | Short description and **when** to use the skill. Injected into the system prompt so the agent can match tasks to skills. |

The **body** (everything after the second `---`) is the content returned when the agent loads the skill via the `skill` tool. For directory-based skills, a short list of other files in the same directory is appended so the agent can use the `read` tool for reference files.

### 2.3 Legacy Single-File Skills

Files without front matter are still supported. The skill name is the file stem (e.g. `deploy.md` → name `deploy`), and the description is empty. The entire file content is treated as the skill body.

---

## 3. Where Skills Are Loaded From

Skills are discovered in this order. The **first** occurrence of a given `name` wins (later sources do not override).

| Priority | Location | Description |
|----------|----------|-------------|
| 1 | `<working_folder>/.loom/skills/` | Project-level; version-controlled, shared with the team. |
| 2 | Profile `skills.dirs` | Extra directories configured in the agent profile (paths relative to profile or absolute). |
| 3 | `~/.loom/skills/` | User-level; available across projects, not committed. |

If the working folder has no `.loom/skills/` directory, no skills are discovered and no `<available_skills>` block is added to the system prompt.

---

## 4. System Prompt Injection

The resolved system prompt is built in this order (see also [agent.md](./agent.md) §4):

1. **Role / instructions** (from profile, `--role`, or `instructions.md`)
2. **AGENTS.md** (project rules)
3. **Skills prompt** — `<available_skills>` block + optional `<preloaded_skills>` (see §6)
4. **Base ReAct prompt** (workdir rules, approval policy, etc.)

The **available skills** block looks like:

```xml
<available_skills>
When the user's task matches a known skill, use the `skill` tool to load its full instructions before proceeding.

Available skills:
- code-review: Review code for quality, security, and maintainability. Use when reviewing PRs.
- git-workflow: Generate commit messages following team conventions. Use for git operations.
</available_skills>
```

This gives the agent a compact list of skills and when to use them, without loading full content until needed.

---

## 5. Profile Configuration

Agent profiles can configure skills via a `skills` section. See [agent.md](./agent.md) for profile layout and resolution.

### 5.1 Schema

```yaml
skills:
  dirs:                      # Optional: extra directories to scan
    - ./custom-skills
    - /shared/team-skills
  enabled:                   # Optional: whitelist (only these skills)
    - code-review
    - git-workflow
  disabled:                  # Optional: blacklist (exclude these)
    - legacy-format
  preload:                   # Optional: inject full content into system prompt at startup
    - code-review
```

| Field      | Type     | Description |
|------------|----------|-------------|
| `dirs`     | string[] | Additional paths (relative to profile location or absolute) scanned for skills. Same name from a higher-priority source still wins. |
| `enabled`  | string[] | If non-empty, only skills in this list are available. If empty or omitted, all discovered skills are available. |
| `disabled` | string[] | Skills to exclude (applied after `enabled`). |
| `preload`  | string[] | Skill names whose **full content** is injected into the system prompt at startup inside `<preloaded_skills>`. Use for skills the agent needs on every run to avoid an extra tool call. |

### 5.2 Example Profile

```yaml
name: coding-agent
role:
  file: instructions.md
skills:
  dirs:
    - ./company-skills
  enabled:
    - code-review
    - git-workflow
  preload:
    - code-review
model:
  name: claude-sonnet-4-20250514
```

---

## 6. The `skill` Tool

When a working folder is set (and thus file tools are enabled), the **skill** tool is registered. It supports:

| Usage | Behavior |
|-------|----------|
| `skill(name: "list")` | Returns a list of all available skills with their names and descriptions. |
| `skill(name: "<skill-name>")` | Loads the full skill content (body after front matter) and returns it wrapped in `<skill_content name="...">...</skill_content>`. |

If the skill is directory-based, the response also includes a short list of other files in that directory so the agent can use the `read` tool for references (e.g. `standards.md`, `examples.md`).

The tool description in the agent’s tool list explains that the agent should use it when a task matches one of the available skills listed in the instructions.

---

## 7. Creating a Skill

### 7.1 Project-Level Skill (Recommended)

1. Create the skills directory if it does not exist:
   ```bash
   mkdir -p .loom/skills/code-review
   ```
2. Add `SKILL.md` with front matter and body:
   ```markdown
   ---
   name: code-review
   description: Review code for quality, security, and maintainability. Use when reviewing PRs or when the user asks for code review.
   ---

   # Code Review

   ## Instructions
   ...
   ```
3. Optionally add reference files in the same directory (e.g. `standards.md`, `examples.md`). The agent can read them with the `read` tool when the skill content mentions them.

### 7.2 User-Level Skill

Create `~/.loom/skills/<skill-name>/SKILL.md` (or a single file `~/.loom/skills/<name>.md`). These skills are available in every project unless a project or profile skill with the same name overrides them.

### 7.3 Description Best Practices

- Write the **description** in third person and include both **what** the skill does and **when** the agent should use it (trigger scenarios).
- Good: *"Review code for quality, security, and maintainability. Use when reviewing PRs or when the user asks for code review."*
- Avoid vague descriptions; they reduce the agent’s ability to match tasks to skills.

---

## 8. Summary

| Topic | Summary |
|-------|---------|
| **Format** | `SKILL.md` (or `<name>.md`) with optional YAML front matter (`name`, `description`) and a Markdown body. |
| **Locations** | Project: `.loom/skills/`; user: `~/.loom/skills/`; profile: `skills.dirs`. First-seen name wins. |
| **Prompt** | `<available_skills>` block (name + description) injected after role and AGENTS.md; optional `<preloaded_skills>` from profile `skills.preload`. |
| **Runtime** | `skill` tool: `name: "list"` lists skills; `name: "<skill-name>"` loads full content. |
| **Profile** | `skills.dirs`, `skills.enabled`, `skills.disabled`, `skills.preload` in agent profile YAML. |

For agent profile structure and resolution, see [agent.md](./agent.md).
