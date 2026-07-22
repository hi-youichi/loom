# `loom skills inspect <name>` 设计文档

> 在 `loom skills` 命名空间下新增一个 `inspect` 子命令，按 **agent 视角** 深查单个 skill 的元数据、内容、就绪状态、依赖条件、引用文件、运行时统计。对 `loom skills show` 形成补充而非替代。

**状态**：方案设计（已收敛，待实现）
**范围**：CLI 层为主；复用现有 `skill::discovery::SkillRegistry`、`SkillViewTool` 的安全模式和 `tool-workflow` 的 builtin provider。不扩展 `agent/skill` 公共 API。
**本文档不涉及**：实现代码、提交动作、跑大测试。

---

## 1. 背景与现状

### 1.1 三套并存的"skill 视角"

Loom 生态里目前并存三套互相重叠但目的不同的 skill 入口。下表是梳理后的全貌：

| 视角 | 入口（类型 / 命令） | 数据来源 | 核心关注点 | 去重规则 | 输出形态 |
|---|---|---|---|---|---|
| **存储 / 策划** | `cli::run::skill_registry::SkillRegistry`（旧）+ `loom_curator::skill_registry::SkillRegistry` | `~/.loom/data/skills/` 下的 `SKILL.md`、`.usage.json`、`.bundled_manifest` | 生命周期 (`Active`/`Stale`/`Archived`)、出处 (`Auto`/`Manual`/`Evolved`)、curator 统计、被锁定的 hub/bundled skill | 文件名优先、curator 阶段可能改名 | `loom skills show/list/create/edit/delete`（CLI 旧命令） |
| **发现 / 加载** | `agent::skill::discovery::SkillRegistry` | 五条磁盘路径 + builtin 内嵌 | 加载顺序、frontmatter、conditions、embedded content、embedded references | 首次出现胜出（Project > ProfileDir > User > Agent > Data），builtin 仅在磁盘无同名时注入 | `skill_view` tool（agent 端用） |
| **运行时统计** | `loom_curator::SkillUsageStore` | `~/.loom/data/skills/.usage.json` | `use_count` / `view_count` / `patch_count` / `last_*_at` / `pinned` / `created_by == "agent"` | 仅追踪 `agent_created` 的 skill（bundled/hub-owned 排除） | `loom skills-usage show`（CLI） |

**`loom skills show <name>` 的真实位置**：见 `apps/cli/src/subcommands.rs:396-525` 的 `handle_skills_command`。它用的是 **`cli::run::skill_registry::SkillRegistry`**（旧存储视角），输出固定为 6 个字段（`name` / `description` / `source` / `lifecycle` / `triggers` / `body`），**没有** `readiness`、`category`、`conditions`、`requires_tools`、`fallback_for_*`、`supporting files`、`embedded references`、`.usage.json` 统计。

### 1.2 旧 `show` 命令的具体能力（与本设计对比的基线）

```text
$ loom skills show workflow
Skill: workflow
════════════════════════════════════════════════════════════
Description: ...（前 80 字）
Source: Auto
Lifecycle: Active
Triggers: workflow, multi-agent, lua script

[raw body]
```

字段含义：

| 字段 | 类型 | 来源 |
|---|---|---|
| `name` | `String` | 文件名（无 `.md`） |
| `description` | `String` | frontmatter `description`（可能为空字符串） |
| `source` | `Source` 枚举 | `Auto` / `Manual` / `Evolved` —— 由 `.usage.json` 的 `created_by` 推导 |
| `lifecycle` | `Lifecycle` 枚举 | `Active` / `Stale` / `Archived` |
| `triggers` | `Vec<String>` | frontmatter `triggers` |
| `body` | `String` | frontmatter 之后的纯文本（不渲染 conditions / references） |

`show` 不报告的（也是 `inspect` 要补齐的）：

- skill 来自哪条 **发现路径**（Project / ProfileDir / User / Agent / Data / **Builtin**）以及具体 `base_path`
- 是否有 `embedded_content`（即 builtin 内嵌，无对应文件）
- `readiness_status()`：是否缺 env var、平台不支持
- `category` / `category_desc`（分组显示）
- `metadata.conditions.requires_tools` / `fallback_for_tools` / `requires_toolsets` / `fallback_for_toolsets`
- `metadata.tags` / `related_skills` / `required_env_vars`
- `prerequisites.commands`
- 同目录的 supporting 目录（`references/`, `templates/`, `scripts/`, `assets/`）以及内嵌 references
- `.usage.json` 的 `use_count` / `view_count` / `patch_count` / `last_*_at` / `pinned` / `created_by`

### 1.3 `skill_view` tool 的能力（agent 端，不是 CLI）

`agent/tool/tool-basic/src/skill/view.rs` 中的 `SkillViewTool`（tool 名 `TOOL_SKILL_VIEW = "skill_view"`）做了三件事，与本设计强相关：

1. **加载 skill 全文**：从 `SkillRegistry` 找到 entry，优先用 `embedded_content`，否则读 `skill_file`。
2. **解析子文件**：`file_path` 参数（`references/api.md` 形式）可读 skill 目录内的子文件。**带有 path traversal 防护**：见 §7。
3. **同名歧义解析**：内部已有 `name:ns` 语法（`ns` 是子串匹配 `base_path`），但它更适合 agent tool 调用。CLI v1 以显式 `--source <SOURCE>` 为主（见 §7）。

`skill_view` **不**做的事：readiness header 之外不再单独输出 conditions 详情、usage 统计、supporting 目录列表、category 描述、triggers。它把这一堆信息以 `<skill_content>` 块直接给到 LLM，让 LLM 自行消化。

### 1.4 builtin skill 注入机制

`docs/design/builtin-skill-from-tool.md` 已经定了型的机制，对本设计至关重要（尤其 `workflow` builtin）：

- `tool_core::Tool` trait 新增默认方法 `builtin_skill() -> Option<BuiltinSkill>`，默认 `None`。
- `agent_core::run::config_builder::inject_builtin_skills(registry, extra_tools)` 在 `discover()` 之后、`apply_filters` 之前调用，遍历 `extra_tools` 把 `builtin_skill()` 注入。
- `SkillRegistry::add_builtin(name, desc, content, triggers, requires_tools, references)` 把 `(name, Some(content), Some(references))` 形式的 `SkillEntry` 推入；**重名时 no-op**，磁盘胜出。
- 唯一在生产代码中实现了 `builtin_skill()` 的是 **`WorkflowTool`**（`agent/tool/tool-workflow/src/tool.rs:476-510`），其 `name = "workflow"`，内嵌 `workflow_skill.md` 全文 + 5 个 `references/*.md` 文件（`architecture-header` / `agent-prompts` / `task-decomposition` / `adversarial-verification` / `examples`）。

