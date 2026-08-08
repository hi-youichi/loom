# Goal loop 与 task mode（实验性）

> **实验性功能。** `loom goal` 的 autonomous goal loop，以及 `loom task` / `task-cli` 的 AI Company task mode 都属于实验性能力。它们不是生产级 scheduler，也不提供无人值守执行、可靠恢复或“状态为完成即代码正确”的保证。请只在 disposable branch 或 disposable worktree 中评估。

本文面向希望试用 Loom autonomous goal loop 或 AI Company task mode 的用户。普通 Agent session、`--session-id` 恢复和稳定 workflow 的用法不在本文重复；goal/task 与它们的边界见最后一节。

## 前置条件与隔离

需要能构建并运行 workspace 中的 `cli`，并在 `~/.loom`（或 `LOOM_HOME`）配置模型凭据。配置加载器对同一个环境变量的优先级从高到低是：已存在的 shell/process 环境变量 > 项目 `.env` > `config.toml` 中由 `[default].provider` 选中的 active provider > `config.toml` 的 `[env]`；因此，进程环境变量可能让 `.env` 或 `config.toml` 看起来没有生效。这里的优先级描述的是配置加载器；普通 CLI 启动路径并不保证每次都自动调用该环境注入流程。goal 的默认工作目录是启动命令时的当前目录，`loom task` 也以当前目录作为 Agent 的 working directory。goal 的 `--tool loom` 会在该目录运行 Loom Agent，并为 task MCP server 写入 `.loom/goal-mcp.json`；使用 `codex`、`claude`、`cursor` 或自定义命令时，外部程序同样在该目录启动。

推荐先建立一次性分支或 worktree，并确认路径：

```powershell
git switch -c experiment/loom-goal
git status --short --branch
Get-Location
```

这些命令只是隔离和检查；goal 本身不会替你创建 Git worktree。稳定的一次性 Agent CLI 支持 `--worktree`，但 `loom goal` 创建的 `RunOptions` 明确关闭了该选项，所以不要把 goal 当作自动隔离层。

## `loom goal`：外部 coding tool 驱动的循环

入口是 `loom goal [DESCRIPTION]`。不提供 description 且不提供 `--resume` 会直接报错。注意：当前实现会先在 `goal_cmd` 中创建一条 `InProgress` task（用于生成 session ID），随后 `GoalRunner::new` 又创建一条 `InProgress` task；runner、状态更新和 resume 元数据绑定后一条记录，session ID 却由前一条记录派生。因此新 goal 当前会留下两条 task 记录，这不是“一条可恢复记录”的保证；检查时应留意重复记录。默认迭代上限是 100 次。每轮会保存 goal 元数据（iteration、tool、耗时、token 使用量和最近最多 20 条 history），之后再决定是否继续。

最小试运行：

```powershell
cargo run -p cli -- goal "为 src/foo.rs 补充一个回归测试" `
  --tool loom `
  --verify "cargo test -p my-crate foo_test"
```

`--tool` 的实际选择如下：

- `loom` 是默认值。每轮使用 Loom 的 ReAct Agent，启用 `goal_mode`，并通过 task MCP server 暴露 task 管理工具。
- `codex`、`claude`、`cursor` 会被转换为 `--goal-prompt` 参数。
- 其他字符串被当作可执行命令，不自动添加参数；每轮目标提示通过环境变量 `LOOM_GOAL_PROMPT` 传入。

外部工具每轮必须成功返回；非零退出码会被视为 tool failure。源码只对已分类的失败、取消和有限的 rate-limit 情况做处理，不承诺外部工具的幂等性、自动重试或后台无人值守恢复。

### 参数边界

| 参数 | 当前语义 |
| --- | --- |
| `description` | 要实现的目标；缺失时退出。 |
| `--tool TOOL` | `loom`、已知外部 coding tool，或自定义 executable；默认 `loom`。 |
| `--resume ID` | 按完整 ID 或前缀恢复暂停的 goal；恢复时使用当前目录。不能与“新建目标”混为一谈。 |
| `--id ID` | 参数定义中存在，但当前 `goal_cmd` 没有读取它；不要把它当成可用的自定义 task ID 入口。 |
| `--model MODEL` | 仅传给 `loom` goal turns；支持裸模型名或 `provider/model` 形式。外部 shell tool 不读取此参数。 |
| `--token-budget TOKENS` | runner 会把每轮 `TurnResult.usage.total_tokens` 累加，并在累计值达到预算时返回 `UsageLimited`；这不是当前所有 goal 实际消耗的可靠硬上限，因为现有 `LoomTool` 和 `ShellTool` 都返回 `usage: None`，通常不会提供可检查的用量。未设置则没有此项检查，但仍受 100 轮上限约束。 |
| `--verify CMD` | 每轮 coding tool 完成后执行的 shell 命令；退出码 0 自动标记该 goal 为 `completed`，非 0 只表示本轮验证失败，循环继续。 |
| `--effort LEVEL` | 传给 `loom` 每轮 LLM 请求的 reasoning effort：`auto`、`none`、`minimal`、`low`、`medium`、`high`、`xhigh`。外部 shell tool 不读取它。 |
| `--verbose` | 将迭代信息写到 stderr。 |

