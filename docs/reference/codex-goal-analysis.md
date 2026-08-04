# Codex Goal 系统分析

> 基于 OpenAI Codex CLI 源代码（`codex-rs`）的完整分析，涵盖 `/goal` 斜杠命令的架构、数据流、状态机、记账系统和交互设计。
>
> 用于指导 Loom 的 `/goal` 斜杠命令实现。

**创建时间**：2025-08-25｜**最后更新**：2025-08-25

---

## 目录

1. [架构总览](#1-架构总览)
2. [数据模型](#2-数据模型)
3. [状态机](#3-状态机)
4. [核心流程](#4-核心流程)
5. [Model 工具](#5-model-工具)
6. [外部 API](#6-外部-api)
7. [记账系统](#7-记账系统)
8. [Prompt 系统](#8-prompt-系统)
9. [TUI 交互](#9-tui-交互)
10. [关键设计原则](#10-关键设计原则)
11. [文件清单](#11-文件清单)

---

## 1. 架构总览

Codex Goal 系统分 6 层，自底向上：

```
┌──────────────────────────────────────────────────────────────┐
│  TUI 层 (tui/)                                              │
│  /goal 斜杠命令解析、状态栏显示、摘要面板、编辑对话框       │
│  slash_command.rs / goal_menu.rs / goal_status.rs           │
│  goal_display.rs / goal_files.rs                            │
├──────────────────────────────────────────────────────────────┤
│  扩展层 (ext/goal/)                                         │
│  生命周期钩子、运行时、Model 工具、记账、提示注入、事件     │
│  extension.rs / runtime.rs / api.rs / tool.rs               │
│  accounting.rs / steering.rs / events.rs                    │
│  analytics.rs / metrics.rs / spec.rs                        │
├──────────────────────────────────────────────────────────────┤
│  Prompt 层 (prompts/)                                       │
│  continuation / budget_limit / objective_updated 三个模板   │
│  goals.rs + templates/goals/continuation.md                 │
│  templates/goals/budget_limit.md + objective_updated.md     │
├──────────────────────────────────────────────────────────────┤
│  State 层 (state/)                                          │
│  GoalStore — 1728 行，CRUD + accounting 原子操作            │
│  runtime/goals.rs                                           │
├──────────────────────────────────────────────────────────────┤
│  SQLite 层                                                  │
│  thread_goals 表 + thread_goal_continuation_deferrals 表   │
│  state/goals_migrations/*.sql                               │
├──────────────────────────────────────────────────────────────┤
│  协议层 (protocol/)                                         │
│  ThreadGoal / ThreadGoalStatus / ThreadGoalUpdatedEvent     │
└──────────────────────────────────────────────────────────────┘
```

### 1.1 模块职责

| 模块 | 路径 | 行数 | 职责 |
|---|---|---|---|
| **GoalStore** | `state/src/runtime/goals.rs` | 1728 | SQLite 数据访问（CRUD + accounting） |
| **GoalExtension** | `ext/goal/src/extension.rs` | ~300 | 6 个生命周期钩子 + 3 个 Model 工具注册 |
| **GoalRuntimeHandle** | `ext/goal/src/runtime.rs` | ~400 | 运行时核心：记账、continuation、steering |
| **GoalAccountingState** | `ext/goal/src/accounting.rs` | ~350 | 内存记账：token + 墙钟时间 |
| **GoalToolExecutor** | `ext/goal/src/tool.rs` | ~350 | 3 个 Model 工具的实现 |
| **GoalService** | `ext/goal/src/api.rs` | ~300 | 外部 API（TUI/IDE 调用） |
| **GoalEventEmitter** | `ext/goal/src/events.rs` | ~50 | 事件发射 |
| **GoalSteering** | `ext/goal/src/steering.rs` | ~100 | 提示注入 |
| **GoalAnalytics** | `ext/goal/src/analytics.rs` | ~100 | 分析事件追踪 |
| **GoalMetrics** | `ext/goal/src/metrics.rs` | ~100 | OTel 指标 |
| **GoalSpec** | `ext/goal/src/spec.rs` | ~120 | 工具 JSON Schema 定义 |
| **GoalPrompts** | `prompts/src/goals.rs` | ~100 | Prompt 模板渲染 |
| **GoalPromptsTemplates** | `prompts/templates/goals/*.md` | 3 个 | Prompt 模板文件 |
| **SlashCommand** | `tui/src/slash_command.rs` | ~15 | /goal 命令注册 |
| **GoalMenu** | `tui/src/chatwidget/goal_menu.rs` | ~150 | 交互：摘要/编辑/pause/resume |
| **GoalStatus** | `tui/src/chatwidget/goal_status.rs` | ~150 | 状态栏指示器 |
| **GoalDisplay** | `tui/src/goal_display.rs` | ~100 | 格式化显示 |
| **GoalFiles** | `tui/src/goal_files.rs` | ~200 | 大目标文件化 |

---

## 2. 数据模型

### 2.1 SQLite 表结构

```sql
-- 主表
CREATE TABLE thread_goals (
    thread_id TEXT PRIMARY KEY NOT NULL,       -- 线程 ID
    goal_id TEXT NOT NULL,                     -- UUID，每次替换生成新 ID
    objective TEXT NOT NULL,                   -- 目标描述
    status TEXT NOT NULL CHECK(status IN (
        'active', 'paused', 'blocked',
        'usage_limited', 'budget_limited', 'complete'
    )),
    token_budget INTEGER,                      -- 可选的 token 预算
    tokens_used INTEGER NOT NULL DEFAULT 0,    -- 已用 token
    time_used_seconds INTEGER NOT NULL DEFAULT 0, -- 已用时间（秒）
    created_at_ms INTEGER NOT NULL,            -- 创建时间戳
    updated_at_ms INTEGER NOT NULL             -- 更新时间戳
);

-- 延迟 continuation 表（防止自动续跑）
CREATE TABLE thread_goal_continuation_deferrals (
    thread_id TEXT PRIMARY KEY NOT NULL
    REFERENCES thread_goals(thread_id) ON DELETE CASCADE
);
```

### 2.2 协议层结构

```rust
pub struct ThreadGoal {
    pub thread_id: String,
    pub objective: String,
    pub status: ThreadGoalStatus,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
```

### 2.3 状态枚举

```rust
pub enum ThreadGoalStatus {
    Active,        // 活跃（正在运行）
    Paused,        // 暂停（用户控制）
    Blocked,       // 卡住（model 标记，需 3 轮重复阻塞）
    UsageLimited,  // 用量超限（系统自动）
    BudgetLimited, // 预算用尽（系统自动）
    Complete,      // 完成（model 标记）
}
```

---

## 3. 状态机

### 3.1 状态转移图

```
                    ┌─────────────────────────────────────────┐
                    │                                         │
                    v                                         │
  (无 goal) ──> Active ──> Paused ──> Active                  │
                    │        │         │                       │
                    │        │         └───────────────────────┘
                    │        │
                    │        └──> Blocked ──> Active
                    │                  │
                    │                  └──> (用户清除)
                    │
                    ├──> BudgetLimited (token_budget 耗尽)
                    │         │
                    │         ├──> Complete (model 标记完成)
                    │         └──> (用户清除)
                    │
                    ├──> UsageLimited (系统用量超限)
                    │         │
                    │         ├──> Active (用户 resume)
                    │         └──> (用户清除)
                    │
                    └──> Complete (model 调用 update_goal)
                              │
                              ├──> Active (新 goal 替换，需 insert)
                              └──> (用户清除)
```

### 3.2 状态控制权

| 状态 | 进入方式 | 控制者 |
|---|---|---|
| `Active` | model 创建、用户 resume | model 活跃运行 |
| `Paused` | 用户 /goal pause | 用户 |
| `Blocked` | model 调用 `update_goal(status: blocked)` | model（需 3 轮校验） |
| `UsageLimited` | 系统自动（turn error 时） | 系统 |
| `BudgetLimited` | 系统自动（token 超 budget） | 系统 |
| `Complete` | model 调用 `update_goal(status: complete)` | model |

### 3.3 不对称控制规则

**Model 可以做的事情**：
- `create_goal` — 创建新 goal（仅当旧 goal 为 `complete` 或无 goal 时）
- `update_goal(status: complete)` — 标记完成
- `update_goal(status: blocked)` — 标记阻塞（3 轮校验）

**Model 不可以做的事情**：
- ~~不能 pause/resume~~
- ~~不能设置 budget~~
- ~~不能清除 goal~~

**用户/系统可以做的事情**：
- `/goal <description>` — 设置新 goal
- `/goal pause` — 暂停
- `/goal resume` — 恢复
- `/goal edit` — 编辑 objective
- `/goal clear` — 清除
- 系统自动 → `BudgetLimited` / `UsageLimited`

---

## 4. 核心流程

### 4.1 Goal 创建流程（model 发起）

```
model 调用 create_goal(objective, token_budget?)
  │
  ├─→ 验证 objective 长度、格式
  ├─→ 验证 token_budget 为正数
  ├─→ GoalStore.insert_thread_goal()
  │     └─→ INSERT ... ON CONFLICT ... WHERE status = 'complete'
  │           └─→ 如果旧 goal 不是 complete，返回 None（创建失败）
  ├─→ 填充空 thread preview
  ├─→ 标记当前 turn 的 goal active
  ├─→ 发出 ThreadGoalUpdated 事件
  └─→ 返回 goal + remaining_tokens
```

### 4.2 Goal 创建流程（用户通过 /goal 发起）

```
用户输入 /goal 完成项目迁移
  │
  ├─→ GoalService.set_thread_goal(thread_id, objective, status=Active)
  │     ├─→ 获取 goal_state_permit（防止并发 mutation）
  │     ├─→ prepare_external_goal_mutation()
  │     │     ├─→ 如果当前有活跃 turn，先 account progress
  │     │     └─→ 如果空闲，account idle progress
  │     ├─→ 如果有旧 goal，update（带 expected_goal_id）
  │     └─→ 如果没有旧 goal，replace（全新创建）
  ├─→ apply_external_goal_set()
  │     ├─→ 如果 objective 变更，注入 objective_updated_steering
  │     └─→ 如果 goal 为 active，尝试 continue_if_idle()
  └─→ 返回 GoalSetOutcome → 发送 ThreadGoalUpdated 事件
```

### 4.3 自动 Continuation 流程

```
线程空闲（on_thread_idle 钩子）
  │
  ├─→ 获取 goal_state_permit（信号量串行化）
  ├─→ 检查 continuation_deferral（有则跳过）
  ├─→ 读取当前 goal
  ├─→ 检查 goal.status == Active
  ├─→ 构建 continuation_steering_item（注入 completion audit prompt）
  ├─→ thread.try_start_turn_if_idle(vec![item])
  │     └─→ 启动新 turn，item 作为系统提示注入
  └─→ 检查当前 turn 是否关联了 goal
        └─→ 如果否，clear_active_goal()
```

### 4.4 Progress Accounting 流程

```
触发时机：
  1. on_tool_finish — 每次工具调用完成
  2. on_turn_stop — turn 结束
  3. on_turn_abort — turn 中止
  4. prepare_external_goal_mutation — 外部修改

流程：
  │
  ├─→ 获取 progress_accounting_permit（Semaphore 1，串行化）
  ├─→ 取 ProgressSnapshot
  │     ├─→ token_delta = current - last_accounted
  │     └─→ time_delta = wall_clock elapsed
  ├─→ GoalStore.account_thread_goal_usage()
  │     └─→ UPDATE tokens_used += delta, time_used_seconds += delta
  │         ├─→ 如果 token_budget 不为空且 tokens_used >= budget
  │         │     └─→ status = 'budget_limited'
  │         └─→ RETURNING 更新后的行
  ├─→ 如果 status 变为 BudgetLimited
  │     └─→ 注入 budget_limit_steering_item（引导收尾）
  ├─→ 更新内存基线
  └─→ 发出 ThreadGoalUpdated 事件
```

### 4.5 Goal 完成审计流程

```
model 标记 complete 前，continuation prompt 强制执行：

  1. 从 objective 和引用的文件中推导具体需求
  2. 保留原始范围，不重新定义成功标准
  3. 对每个需求：
     ├─→ 识别权威证据来源（文件、命令输出、测试结果等）
     ├─→ 检查当前状态是否满足
     └─→ 分类：已证明 / 矛盾 / 不完整 / 证据不足 / 缺失
  4. 验证范围必须匹配需求范围（窄检查不能支撑宽结论）
  5. 测试/检查结果仅当确认覆盖了相关需求才视为证据
  6. 不确定的证据视为未完成

只有全部需求都被当前证据证明满足，才可调用 update_goal(complete)。
```

### 4.6 Blocked 审计流程

```
model 标记 blocked 需要满足：
  ├─× 第一次遇到阻塞 → 不能标记
  ├─× 工作困难/缓慢/不确定 → 不能标记
  ├─× 需要澄清 → 不能标记
  ├─× 预算用尽 → 不能标记
  └─● 同一阻塞条件重复 ≥ 3 轮（含原始 turn + 自动 continuation）
       └─→ 用户 resume 后，重新开始 3 轮计数
```

---

## 5. Model 工具

### 5.1 工具定义

3 个工具，均在 `ext/goal/src/spec.rs` 中定义：

```rust
pub const GET_GOAL_TOOL_NAME: &str = "get_goal";
pub const CREATE_GOAL_TOOL_NAME: &str = "create_goal";
pub const UPDATE_GOAL_TOOL_NAME: &str = "update_goal";
```

### 5.2 get_goal

- **描述**：获取当前 thread 的 goal，包括状态、预算、token 和时间用量
- **参数**：无
- **返回**：`GoalToolResponse { goal, remaining_tokens }`

### 5.3 create_goal

- **描述**：创建新 goal（仅当用户/系统明确要求时）
- **参数**：
  - `objective`（必填）：具体目标
  - `token_budget`（可选）：正整数 token 预算
- **限制**：旧 goal 必须是 `complete` 状态才允许替换
- **返回**：`GoalToolResponse { goal, remaining_tokens }`

### 5.4 update_goal

- **描述**：更新现有 goal 状态
- **参数**：
  - `status`（必填）：只能是 `complete` 或 `blocked`
- **限制**：
  - 不能 pause/resume/budget-limit/usage-limit
  - `complete` 时返回 `completion_budget_report`
  - `blocked` 需要 3 轮重复阻塞校验

### 5.5 工具返回结构

```rust
struct GoalToolResponse {
    goal: Option<ThreadGoal>,
    remaining_tokens: Option<i64>,          // 剩余 token 预算
    completion_budget_report: Option<String>, // 完成时包含 token 使用总结
}
```

---

## 6. 外部 API

### 6.1 GoalService

`ext/goal/src/api.rs` 中的 `GoalService` 提供外部（TUI/IDE）调用的 API：

```rust
impl GoalService {
    // 获取 goal
    pub async fn get_thread_goal(&self, state_db, thread_id) -> Result<Option<ThreadGoal>>

    // 设置 goal（创建/替换/更新）
    pub async fn set_thread_goal(&self, state_db, request: GoalSetRequest) -> GoalSetOutcome

    // 清除 goal
    pub async fn clear_thread_goal(&self, state_db, thread_id) -> Result<bool>

    // 恢复线程运行时
    pub async fn restore_thread_runtime_after_resume(&self, thread_id) -> Result<()>

    // 在 fork 前 flush 进度
    pub async fn flush_thread_goal_progress_for_fork(&self, thread_id) -> Result<()>
}
```

### 6.2 GoalSetRequest

```rust
pub struct GoalSetRequest<'a> {
    pub thread_id: ThreadId,
    pub objective: GoalObjectiveUpdate<'a>,  // Keep 或 Set(&str)
    pub status: Option<ThreadGoalStatus>,
    pub token_budget: GoalTokenBudgetUpdate,  // Keep 或 Set(Option<i64>)
}
```

### 6.3 并发安全

- `goal_state_permit`（Semaphore 1）：确保外部 mutation 和 idle continuation 不会交错
- `progress_accounting_permit`（Semaphore 1）：确保 tool finish 和 turn stop 不会并发写入
- `expected_goal_id`：stale update protection，防止并发覆盖

---

## 7. 记账系统

### 7.1 内存结构

```rust
struct GoalAccountingState {
    inner: Mutex<GoalAccountingInner>,
    progress_accounting_lock: Semaphore,  // 串行化 progress accounting
}

struct GoalAccountingInner {
    current_turn_id: Option<String>,
    turns: HashMap<String, GoalTurnAccounting>,   // 每个 turn 的记账
    wall_clock: GoalWallClockAccounting,           // 墙钟时间追踪
    budget_limit_reported_goal_id: Option<String>, // 防止重复注入
}

struct GoalTurnAccounting {
    current_token_usage: TokenUsage,           // 当前 token 用量
    last_accounted_token_usage: TokenUsage,    // 上次记账时的基线
    active_goal_id: Option<String>,            // 当前 turn 关联的 goal
    account_tokens: bool,                      // 是否记账 token（Plan mode 不记）
}
```

### 7.2 Token 计算公式

```rust
pub fn goal_token_delta_for_usage(usage: &TokenUsage) -> i64 {
    usage.input_tokens
        .saturating_sub(usage.cached_input_tokens)  // 仅计算非缓存 input
        .saturating_add(usage.output_tokens.max(0))  // output token
}
```

### 7.3 墙钟时间追踪

```rust
struct GoalWallClockAccounting {
    last_accounted_at: Instant,  // 上次记账时的时间点
    active_goal_id: Option<String>,
}
```

- 每次 mark_active_goal 时重置基线
- 每次 accounting 时计算 `elapsed_since_last_accounted`
- 不计入 idle 时间（仅当 goal active 时计时）

### 7.4 AccountingMode

```rust
pub enum GoalAccountingMode {
    ActiveStatusOnly,    // 仅活跃状态（'active'）
    ActiveOnly,          // 活跃 + budget_limited
    ActiveOrComplete,    // 活跃 + budget_limited + complete（完成时结算）
    ActiveOrStopped,     // 所有非 complete 状态（暂停/阻塞后结算）
}
```

---

## 8. Prompt 系统

### 8.1 Continuation Prompt

**文件名**：`prompts/templates/goals/continuation.md`

**用途**：自动 continuation 时注入，引导模型继续工作。

**核心指令**：

1. **保持范围完整**：不要缩小 objective，不要重新定义成功标准
2. **基于证据工作**：检查当前状态，不依赖历史记忆
3. **Fidelity**：对齐 = 向最终状态移动，不是最小稳定子集
4. **完成审计**：标记 complete 前必须逐项验证
5. **Blocked 审计**：3 轮重复阻塞才能标记
6. **进度可视化**：如果 `update_plan` 可用，用 plan 展示多步工作

### 8.2 Budget Limit Prompt

**文件名**：`prompts/templates/goals/budget_limit.md`

**用途**：预算用尽时注入，引导模型优雅收尾。

**核心指令**：
- 不开始新实质性工作
- 总结有用进度
- 指出剩余工作或阻塞
- 给用户清晰的下一步

### 8.3 Objective Updated Prompt

**文件名**：`prompts/templates/goals/objective_updated.md`

**用途**：用户编辑 goal objective 时注入。

**核心指令**：
- 新 objective 覆盖旧的
- 调整当前 turn 方向
- 不要继续做只服务于旧目标的工作

### 8.4 注入机制

所有 prompt 通过 `InternalModelContextFragment` 注入：

```rust
pub(crate) fn continuation_steering_item(goal: &ThreadGoal) -> ResponseItem {
    ContextualUserFragment::into(InternalModelContextFragment::new(
        InternalContextSource::from_static("goal"),
        prompt_text,
    ))
}
```

注入源标记为 `goal`，模型知道这是系统级别的提示。

---

## 9. TUI 交互

### 9.1 /goal 斜杠命令

```rust
pub enum SlashCommand {
    Goal,  // 支持 inline args
}
```

| 命令 | 行为 |
|---|---|
| `/goal` | 显示当前 goal 摘要 |
| `/goal <description>` | 设置新 goal |
| `/goal edit` | 弹出编辑框修改 objective |
| `/goal pause` | 暂停 (active → paused) |
| `/goal resume` | 恢复 (paused/blocked/usage_limited → active) |
| `/goal clear` | 清除 goal |

### 9.2 状态摘要显示

```
Goal
────────────────────────────────────────
Status: active
Objective: 将项目从 JS 迁移到 TS
Time used: 2m
Tokens used: 12.5K
Token budget: 50K

Commands: /goal edit, /goal pause, /goal clear
```

### 9.3 状态栏指示器

在 TUI 底部状态栏显示紧凑状态：

| 状态 | 显示 | 示例 |
|---|---|---|
| Active | 活跃，带用时或 budget | `12.5K / 50K` 或 `2m` |
| Paused | 暂停 | `paused` |
| Blocked | 卡住 | `stalled` |
| UsageLimited | 用量超限 | `usage limited` |
| BudgetLimited | 预算用尽 | `limited by budget` |
| Complete | 完成，带用时或 token | `40K tokens` 或 `10h 12m` |

### 9.4 大目标文件化

当 objective 超过 `MAX_THREAD_GOAL_OBJECTIVE_CHARS` 时，自动写入文件：

```
Read the Codex goal objective file at /path/to/attachments/{uuid}/goal-objective.md before continuing.
```

---

## 10. 关键设计原则

### 10.1 不对称控制

Model 只能创建和完成，不能 pause/resume/budget。这是最核心的安全设计。

### 10.2 Stale Update Protection

每次替换生成新 UUID `goal_id`，更新时传入 `expected_goal_id` 校验。防止并发会话覆盖。

### 10.3 Budget 软停止

超预算时不中断正在执行的 turn，而是：
1. 将状态设为 `BudgetLimited`
2. 注入 steering prompt 引导模型优雅收尾
3. 不开始新工作，但允许完成当前工作

### 10.4 完成审计

标记 complete 前必须逐项验证，不能凭"看起来完成了"就标记。审计指令内嵌在 continuation prompt 中，每次都会触发。

### 10.5 Blocked 审计

防止模型过早放弃。同一阻塞条件必须重复 3 轮才允许标记 blocked。

### 10.6 Accounting 串行化

`Semaphore(1)` 确保 tool finish 和 turn stop 不会并发写入同一个 goal 的 progress。

### 10.7 事件驱动

所有状态变更通过 `ThreadGoalUpdatedEvent` 通知 TUI 更新，保持 UI 与状态同步。

### 10.8 Idle Continuation

`on_thread_idle` 钩子自动启动新 turn，无需用户手动触发。但通过 `continuation_deferral` 表提供退出机制。

---

## 11. 文件清单

### State 层

| 文件 | 行数 | 说明 |
|---|---|---|
| `state/src/runtime/goals.rs` | 1728 | GoalStore — CRUD + accounting 原子操作 |

### 扩展层

| 文件 | 行数 | 说明 |
|---|---|---|
| `ext/goal/src/extension.rs` | ~300 | 生命周期钩子 + 工具注册 |
| `ext/goal/src/runtime.rs` | ~400 | 运行时核心 |
| `ext/goal/src/accounting.rs` | ~350 | 内存记账 |
| `ext/goal/src/tool.rs` | ~350 | Model 工具实现 |
| `ext/goal/src/api.rs` | ~300 | 外部 API |
| `ext/goal/src/spec.rs` | ~120 | 工具 JSON Schema |
| `ext/goal/src/steering.rs` | ~100 | 提示注入 |
| `ext/goal/src/events.rs` | ~50 | 事件发射 |
| `ext/goal/src/analytics.rs` | ~100 | 分析事件 |
| `ext/goal/src/metrics.rs` | ~100 | OTel 指标 |

### Prompt 层

| 文件 | 说明 |
|---|---|
| `prompts/src/goals.rs` | Prompt 模板渲染 |
| `prompts/templates/goals/continuation.md` | 自动 continuation 提示 |
| `prompts/templates/goals/budget_limit.md` | 预算超限提示 |
| `prompts/templates/goals/objective_updated.md` | 目标更新提示 |

### TUI 层

| 文件 | 说明 |
|---|---|
| `tui/src/slash_command.rs` | `/goal` 命令注册 |
| `tui/src/chatwidget/goal_menu.rs` | goal 摘要/编辑/交互 |
| `tui/src/chatwidget/goal_status.rs` | 状态栏指示器 |
| `tui/src/goal_display.rs` | 格式化显示 |
| `tui/src/goal_files.rs` | 大目标文件化 |

### 数据库

| 文件 | 说明 |
|---|---|
| `state/goals_migrations/*.sql` | 表结构定义 |