CLI 当前 **完全不会** 调用 `inject_builtin_skills`。`loom skills show workflow` 能否看到 `workflow` 取决于 `sync_skills` 是否已经把 bundled skill 同步到 `~/.loom/data/skills/`，且旧 storage 视角看不到 `embedded_content` / `embedded_files`。`inspect` 的目标之一是直读 builtin 入口，**不再依赖** `sync_skills` 是否已运行或是否拷贝完整。

### 1.5 痛点

| 场景 | 痛点 |
|---|---|
| 调试一个 agent 报"skill not found" | 不知道 `discover` 视角下它叫什么、来自哪条路径、被 `apply_filters` / `apply_toolset_filters` 静默丢掉了没 |
| 写新 skill 后立刻看效果 | `show` 不报告 `readiness`（缺 env var 会怎样）、不报告 `conditions.requires_tools`、不报告当前 toolset 下是否被过滤 |
| builtin skill 调试 | `show` 走的是磁盘副本或找不到；想看真正生效的 `embedded_content` + `embedded_files` 列表无门 |
| 校对 / 文档 | 不容易拿到"skill 在 agent 看来长什么样"的完整 dump |
| 同名 skill | 多个来源同名时，curator 视角下 `show` 静默返回一个；agent 视角下需要明确选哪条 |

`inspect` 就是补这些的，**不是替换 `show`**。

---

## 2. 目标与非目标

### 2.1 目标

1. 在 `loom skills` 下新增 `inspect` 子命令，按 **`skill::discovery::SkillRegistry` 视角**查看单个 skill。
2. 同时能查 builtin skill（注入 `workflow` 等已实现的 builtin）。
3. 输出 readiness / conditions / supporting files / embedded references / usage 统计等 `show` 没有的字段。
4. 支持 `--json` 模式，遵守全局 `--file` 输出重定向。
5. 支持 `--read-file <sub-path>` 读 skill 目录内子文件（含 builtin 的内嵌 references），并与 `skill_view` 的安全行为对齐（含 path traversal 防护）。
6. 同名歧义以 CLI 显式 `--source <SOURCE>` 选择为主；`name:ns` 仅作为后续兼容项记录，不进入 v1 验收。

### 2.2 非目标

- **不**替换 `loom skills show`。`show` 继续走旧 storage 视角服务 curator / 用户管理；`inspect` 走新 discovery 视角服务 agent 调试 / 校对。
- **不**改动 `agent/skill` 核心 API（不增加新 trait 方法、不动 `SkillEntry` 字段）。
- **不**改 `skill_view` tool 的实现或行为。
- **不**实现 skill 的写操作（`create` / `edit` / `delete` 已在 `show` 命名空间下，不动）。
- **不**重写 `loom skills list` / `loom skills create` 等既有命令。
- **不**做 skill 校验 / lint / diff（属于 `loom curator` 范畴）。
- **不**远程访问（仅本机 registry / 文件系统 / `~/.loom/data/skills/.usage.json`）。

---

## 3. 为什么保留 `loom skills show` 并新增 `inspect`

`show` 与 `inspect` 的关系是 **互补** 而非 **二选一**。下表给出每个维度的明确分工：

| 维度 | `show`（旧 storage 视角） | `inspect`（新 discovery 视角） |
|---|---|---|
| Registry | `cli::run::skill_registry::SkillRegistry` | `agent::skill::discovery::SkillRegistry` |
| 路径 | `~/.loom/data/skills/` | 5 条磁盘 + builtin |
| 来源枚举 | `Auto` / `Manual` / `Evolved` | `Project` / `ProfileDir` / `User` / `Agent` / `Data` / **`Builtin`** |
| 生命周期 | `Active` / `Stale` / `Archived` | 不涉及（discovery 视角不感知 lifecycle） |
| readiness | 不报告 | 报告 (`Available` / `SetupNeeded(missing)` / `Unsupported`) |
| conditions | 不报告 | 报告 `requires_tools` / `fallback_for_tools` / `requires_toolsets` / `fallback_for_toolsets` |
| supporting files | 不报告 | 列出 `references/` `templates/` `scripts/` `assets/` |
| embedded refs | 不感知 | builtin 时列出（来自 `embedded_files`） |
| usage 统计 | 不报告（`show` 走 `SkillContent`，不读 `.usage.json`） | 报告（合并 `SkillUsageStore`） |
| category / tags | 不报告 | 报告 |
| 子文件读取 | 不支持 | 支持（`--read-file`） |
| builtin 直读 | 不支持（依赖 `sync_skills` 拷贝） | 支持（CLI 自己 inject） |
| 用途 | curator 流程 / 用户自管理 | agent 调试 / 文档校对 / 排查"为什么没加载" |

保留 `show` 的具体原因：

1. **向后兼容**：现有 shell 脚本 / 文档 / 测评快照里 `loom skills show <name>` 是契约。
2. **关注点分离**：curator 关心 lifecycle、agent 关心 readiness / conditions。混在一起会让输出臃肿且每次新增字段都要回归老调用者。
3. **数据源不同**：`show` 走 `.usage.json` 推 source，inspect 走 `SkillSource` 枚举；两套 source 语义并不严格 1:1 映射（`Source::Auto` ≈ `created_by == "agent"`，但 discovery 视角下没有 "Auto" 这种概念）。
4. **风险更小**：把新功能放到新命令，老命令不变，避免影响已稳定的 curator 路径。

`inspect` 不沿用 `show` 的另一个原因：`show` 命名暗示"人看的小结"，`inspect` 暗示"系统看的完整 dump"，与 `kubectl get` vs `kubectl describe` 的命名习惯一致。

---

## 4. 命令 UX

### 4.1 CLI 语法

```text
loom skills inspect <NAME> [OPTIONS]
```