`--verify` 的命令由 Loom 在 goal 的 working directory 执行：Windows 使用 `cmd /C`，其他平台使用 `sh -c`。它没有单独的 cost 或 timeout 参数；验证命令本身的耗时和副作用必须由用户控制。验证通过只是“该命令返回 0”，不是独立的代码审查，也不是正确性证明。

如果没有 `--verify`，goal 仍可能因 Agent 在 task MCP 中把状态更新为 `completed` 而结束。源码会据此返回 `Achieved`；因此必须再运行项目自己的测试、审查 diff 和检查运行时行为。

## 中断、恢复与检查

Ctrl-C 会取消当前运行；在取消路径中，goal runner 会保存当前 iteration 等元数据，并尝试把 runner 使用的 task 从 `in_progress` 改为 `pending`，随后退出。这是当前主要的可恢复路径，不是事务级恢复保证。达到已被观测到的 token budget、100 iterations、连续 3 次 tool failure、rate-limit 重试耗尽或其他错误时，多数分支只 cleanup 并返回错误，不会迁移状态；task 可能留下 `in_progress`，而 `--resume` 只接受 `pending`，所以不能按可恢复目标处理这些分支。`--verify` 返回 0 或 Agent 把 task 更新为 `completed` 才会写入完成状态；其他错误不会自动写入统一的 `failed`/`cancelled` 终态。

外部 shell tool 的默认 timeout 是 300 秒，但 `goal_cmd` 总是给它配置 cancellation token；有 token 时执行路径只等待子进程或取消，不执行 timeout 分支。因此外部命令可能无限等待，不能把 300 秒当作 goal 的终止保证。取消与 timeout 也不是同一条可恢复状态迁移路径。

开始前把命令和验证参数记在实验记录中：

```powershell
cargo run -p cli -- goal "修复 parser 的边界条件" `
  --tool loom `
  --model openai/gpt-4o `
  --effort medium `
  --token-budget 20000 `
  --verify "cargo test -p parser"