| 参数 | 必需 | 说明 |
|---|---|---|
| `<NAME>` | 是 | skill 名（frontmatter `name`） |
| `--all` | 否 | 输出全部字段，且 body 不截断；不内联 supporting file 内容 |
| `--json` | 否 | 输出 JSON，遵守全局 `--file` |
| `--read-file <PATH>` | 否 | 读 skill 目录内子文件（如 `references/api.md`），仅打印该文件内容；与其他开关互斥（见 §4.4） |
| `--source <SOURCE>` | 否 | 当同名多来源时按 `SkillSource` 标签过滤（`Project`/`ProfileDir`/`User`/`Agent`/`Data`/`Builtin`） |

全局开关（沿用 `apps/cli/src/args.rs:74-83`）：

- `--json`
- `--file <PATH>` （全局输出重定向：`--json` 时写入 JSON 到文件而非 stdout。注意它不是读取 skill 子文件的开关）
- `--pretty` （JSON 多行缩进）
- `--log-level`、`--log-file`、`--log-rotate`、`--log-format`（沿用已有）

### 4.2 行为矩阵

| 开关组合 | 行为 |
|---|---|
| 默认（仅 `<NAME>`） | 文本概要：核心字段 + 缩略 body |
| `<NAME> --all` | 文本完整：含 supporting files 列表、embedded refs 全量、usage 统计、完整 body；不内联 references 内容 |
| `<NAME> --json` | 单个 JSON 对象（见 §5.2 schema） |
| `<NAME> --json --file /tmp/x.json` | 写入 `/tmp/x.json`（沿用 `apps/cli/src/output.rs::write_json_output`） |
| `<NAME> --read-file references/architecture-header.md` | 仅打印该子文件内容（path traversal 拒绝，§7） |
| `<NAME> --read-file` + `--json` | 错误：`--read-file` 与 `--json` 互斥 |
| `<NAME> --read-file` + `--all` | 错误：互斥 |

### 4.3 文本 view 样例（默认）

```text
$ loom skills inspect workflow
Skill: workflow
════════════════════════════════════════════════════════════
Source:        Builtin
Path:          (embedded)  ← base_path 为空，content 来自 include_str!
Readiness:     Available
Category:      general
Triggers:      workflow, multi-agent, lua script

Conditions:
  requires_tools:     workflow
  requires_toolsets:  (none)
  fallback_for_tools: (none)

Supporting files: (none on disk; embedded instead)
Embedded references:
  - references/architecture-header.md
  - references/agent-prompts.md
  - references/task-decomposition.md
  - references/adversarial-verification.md
  - references/examples.md

Usage (.usage.json):
  use_count:       0
  view_count:      0
  patch_count:     0
  last_used_at:    (never)
  last_viewed_at:  (never)
  pinned:          false
  created_by:      (not marked)

────────────────────────────────────────────────────────────
Body (truncated, 1.2 KB / 5.8 KB — use --all to see all):
[body text...]
```

### 4.4 互斥规则

```
--read-file <PATH>   互斥  --json, --all
--all           与 --json 兼容（json schema 不变，但 body 字段长度不限）
--source        v1 唯一正式消歧机制；name:ns 仅作为后续兼容项记录
```

错误信息统一格式：

```
error: invalid flag combination
  --read-file cannot be combined with --json or --all
```

### 4.5 退出码

| 场景 | 退出码 |
|---|---|
| 成功 | `0` |
| 找不到 skill | `2`（与现有 `show` 的失败行为一致） |
| 歧义未消解 | `2`（错误信息列出所有候选，附 `--source` 提示） |
| `--read-file` path traversal 拒绝 | `2` |
| I/O 错误（read 失败） | `3` |
| 互斥选项冲突 | `2` |

---

## 5. 输出字段

### 5.1 文本 view 字段顺序

| # | 字段 | 来源 | 备注 |
|---|---|---|---|
| 1 | `Name` | `entry.metadata.name` | 必显 |
| 2 | `Source` | `entry.source.label()`（`SkillSource::label`） | 显示 `Builtin` / `Project` / 等 |
| 3 | `Path` | `entry.base_path`（若为空 → `(embedded)`） | 优先显示磁盘路径；builtin 显示 `(embedded)` |
| 4 | `Skill file` | `entry.skill_file`（仅当 Source ≠ Builtin） | builtin 时隐藏 |
| 5 | `Readiness` | `metadata.readiness_status()` | 文本化：`Available` / `SetupNeeded: missing FOO, BAR` / `Unsupported: <reason>` |
| 6 | `Category` | `metadata.category` + `category_desc` | 二者拼成 `name — desc` |
| 7 | `Triggers` | `metadata.triggers` | 逗号分隔；空时显示 `(none)` |
| 8 | `Tags` | `metadata.metadata.tags` 或顶层 `metadata.tags` | 缺省时折叠到 `Conditions` 块 |
| 9 | `Conditions` | `metadata.conditions()` | 见 §5.3 |
| 10 | `Required env vars` | `metadata.required_env_vars()` | 与 readiness 重复时省略 |
| 11 | `Prerequisites` | `metadata.prerequisites.commands` | 空时折叠 |
| 12 | `Related skills` | `metadata.metadata.related_skills` | 折叠 |
| 13 | `Supporting files` | 扫 `entry.base_path` 下的 `references/` `templates/` `scripts/` `assets/` | builtin 时此区为 `(none on disk; embedded instead)` |
| 14 | `Embedded references` | `entry.embedded_files` | 仅 builtin 时出现；列出 `(name, byte_size)` |
| 15 | `Usage (.usage.json)` | `SkillUsageStore::get(name)` | 见 §5.4 |
| 16 | `Body` | `parse_frontmatter` 后的 `body` | 默认截断到 ~1.2 KB；`--all` 完整 |

### 5.2 JSON schema（`--json`）

```json
{
  "name": "workflow",
  "source": "Builtin",
  "source_raw": "Builtin",
  "path": null,
  "skill_file": null,
  "is_builtin": true,
  "readiness": {
    "status": "Available",
    "missing_env_vars": [],
    "unsupported_reason": null
  },
  "category": "general",
  "category_desc": null,
  "description": "Lua DSL reference for writing multi-agent workflows",
  "triggers": ["workflow", "multi-agent", "lua script"],
  "tags": [],
  "conditions": {
    "requires_tools": ["workflow"],
    "requires_toolsets": [],
    "fallback_for_tools": [],
    "fallback_for_toolsets": []
  },
  "required_env_vars": [],
  "prerequisites": { "commands": [] },
  "related_skills": [],
  "supporting_files": {
    "references": [],
    "templates": [],
    "scripts": [],
    "assets": []
  },
  "embedded_references": [
    { "name": "references/architecture-header.md", "byte_size": 1234 },
    { "name": "references/agent-prompts.md",         "byte_size":  987 },
    { "name": "references/task-decomposition.md",    "byte_size":  654 },
    { "name": "references/adversarial-verification.md", "byte_size": 2100 },
    { "name": "references/examples.md",              "byte_size": 3456 }
  ],
  "usage": {
    "use_count": 0,
    "view_count": 0,
    "patch_count": 0,
    "last_used_at": null,
    "last_viewed_at": null,
    "last_patched_at": null,
    "last_activity_at": null,
    "created_at": "2025-08-19T12:00:00Z",
    "state": "Active",
    "pinned": false,
    "archived_at": null,
    "absorbed_into": null,
    "created_by": null
  },
  "body": "# Workflow DSL Reference\n\n...",
  "frontmatter_raw": "name: workflow\ndescription: ...\ntriggers: [...]"
}
```

> 注：`body` 字段在 JSON 模式下**不截断**（与 `--all` 等价）。调用方需要用 `jq` 截取。

### 5.3 `Conditions` 文本格式

```
Conditions:
  requires_tools:     workflow
  requires_toolsets:  (none)
  fallback_for_tools: (none)
  fallback_for_toolsets: (none)
```

仅显示**非空**项（节省输出），`--all` 时显示全部（含 `(none)`）。

### 5.4 `Usage` 文本格式

```
Usage (.usage.json):
  use_count:       3
  view_count:      12
  patch_count:     0
  last_used_at:    2025-08-15T09:23:11Z
  last_viewed_at:  2025-08-19T08:00:00Z
  last_activity_at:2025-08-19T08:00:00Z
  state:           Active
  pinned:          false
  created_by:      (not marked)
```

当 `SkillUsageStore::get(name)` 返回 `None`（即 `.usage.json` 里没有该 skill）时，显示 `(not in .usage.json)`。注意 `SkillUsageStore::is_agent_created` 的判断**不影响**显示 —— 即 builtin 技能即使从不进 `.usage.json`（因为 `bump_*` 会早返），我们依然如实显示 `view_count: 0` 等。

### 5.5 Body 截断规则

| 模式 | 截断阈值 | 提示语 |
|---|---|---|
| 默认 | 前 1.2 KB 或前 30 行（先到者） | `(truncated, 1.2 KB / 5.8 KB — use --all to see all)` |
| `--all` | 不截断 | 无 |
| `--read-file <PATH>` | 不适用（直接输出子文件） | 无 |

JSON 模式不截断。

---

## 6. builtin skill 注入策略

### 6.1 为什么 CLI 端需要重新注入

`SkillRegistry::add_builtin` 不会持久化 —— 它只是把内存里的 `SkillEntry.embedded_content` / `embedded_files` 字段填上。`SkillRegistry::discover` 只扫磁盘，**不** 自动调用 `inject_builtin_skills`（后者是 `agent_core::run::config_builder` 的私有函数，依赖 `extra_tools: Vec<Arc<dyn Tool>>`）。

CLI 没有现成的运行时 tool registry 上下文，所以 `loom skills inspect` 必须**主动**触发 builtin 注入。两种选择：

| 方案 | 优点 | 缺点 |
|---|---|---|
| A. CLI 硬编码工具列表 | 简单；不依赖 `agent-core` | 每次新工具加 `builtin_skill()` 都要改 CLI |
| B. CLI 复用现有 workflow tool provider，在 CLI 层构造 `WorkflowTool` 并注入 builtin | 不引入 `agent-core -> tool-workflow` 反向依赖；`apps/cli` 已依赖 `tool-workflow`；实现边界清楚 | CLI 需要维护 builtin provider 列表 |

**采用方案 B**。具体动作：

- 在 `apps/cli/src/skill_inspect.rs` 中新增 CLI 专用 registry 构造函数：

  ```rust
  /// Discover skills from the same filesystem locations as the agent,
  /// then inject CLI-known builtin skills such as WorkflowTool.
  fn build_inspect_registry(
      working_folder: &Path,
      extra_dirs: &[PathBuf],
  ) -> (SkillRegistry, Vec<BuiltinSkillContribution>) { ... }
  ```

- `BuiltinSkillContribution` 是 `apps/cli/src/skill_inspect.rs` 内部结构体，仅用于 `--json` / debug 输出，不作为公共 API：

  ```rust
  struct BuiltinSkillContribution {
      pub tool_name: String,
      pub skill_name: String,
      pub source: SkillSource, // always Builtin
  }
  ```

- CLI 调 `build_inspect_registry(&cwd, &[])`，**不**触发 `apply_filters` / `apply_toolset_filters`（inspect 视角要看原始全集）。
- `build_inspect_registry` 先调用 `SkillRegistry::discover(working_folder, extra_dirs)`，再构造 `tool_workflow::WorkflowTool` 并调用 `Tool::builtin_skill()`，最后调用 `registry.add_builtin(...)`。
- 不修改 `agent-core::run::config_builder::inject_builtin_skills` 的签名或可见性；该函数继续服务 agent runtime。

### 6.2 workflow builtin 的特殊地位

`WorkflowTool` 是目前唯一实现了 `builtin_skill()` 的工具，所以 `inspect workflow` 必须能直接看到 builtin 版本。

CLI 的 `build_inspect_registry` 内部构造 `WorkflowTool::new(AgentConfig::default())`（同 `agent/tool/tool-workflow/tests/builtin_skill.rs` 的测试套路），取出 `builtin_skill()`，调 `add_builtin`。这样：

- `loom skills inspect workflow` 优先返回 **builtin 入口**（`Source: Builtin`，`Path: (embedded)`），`embedded_content` = `workflow_skill.md` 全文。
- 5 个 `references/*.md` 列在 `embedded_references`，每条带 `byte_size`。
- 不会依赖 `sync_skills` 是否已把 bundled 副本同步到 `~/.loom/data/skills/`。
- 即便用户用 `.loom/skills/workflow/SKILL.md` 覆盖了内置版（见 `add_builtin` 重名 no-op 逻辑），`inspect` 仍按 `Source: Project` 返回用户版本，**不**回退到 builtin。这一点 `add_builtin` 自身已保证。