```

终止或命令返回后，先检查 task，而不是直接再次启动：

```powershell
cargo run -p cli -- task list --status pending
cargo run -p cli -- task list --status in_progress
cargo run -p cli -- task show <task-id-or-prefix>
```

然后检查 Git 状态和 diff：

```powershell
git status --short
git diff --check
git diff
```

恢复使用：

```powershell
cargo run -p cli -- goal --resume <task-id-or-prefix> --verbose
```

`--resume` 会从 task DB 读取 goal 元数据和 history，并以当前目录继续；不要从另一个目录恢复并假设它会回到原来的 working directory。恢复时的 tool、token budget、verify command 等来自保存的 goal 元数据；`goal_cmd` 仍以当前进程目录为 `working_dir`。恢复前应重新确认当前分支、依赖、模型配置和验证命令。

## `loom task`：CLI 中实际暴露的 task mode

`loom task` 是 CLI 内的另一条实验性路径，不是普通 `loom -m` session 的别名。当前公开命令只有：

```text
loom task new <DESCRIPTION...> [--agent <AGENT>] [--model <MODEL>]
loom task list [--status <STATUS>] [--assignee <ASSIGNEE>]
loom task show <ID-or-prefix>
loom task continue <ID-or-prefix> [--agent <AGENT>]
```

实际行为：

- `task new` 将 task 建为 `in_progress`，默认 assignee/agent 为 `ceo`，打印短 ID，然后立即进入交互式 Agent 模式。 `--model` 只作用于这次新建任务的初始运行。
- `task list` 最多查询 50 条，按 `created_at` 降序显示短 ID、status、assignee 和 name；status 只能是 `pending`、`in_progress`、`completed`、`cancelled`，非法值不会成为有效筛选器。
- `task show` 显示完整 ID、名称、状态、assignee、start time、created at 和 description。
- `task continue` 按 ID 或前缀读取任务，在当前目录用指定 agent（默认 `ceo`）重新开始交互式运行。该命令不接受 model 参数，也没有记录并恢复原始 working directory 的字段。

task ID 是 SQLite 中生成的 UUID；`show`/`continue` 接受完整 UUID 或前缀。前缀匹配多个任务会报 ambiguous，检查候选后使用更长前缀或完整 ID。 `assignee` 只是 task 记录和本次 Agent profile 的选择，不等同于外部人员、队列消费者或 scheduler。

CLI 内置 task DB 位于 `<LOOM_HOME>/tasks/tasks.db`，默认即 `~/.loom/tasks/tasks.db`；goal 也使用这套 DB。可用 `LOOM_HOME` 明确隔离实验数据。 `loom task` 的 Ctrl-C 通过 `RunCancellation` 终止运行；再次继续前务必使用 `task list`、`task show`、`git status` 和测试检查实际留下的状态与改动。

## 独立的 `task-cli` 与 task MCP server

workspace 还包含实验性 crate `experimental/task/task-cli` 和 `experimental/task/task-mcp-server`。它们不是 `loom task new/list/show/continue` 的同一套命令：

```powershell
cargo run -p task-cli -- --work-folder . create --name "实验任务" --assignee ceo
cargo run -p task-cli -- --work-folder . list --status pending --limit 20 --page 1
cargo run -p task-cli -- --work-folder . show <task-id-or-prefix>
cargo run -p task-cli -- --work-folder . update <task-id-or-prefix> --status completed
```

独立 `task-cli` 使用 `<work-folder>/tasks.db`（默认当前目录），提供 `create`、`show`、`list`、`update`、`delete`，输出 `{ok: ..., data/error: ...}` JSON；帮助文本声称 ID 前缀至少 4 字符，但当前 `task-core` 的查询实现没有执行这个最小长度校验，实际不会强制拒绝短前缀（仍可能因 0 或多个匹配而失败）。它的状态模型是 `pending | in_progress | completed | cancelled`，但这些字段仍只是 SQLite 记录。

`task-mcp-server --db-path <PATH>` 通过 stdio 暴露 `task_create`、`task_show`、`task_list`、`task_update`、`task_delete`。goal 的 `--tool loom` 会为 goal 所用的 `<LOOM_HOME>/tasks/tasks.db` 生成配置；默认 `<LOOM_HOME>` 是 `~/.loom`，设置 `LOOM_HOME` 后应以解析后的实际路径为准，并启动该 server。server 只是 task CRUD 接口，不会自行调度任务、执行代码或证明结果。

当前源码中这两个 crate 的入口是 `src/main.rs`；请求中所列的 `experimental/task/task-cli/src/lib.rs` 和 `task-mcp-server/src/lib.rs` 并不存在，不应据此假设有 library API。

## 状态不是正确性证明

`in_progress` 只表示 runner/CLI 将 task 置于进行中；当前源码只有显式取消等调用 `save_paused_state` 的路径会把 goal 保存为 `pending`；`completed` 可能来自 `--verify` 返回 0，也可能来自 Agent 通过 task 工具更新状态。错误、预算/迭代耗尽和失败上限目前不统一迁移到可恢复或明确失败终态，所以必须先用 `task show` 核对实际状态。即使 CLI 输出 `goal achieved` 或 task 状态为 `completed`，也不能推断：

- 所有需求都已实现；
- 工作树没有未预期改动；
- 测试覆盖了目标；
- 代码经过人工 review；
- 依赖、生成物或外部系统状态正确。

最低限度的验收顺序是：查看 `task show`，查看 `git diff`，运行与目标相关的显式测试，再做人工 review。 `--verify` 应选择窄而可重复的命令，例如 `cargo test -p <crate> <test-filter>`；它只把 exit code 0 当作成功，不能替代这些检查。

## 与稳定 session/workflow 的边界与已知限制

- 普通 `loom -m ...` / `--session-id ...` 是对话 session 连续性入口；goal 则是一个以 task 记录、外部 coding tool 和迭代验证为中心的实验 loop。不要把 goal 的 task history 当作普通 session transcript。
- workflow background execute 的契约是 `workflow_start` 立即返回 receipt，再用 `workflow_status` 查询终态；它与 `loom goal` 的逐轮 coding loop、`loom task` 的交互式 REPL 是不同机制。不要用本文 task 状态代替 workflow instance status。
- goal 的恢复入口是 task ID 前缀，且使用当前目录；源码没有持久化并自动切换原始 Git branch/worktree 的逻辑。
- token budget 是 runner 对已填充的 `TurnResult.usage` 做的累计 token 检查，不是金钱 cost 上限；当前 Loom goal 工具没有填充该 usage，因此不能把 `--token-budget` 当作当前实现可靠的硬上限。当前 CLI 没有 `--cost-budget`；如果需要成本控制，应在 provider 侧或实验外部记录实际账单和用量。
- 任务 DB、Agent 输出、外部 coding tool 的工作树修改和验证命令都可能失败或留下部分结果。文档和源码没有提供生产级 scheduler、可靠恢复、自动回滚或无人值守保证。

把 goal/task 当作可观察的实验 harness：限制目录和 token，保留日志与 diff，验证失败就停下来检查；确认结果前不要合并或把状态发布为完成。