### 6.3 未来扩展点

当第二个工具实现 `builtin_skill()` 时，只需：

1. 在 `apps/cli/src/skill_inspect.rs` 维护的 CLI builtin provider 列表里追加构造闭包。
2. `build_inspect_registry` 遍历闭包统一注入。

这仍是 CLI 层改动，但范围局部，不改变 agent runtime 的 builtin 注入路径。

### 6.4 `add_builtin` 的语义保持不变

重申现有规则，本设计不动：

- 同名 no-op（用户 / 项目盘内 skill 永远胜出）。
- `embedded_content` / `embedded_files` 仅 builtin 时使用。
- `apply_toolset_filters` 仍按 `requires_tools` 过滤 —— 但本命令**不**调 `apply_filters`，所以 inspect 出来的 builtin 即使当前 toolset 不满足也会被列出，并在文本里提示 `requires_tools: workflow`（用户自检）。

---

## 7. 同名 skill 歧义处理

### 7.1 可能出现歧义的来源

`skill::discovery::SkillRegistry::discover` 在去重时**先到先得**（Project > ProfileDir > User > Agent > Data > Builtin）。所以正常情况下同名只会有一个 entry。但以下情况会出现多匹配：

1. **显式 `add_agent_skills` 之后**：CLI 不走这条路径，但若 future 调用方在 `discover` 之后又 `add_agent_skills`，可能与原 Project 冲突。
2. **用户 inspect 时自己指定了不同 `--source`**：CLI 看到多个候选就触发歧义。
3. **builtin 与磁盘重名**：见 §6.2，CLI 当前用 `add_builtin` no-op 解决，**不**触发歧义。但如果未来用其他方式注入（多 builtin 工具同名），就会出现歧义。

### 7.2 消歧方案

#### 7.2.1 `--source <SOURCE>` 显式选择（v1 主路径）

```text
loom skills inspect workflow --source Builtin
loom skills inspect workflow --source Project
```

**优点**：自解释；clap 自动补全（用 `value_enum`）。
**缺点**：与 `skill_view` 的内部 `name:ns` 风格不同，但 CLI 可读性更好。

#### 7.2.2 `name:ns` 风格（v1 非目标，后续兼容项）

`SkillViewTool::resolve_name_with_ns` 已经实现了：

```text
workflow:project
workflow:builtin
```

但它的 `ns` 当前是对子串匹配 `base_path`，不是稳定的 CLI 用户契约。因此 v1 **不**把 `name:ns` 作为帮助文本、错误 hint 或验收标准的一部分。后续若要正式支持，应先把 `ns` 改成稳定枚举或明确规范。

### 7.3 决策：**v1 主用 `--source`**

- 默认行为：检测到多匹配时报错，错误信息列出所有候选及 `Source` 标签，附 `Use --source <SOURCE>` 提示。
- `--source <SOURCE>`：按 `SkillSource` 标签精确匹配。`SOURCE` 是 `value_enum`，clap 拒绝非法值。
- `name:ns`：v1 不作为正式 CLI contract；文档仅记录后续兼容方向，实现和错误信息都不必支持。
- 错误信息格式：

  ```
  error: ambiguous skill 'workflow': found 2 matches
    1. workflow (Source: Project)  path=/home/u/proj/.loom/skills/workflow
    2. workflow (Source: Builtin)  path=(embedded)
  hint: use --source <Source> (e.g. --source Builtin)
  ```

### 7.4 resolver 位置

v1 resolver 放在 `apps/cli/src/skill_inspect.rs` 内部，输入 `&[SkillEntry]`、`name`、`Option<SkillSourceFilter>`，输出 0/1/N 个候选。这样不扩大 `agent/skill` 公共 API，也不影响 `SkillViewTool`。

---

## 8. Path Traversal 防护

### 8.1 攻击面

`--read-file <PATH>` 让用户读到 `skill_dir` 内的任意文件。如果 `<PATH>` 含 `..` 或符号链接，可能逃出 skill 目录读到 `~/.ssh/id_rsa` 之类的敏感文件。

### 8.2 复用 `SkillViewTool::view_sub_file` 的防御

`agent/tool/tool-basic/src/skill/view.rs:220-264` 已经实现了该模式：

```rust
let target = skill_dir.join(file_path);

let canonical_skill = skill_dir
    .canonicalize()
    .map_err(|e| ToolSourceError::InvalidInput(format!("invalid skill dir: {}", e)))?;
let canonical_target = target.canonicalize().map_err(|e| {
    ToolSourceError::InvalidInput(format!(
        "file '{}' not found in skill '{}': {}",
        file_path, skill_name, e
    ))
})?;

if !canonical_target.starts_with(&canonical_skill) {
    return Err(ToolSourceError::InvalidInput(format!(
        "path traversal: '{}' is outside skill directory",
        file_path
    )));
}
```

三个关键点：

1. **先 `canonicalize` 再比较**：`canonicalize` 解析 `..` 和符号链接，绝对路径上做前缀比较。
2. **`starts_with` 防 prefix 撞库**：`/home/u/proj/.loom/skills/workflow-evil/` 不应该 prefix-match `/home/u/proj/.loom/skills/workflow`，但因 `starts_with` 不会因 `/` 边界出错，正确拒绝。
3. **不存在即报错**：跳过 `is_file()` 检查，让 `canonicalize` 失败兜底。

### 8.3 v1 在 CLI 内部实现 `safe_join_under`

为避免扩大 `agent/skill` 公共 API，v1 先在 `apps/cli/src/skill_inspect.rs` 内部实现一份小型 helper，并直接照搬 `SkillViewTool::view_sub_file` 的安全模式：

```rust
fn safe_join_under(skill_dir: &Path, file_path: &str) -> Result<PathBuf, SkillInspectError> {
    let target = skill_dir.join(file_path);
    let canonical_skill = skill_dir.canonicalize()
        .map_err(|e| SkillInspectError::InvalidSkillDir(e))?;
    let canonical_target = target.canonicalize()
        .map_err(|e| SkillInspectError::FileNotFound(file_path.to_string(), e))?;
    if !canonical_target.starts_with(&canonical_skill) {
        return Err(SkillInspectError::PathTraversal(file_path.to_string()));
    }
    Ok(canonical_target)
}
```

后续如果 `SkillViewTool` 和 CLI 都需要持续演进，再把它下沉到 `agent/skill`；v1 不做这一步，减少跨 crate 改动。

### 8.4 builtin skill 的 `--read-file` 行为

builtin skill 没有 `base_path`（`PathBuf::new()`），`--read-file` 必须**仅**查 `embedded_files` 列表：

```rust
if entry.embedded_content.is_some() {
    // builtin path
    let refs = entry.embedded_files.as_deref().unwrap_or(&[]);
    return refs.iter()
        .find(|(n, _)| n == &file_path)
        .ok_or_else(|| SkillInspectError::FileNotFound(file_path.to_string(), io::Error::...));
}
```

**绝不能** `entry.base_path.join(file_path)` —— `base_path` 为空，拼接结果不可控。

`--read-file references/architecture-header.md` 命中 builtin 的内嵌 references 时，直接打印 `(name, content)` 的 `content` 字段。这条路径**完全无 path traversal 风险**（数据在内存里，不接触文件系统），但仍要在 `file_path` 与 entry 的 embedded refs 列表做严格 `==` 匹配，不做前缀匹配（避免 `references/architecture` 撞 `references/architecture-header.md`）。

### 8.5 错误信息

```
error: path traversal: 'references/../../../etc/passwd' resolves outside skill directory 'workflow'
```

非 verbose 模式不暴露 canonical 后的绝对路径，避免反向泄露 skill 存储位置（agent 场景下 `~/.loom/data/skills/...` 可能是敏感信息）。

---

## 9. 实现文件范围

### 9.1 新增 / 修改文件清单

| 文件 | 改动 | 行数估计 |
|---|---|---|
| `apps/cli/src/skill_inspect.rs`（新） | `inspect` 命令实现：参数解析、registry 构造、文本 / JSON 渲染、`--read-file` 处理 | ~280 |
| `apps/cli/src/args.rs` | `SkillsCommand` 新增 `Inspect { name, all, read_file, source }` 变体 | +15 |
| `apps/cli/src/subcommands.rs` | `handle_skills_command` 增加 `Inspect` 分支；路由到 `skill_inspect::run`；签名从 `(skills_args, json)` 扩展为 `(skills_args, json, output_file)` | +25 |
| `apps/cli/src/main.rs` | `mod skill_inspect;` | +1 |

总计：**新文件 1 个，修改 3–4 个，约 300–380 行（含测试）**。v1 不要求修改 `agent-core`、`agent/skill` 或 `tool-basic`。

### 9.2 不修改的部分（明确划线）

- `agent/skill/src/discovery.rs::SkillRegistry` 的字段 / API（不新增字段、不改 `discover` 行为）。
- `SkillViewTool` 的 `view_from_directory` / `view_from_registry` 主逻辑。
- `cli::run::skill_registry`（旧 storage 视角）的任何代码。
- `loom_curator::skill_registry`。
- `SkillEntry` 的 `embedded_content` / `embedded_files` 字段含义。
- `inject_builtin_skills` 现有签名 / 行为。
- `agent-core` 的 run/config builder。

### 9.3 crate 依赖变化

- `apps/cli/Cargo.toml`：当前已依赖 `agent`、`skill`、`tool-workflow`（需要实现前确认）；v1 预期不新增依赖。
- `agent/skill/Cargo.toml`：无改动。
- `agent/agent-core/Cargo.toml`：无改动，避免 `agent-core -> tool-workflow` 反向依赖。

### 9.4 CLI 命令注册

`apps/cli/src/main.rs` 当前只把 `args.json` 传给 `handle_skills_command`。为了让 `inspect --json --file out.json` 复用全局输出重定向，必须把 `args.file.as_deref()` 一并传入：

```rust
if let Some(Cmd::Skills(sa)) = &args.cmd {
    if let Err(err) = handle_skills_command(sa, args.json, args.file.as_deref()) {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}
```

`apps/cli/src/subcommands.rs::handle_skills_command` 目标形态：

```rust
match &skills_args.command {
    SkillsCommand::List => { ... }
    SkillsCommand::Show { name } => { ... }
    // ...
SkillsCommand::Inspect { name, all, read_file, source } => {
    return skill_inspect::run(name, *all, read_file.as_deref(), source.as_ref(), json, output_file);
}
}
```

`run` 函数签名（草案）：

```rust
pub fn run(
    name: &str,
    all: bool,
    read_file: Option<&Path>,
    source: Option<&SkillSourceFilter>,
    json: bool,
    output_file: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>>
```

### 9.5 不引入新 crate

`skill_inspect` 是 `apps/cli` 内部子模块，不发新 crate。`safe_join_under` v1 放在 `apps/cli/src/skill_inspect.rs` 内部，后续确有复用需求时再下沉到 `agent/skill`。

---

## 10. 测试计划

### 10.1 单元测试（`apps/cli/src/skill_inspect.rs` 同文件 `#[cfg(test)] mod tests`）

| 用例 | 覆盖点 |
|---|---|
| `text_view_default_truncates_body` | body 超过 1.2 KB 时被截断并附 `(truncated ...)` 提示 |
| `text_view_all_includes_full_body_and_embedded_refs` | `--all` 时 body 完整、embedded refs 全列 |
| `json_view_emits_complete_schema` | JSON 输出所有字段（spot-check 9 个关键字段） |
| `json_to_file_uses_global_file_flag` | `--json --file /tmp/x.json` 写到文件而非 stdout |
| `read_file_rejects_path_traversal` | `--read-file references/../../etc/passwd` 错误退出 |
| `read_file_accepts_valid_subpath` | builtin `--read-file references/examples.md` 输出其 content |
| `read_file_rejects_prefix_collision` | `--read-file references/architecture` 不命中 `references/architecture-header.md` |
| `source_flag_filters_by_label` | `--source Builtin` 只返回 builtin entry |
| `ambiguous_errors_lists_candidates` | 两个 entry 时错误信息包含两个候选及 hint |
| `name_ns_disambiguates` | `workflow:builtin` 只匹配 builtin |
| `nonexistent_skill_exits_2` | 找不到时退出码 2 + 错误信息含 `not found` |
| `mutual_exclusion_json_read_file` | `--read-file` + `--json` 报错 |
| `mutual_exclusion_all_read_file` | `--read-file` + `--all` 报错 |
| `builtin_workflow_injected_via_build_inspect_registry` | `build_inspect_registry` 注入 workflow builtin，`Source::Builtin` 出现 |
| `disk_skill_overrides_builtin` | `.loom/skills/workflow/SKILL.md` 存在时，inspect 返回 Project 版本，builtin 不覆盖 |

### 10.2 `agent/skill` 端的单元测试

| 文件 | 用例 |
|---|---|
| `apps/cli/src/skill_inspect.rs` | `safe_join_under_relative` / `safe_join_under_symlink_escape` / `safe_join_under_not_found` / `safe_join_under_traversal_dotdot` |
| `apps/cli/src/skill_inspect.rs` | `build_inspect_registry_returns_workflow_builtin` / `build_inspect_registry_preserves_disk_overrides` |

### 10.3 集成测试

`agent/tool/tool-workflow/tests/builtin_skill.rs` 已经覆盖了 `WorkflowTool::builtin_skill() -> registry.add_builtin -> load_skill_with_dir` 的完整链路，本设计**复用**这套测试。

新增 `apps/cli/tests/skill_inspect.rs`（如果未来有集成测试目录），跑端到端：

```rust
#[test]
fn cli_inspect_workflow_shows_builtin() {
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skills", "inspect", "workflow", "--json"])
        .output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["source"], "Builtin");
    assert!(v["embedded_references"].as_array().unwrap().len() >= 5);
}

#[test]
fn cli_inspect_with_global_file_writes_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("o.json");
    Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skills", "inspect", "workflow", "--json", "--file", p.to_str().unwrap()])
        .assert().success();
    let content = std::fs::read_to_string(&p).unwrap();
    assert!(content.contains("Builtin"));
}
```

**注**：本文档不要求实现阶段跑这些，只列在测试计划里。集成测试目录 `apps/cli/tests/` 当前是否存在需在实现前 `ls` 确认；若无，新建。

### 10.4 回归测试

- `loom skills show workflow` 行为不变（保留旧命令不破坏）。
- `SkillViewTool` 不在 v1 改动范围内；其既有测试无需因 `inspect` 实现变更。
- `agent/skill` crate 既有 15 个 `discovery.rs` 测试 + `add_builtin_inserts_skill` / `add_builtin_skips_when_name_exists` 等继续 pass。

### 10.5 手工 smoke 测试清单

实现完成后开发者手动跑：

```text
loom skills inspect workflow                  # builtin 文本
loom skills inspect workflow --json           # builtin JSON
loom skills inspect workflow --all            # 完整 body
loom skills inspect workflow --read-file references/examples.md
loom skills inspect workflow --read-file references/../etc/passwd  # 应拒
loom skills inspect nonexistent               # 退出码 2
loom skills inspect workflow:builtin --json   # ns 消歧
loom skills inspect workflow --source Builtin # source 消歧
loom skills inspect workflow --json --file /tmp/x.json  # 全局 file
```

---

## 11. 风险

### 11.1 行为 / 用户面风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| 用户期望 `inspect` 替换 `show`，发现两者并存后困惑 | 低 | 帮助文本明确 `inspect` 用途；错误信息中不再引导到 `show`；文档/README 单独章节对比 |
| `add_builtin` 的"用户盘内胜出"行为导致 builtin 在用户覆盖时不被 inspect 看到，调试时困惑 | 中 | 在 builtin 不出现时打印 `note: builtin 'workflow' is shadowed by user file at <path>`，提示覆盖发生 |
| `--read-file` 大内容（如 `examples.md` 几 MB）被一口气打到 stdout | 低 | 加 `--max-bytes` 上限（默认 5 MB，超出报错而非截断；agent 调 `read` tool 自己分页） |
| JSON 模式下 body 巨大 | 低 | 调用方用 `jq '.body, .embedded_references[] | length'` 自取所需；不在 CLI 截断 |
| `build_inspect_registry` 把 workflow builtin 强注入，导致普通用户 inspect 看到 builtin 而非自己版本 | 中 | 见上 `note: ... shadowed by user file`，并把 builtin 注入顺序严格在 `discover` 之后（沿用 `add_builtin` no-op） |

### 11.2 实现 / 代码质量风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| `safe_join_under` 与 `SkillViewTool::view_sub_file` 漂移 | 低 | v1 明确照搬当前安全模式，并在 CLI 内部加边界测试；后续有复用需求再下沉 |
| `name:ns` 与 `--source` 双路径让实现复杂 | 低 | v1 不实现 `name:ns`，仅在设计文档中作为后续兼容项说明 |
| `build_inspect_registry` 维护 CLI builtin provider 列表 | 中 | v1 只注入 `WorkflowTool`；新增 builtin provider 时在 `skill_inspect.rs` 单点追加 |
| `--source` 接受 enum 时 clap 与 `SkillSource` 标签大小写不一致 | 低 | 显式实现 `clap::ValueEnum`，与 `SkillSource::label()` 严格对齐 |
| 互斥选项检查放在 clap derive 层 vs 业务层 | 低 | 互斥用 `#[arg(conflicts_with = ...)]`，互斥错误信息统一在业务层格式化 |
| 性能：5 条磁盘路径扫描 + builtin 注入在冷启动 ~50ms 级别 | 极低 | CLI 单次命令，可接受；不缓存 |

### 11.3 安全风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| `--read-file` 越权读（path traversal） | 中 | `safe_join_under`（§8.3）；canonicalize + starts_with；非 verbose 模式不泄露绝对路径 |
| 错误信息泄露 `~/.loom/data/skills/<user-skill>` 绝对路径 | 低 | 普通错误信息用相对路径（如 `(embedded)` 或 skill 名）；详细模式（未来加 `--verbose`）才给绝对路径 |
| JSON body 含敏感 frontmatter（API key 注释之类） | 极低 | CLI 不做内容过滤；用户自己负责；`SkillUsageStore` 也不存敏感内容 |

### 11.4 项目惯例 / 维护风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| 不熟悉设计文档格式的 reviewer 要求改 heading 风格 | 低 | 沿用 `docs/design/builtin-skill-from-tool.md` 的层级和元数据格式 |
| 后续 builtin provider 增多后 CLI 层列表遗漏 | 低 | 把 provider 列表和测试放在 `skill_inspect.rs` 同文件，新增 provider 必须补测试 |
| `SkillSource` 未来新增变体（如 `Runtime`） | 低 | `clap::ValueEnum` 用 `#[value(skip)]` 或 `derive` 自动同步，**不**硬编码枚举字符串列表 |

---

## 12. 非目标（重复强调以免误读）

- **不**替换 `loom skills show`。
- **不**修改 `agent/skill` 核心 trait / struct。
- **不**在 CLI 端持久化 builtin skill（依旧走 `add_builtin` 内存态）。
- **不**实现 skill 写操作、curator 触发、evolve、archive、pin/unpin。
- **不**支持远程 / 跨机器 inspect。
- **不**提供交互式 UI（`less` 翻页由 shell 自带 `| less` 处理）。
- **不**改 `skill_view` tool 的 contract 或内部实现。
- **不**做 skill 校验 / lint / 反链扫描 / 反向引用（属于 `loom curator` 范畴，未来可单独设计）。

---

## 13. 验收标准

实现完成的判定（实现阶段评审用）：

### 13.1 命令可用性

- [ ] `loom skills inspect --help` 列出所有参数、互斥规则、示例（示例可在 `#[command(after_help = ...)]` 加）。
- [ ] `loom skills inspect <name>` 在三种 skill 来源（项目、用户、builtin）下均能查到。
- [ ] 找不到 skill 时退出码 2，错误信息含 `not found`。
- [ ] 歧义时报错，错误信息列出全部候选及 `--source` 提示。

### 13.2 builtin workflow 可查

- [ ] `loom skills inspect workflow --json` 输出 `source == "Builtin"`。
- [ ] `embedded_references` 包含 5 项（`architecture-header` / `agent-prompts` / `task-decomposition` / `adversarial-verification` / `examples`），每项 `byte_size > 0`。
- [ ] `loom skills inspect workflow --read-file references/examples.md` 输出 examples.md 的完整内容。
- [ ] `loom skills inspect workflow --read-file nonexistent.md` 退出码 2。

### 13.3 字段完整

- [ ] 文本模式输出 §5.1 列出的 16 项字段（按顺序）。
- [ ] JSON 模式输出 §5.2 列出的所有键。
- [ ] `usage` 字段从 `~/.loom/data/skills/.usage.json` 读取；缺失时显示 `(not in .usage.json)`。
- [ ] `readiness` 反映 `required_env_vars` 检查结果（可用一个 mock env var 测试 `SetupNeeded` 路径）。

### 13.4 防护到位

- [ ] `--read-file ../../../etc/passwd` 退出码 2，错误信息含 `path traversal`。
- [ ] `--read-file references/../../etc/passwd` 同样被拒（即使真实文件存在）。
- [ ] 符号链接逃逸：若 `references/foo` 是指向 `/etc/passwd` 的 symlink，错误信息 `path traversal`。
- [ ] builtin 的 `--read-file` 严格 `==` 匹配 embedded refs 列表项；前缀不命中。

### 13.5 全局开关联动

- [ ] `--json --file <PATH>` 写到文件而非 stdout。
- [ ] `--json` + `loom --pretty` 的全局开关传递到子命令（与现有 `output.rs::write_json_output(..., pretty: true)` 对齐）。
- [ ] `--read-file` + `--json` / `--all` 报互斥错误。

### 13.6 不破坏现有行为

- [ ] `loom skills show workflow` 输出不变（snapshot diff 0）。
- [ ] `loom skills list` 输出不变。
- [ ] `loom skills create/edit/delete` 不受影响。
- [ ] `SkillViewTool` 既有 6+ 个集成测试（`agent/tool/tool-workflow/tests/builtin_skill.rs`）继续 pass。
- [ ] `agent/skill` 既有 15 个 `discovery.rs` 单测继续 pass。

### 13.7 代码质量

- [ ] `apps/cli/src/skill_inspect.rs` 单元测试 ≥ 12 个（§10.1 列表），全部 pass。
- [ ] `apps/cli/src/skill_inspect.rs::safe_join_under` 单测覆盖 4 边界（§10.2）。
- [ ] CLI 内部 `build_inspect_registry` 至少 2 个单测。
- [ ] `clippy` 无新增 warning（允许 `#[allow(...)]` 仅当有 doc 说明）。
- [ ] `cargo doc` 不产生 broken link。
- [ ] `apps/cli` 既有 doctest 继续 pass（若有）。

### 13.8 文档

- [ ] `docs/design/skill-inspect-cli.md`（本文档）随 PR 一并提交。
- [ ] `README.md` 或 `docs/cli.md` 新增 `inspect` 一节（实现阶段补，与本文档不重叠）。
- [ ] 错误信息 hint 中提到的 `--source` 在 `loom skills inspect --help` 中可找到。

### 13.9 不引入

- [ ] 不新增 crate。
- [ ] 不改 `agent/skill` 公共 trait 签名。
- [ ] 不改 `SkillViewTool` 公共 spec（input_schema / output_hint）。
- [ ] 不改 `loom_curator::skill_registry`。
- [ ] 不依赖网络。

---

## 14. 开放问题（评审时确认）

1. **`build_inspect_registry` 的 cwd 来源**：用 `std::env::current_dir()`、还是 `CliArgs::cwd`（如果已有）、还是新加全局 `--working-folder`？现状 `Cli` 已有 `cwd: Option<PathBuf>`（见 `args.rs:27`），优先复用。
2. **`--source` 命名**：用户视角下 `Builtin` / `Project` 足够自解释；但能否用更友好的别名（`bundled` ↔ `Builtin`、`local` ↔ `Project`）？倾向**不**引入别名，保持 `SkillSource::label()` 一致。
3. **是否保留 `name:ns` 兼容方向**：当前设计不实现 `name:ns`，只在文档里说明后续兼容方向。若 reviewer 认为这会误导实现者，可从本文档删除该兼容项。
4. **`safe_join_under` 的错误信息**：是否要区分 "not found" 和 "is a directory"（`SkillViewTool` 当前两者合一）？倾向保留 `SkillViewTool` 现有行为。
5. **`--max-bytes` 上限默认值 5 MB 是否合理**：取决于 embedded examples.md 实际大小；实现阶段实测。
6. **是否在 inspect 文本里加 `Apply filters:` 一节**，告诉用户"如果按当前 toolset 过滤，这个 skill 会被丢掉因为 requires_tools=X"？倾向**不**加（inspect 视角不调 filters），但在 README 提示。

---

**文档结束**。本文档不实现、不提交；评审通过后再进入实现阶段。
