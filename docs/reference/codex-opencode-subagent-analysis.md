# Codex 与 OpenCode 子代理实现分析

> **状态**：源码调研完成（未做运行时行为验证）
> **日期**：2026-08-18
> **调研范围**：本机检出的 `C:\Users\heycj\dev\codex\codex-rs` 与 `C:\Users\heycj\dev\opencode\packages\opencode\src`。本文只讨论由 LLM 工具调用派生的协作子代理，不涵盖自动审批、标题生成、压缩等内部用途 Agent。
> **相关 Loom 文档**：[ACP 子代理契约设计](../design/acp-subagent-contract.md)

---

## 目录

1. [结论摘要](#1-结论摘要)
2. [比较边界与术语](#2-比较边界与术语)
3. [Codex：线程化的协作运行时](#3-codex线程化的协作运行时)
4. [OpenCode：会话化的 task 执行](#4-opencode会话化的-task-执行)
5. [逐维度对比](#5-逐维度对比)
6. [关键时序与失败路径](#6-关键时序与失败路径)
7. [对 Loom 的启示](#7-对-loom-的启示)
8. [源码索引与未验证项](#8-源码索引与未验证项)

---

## 1. 结论摘要

两者都将子代理落为独立的、持久化的对话执行单元，并让子单元复用正常 Agent 的模型与工具调用管线；因此子代理不是一个「在父 prompt 内递归调用模型」的轻量函数。

区别在于抽象边界：**Codex 的中心是多 Agent 协作运行时**，子代理是可独立寻址、可通信、可观察、可限流的 thread；**OpenCode 的中心是 session runtime**，子代理是由 `task` 工具创建并驱动的 child session。前者优先解决并发调度与 Agent 间通信，后者优先复用既有 session、权限和 UI 数据模型。

| 结论 | Codex | OpenCode |
|---|---|---|
| 默认调用语义 | `spawn_agent` 创建后立即返回，子线程异步运行 | `task` 默认等待子会话完成并返回结果 |
| 后台模型 | 原生模型：创建、消息、follow-up、wait、interrupt 都是独立工具 | 实验性 `background: true`，依赖 `BackgroundJob` |
| 上下文策略 | 支持空白派生或完整历史 fork | 不复制父历史；把任务 prompt 送入新/恢复的 child session |
| 身份与拓扑 | thread id、agent path、nickname、role、父子边 | session id、`parentID`、agent 名称 |
| 结果回传 | watcher 订阅 child status，向父 mailbox/Agent 消息投递完成结果 | 同步时 tool output 直接返回；后台时 synthetic prompt 注入父 session |
| 并发保护 | 深度 + spawn slot + V2 execution limiter/residency | 深度限制；本次调研范围内未见相同的全局 active-subagent 配额 |

对 Loom 而言，推荐的方向不是二选一：以 OpenCode 的 `parentID`、具名 profile、权限模板和前端契约为外部数据模型；以 Codex 的独立调度、完成通知、可寻址协作和容量控制为内部运行时模型。

## 2. 比较边界与术语

### 2.1 本文中的「子代理」

本文仅覆盖以下两条模型可调用路径：

```text
Codex:    LLM function call `spawn_agent` → child thread
OpenCode: LLM tool call `task` → child session
```

两者均支持子代理使用常规模型、工具、消息持久化与取消逻辑。它们不是 MCP server，也不是一个静态工作流节点；MCP 工具只是 child 运行时可见工具的一部分。

### 2.2 对齐维度

比较按同一生命周期展开：Agent 发现 → 工具暴露 → 调用授权 → 创建与上下文 → 执行与取消 → 完成与回传 → 限流、权限和可观测性。源码事实与设计推断分开描述；未实际启动两个项目验证的行为在 §8 单独列出。

## 3. Codex：线程化的协作运行时

### 3.1 工具面：一组协作原语，而不是单个 task

Codex 在 `multi_agent_v1` / V2 命名空间暴露协作工具。除了 `spawn_agent`，还包括对既有 agent 的消息、follow-up、wait、列举和中断。工具 schema 将任务名、父历史 fork、角色、模型、reasoning effort 与 service tier 放在 spawn 参数中；V2 还以 canonical task path 表示 Agent 的层级身份。

这使父 Agent 可以在创建后继续工作、等待状态变化、给同一个 Agent 追加上下文或中断重定向，而无需将一次子任务等价为「必须等待的一次函数调用」。工具定义见 `core/src/tools/handlers/multi_agents_spec.rs`。

### 3.2 创建路径

`handle_spawn_agent` 的完整路径如下：

```text
模型调用 spawn_agent
  → 解析 message/items，拒绝二者同时给出或空输入
  → 按父 session source 推导 child depth，检查 agent_max_depth
  → 记录 CollabAgentToolCall(InProgress) 到父 turn
  → 基于当前 turn 构造 child Config
      ├─ 当前模型、provider、reasoning
      ├─ developer instructions / V2 subagent instructions
      ├─ approval policy、permission profile、cwd、environment
      └─ 可选 model / role / service tier 覆盖
  → AgentControl::spawn_agent_with_metadata
  → 创建新 thread 或完整历史 fork thread
  → 投递 initial input，更新父 turn 的 completed tool item
  → 返回 child thread id、nickname / canonical task path
```

spawn handler 在创建前记录协作 tool item，创建后将实际生效的 model、reasoning、receiver thread 和状态回填，因而 UI/rollout history 能看到「谁派生了谁」，而非仅有一段文本输出。

### 3.3 配置、上下文与隔离

子代理配置从**当前 turn 的有效运行时配置**构建，而不是简单 clone 某个旧 session config。`build_agent_spawn_config` 会刷新 model/provider/reasoning、开发者指令、approval policy、permission profile、cwd 以及选中的 environment；这样父 turn 刚切换模型、沙箱或工作目录时，child 不会继承过期快照。

普通 spawn 创建全新的子 thread。若 `fork_context` 为真，`AgentControl::spawn_forked_thread` 则从父 rollout 筛选可继承历史，生成 fork thread。完整历史 fork 明确禁止覆盖 agent type：child 应继承父的 agent type，避免同一历史在不同角色语义下被重解释。

`SessionSource::SubAgent(SubAgentSource::ThreadSpawn { parent_thread_id, depth, agent_path, agent_role, ... })` 是父子关系的核心载体。它在创建阶段与 metadata 一同写入，并用来决定深度、agent path、通知路由与分析归因。

### 3.4 调度与容量控制

Codex 不是只做 depth guard。`AgentControl::spawn_agent_internal` 先确定 multi-agent 版本，再申请 spawn reservation；V2 resident child 还会占用 residency slot。创建新 thread 后 reservation 才 commit，从而避免并发 spawn 抢占昵称、agent path 或 thread 配额。

V2 对 `SessionSource::SubAgent` 还启用 `AgentExecutionLimiter`：活动子线程数由原子计数追踪，达到 `max_threads` 时拒绝新的执行启动。该限制的 guard 在执行结束时 `Drop`，将 active 计数归还。它把「线程存在」与「线程实际占用模型执行容量」分成两个层面。

### 3.5 通信、完成与取消

`send_input` 可以对 child 发起新的 turn；若其已有活跃 turn，则底层 `start_or_steer_turn` 可转为 steer。V2 还存在携带 source/target agent path 的 `InterAgentCommunication`，允许在协作树中传递更结构化的结果消息。

child 创建后，`maybe_start_completion_watcher` 启动 detached Tokio watcher 订阅 child `AgentStatus`。当状态变为 Completed、Errored 或 Interrupted：

- V2 且存在 agent path 时，格式化 result communication，投递到父 Agent 的 mailbox；
- 其他路径则将格式化的子代理完成通知作为用户消息注入父 thread，但不直接启动 turn。

父 session 的 input queue 有 pending mailbox 队列和「当前 turn 是否接受 mailbox delivery」状态。因此结果既可以在适当的 turn 边界被消费，也不会在已经回答的 turn 中无序插入。

### 3.6 Codex 的设计取舍

优点是长期协作能力强：child 是一等运行时实体，可等待、重用、转向、限流并留下完整拓扑与事件历史。代价是调度层复杂，必须管理 thread reservation、状态订阅、fork 历史裁剪、mailbox delivery 与 V1/V2 兼容分支。

## 4. OpenCode：会话化的 task 执行

### 4.1 Agent 目录与 task tool 暴露

OpenCode 的子代理首先是配置中的具名 `Agent.Info`：其 `mode` 为 `primary`、`subagent` 或 `all`，并包含描述、系统 prompt、权限、可选模型以及可见性。内置 `general` 是可执行多步骤工作的通用 subagent，`explore` 则用 deny-all 再白名单方式限制为检索/读取类能力。

对调用者而言只有一个 `task` tool。ToolRegistry 构造工具描述时实时枚举 `mode !== "primary"` 的 agent，再按调用者的 `task` permission 过滤、排序，将「可选 agent 名称 + 描述」拼入 task 工具描述。这意味着 agent 配置既是执行策略，也是 LLM 的子代理发现目录。

### 4.2 创建与执行路径

```text
模型调用 task(description, prompt, subagent_type, task_id?, background?)
  → 读取父 session，沿 parentID 向上计数并校验 subagent_depth
  → 对 task:<subagent_type> 发起 permission ask
  → 按名称查 Agent.Info
  → 为 child 计算 session permission
  → 若 task_id 存在则恢复旧 session，否则 Session.create(parentID=父 session)
  → 从当前 assistant message 选择 model / variant（agent 可以覆盖 model）
  → 调用 SessionPrompt.prompt(child session, agent, prompt parts)
  → 取末尾 text part 为 task output
```

`TaskTool.execute` 显式要求调用来源提供 `promptOps`；其 `runTask` 将 prompt 模板解析为 parts 后调用同一个 `SessionPrompt.prompt()`。因此 child 的模型循环、工具调用、流式事件、持久化和取消，不需要另写一套子代理 runner。

`task_id` 是「续用」而不是 fork：它取得旧 child session 后再次向该 session prompt，所以既有 child history 得以保留；新 session 并不会获得完整的父 conversation history。

### 4.3 权限模型

子 session 的权限由 `deriveSubagentSessionPermission` 计算：

1. 保留父 session 中所有 deny 规则和 `external_directory` 规则；
2. 其余能力主要由被选择 subagent 自己的 permission ruleset 决定；
3. 若 subagent 未显式声明 `todowrite` 或 `task`，为 child session 追加对应 deny。

这不是「父 Agent 的所有 allow 权限向下传递」模型。它让 agent profile 作为能力边界，同时确保父 session 的禁止项不被 child 绕过。默认 deny `task` 也使嵌套派生必须显式授权；即使配置放开，仍要通过 `subagent_depth`。

### 4.4 同步、后台与结果注入

OpenCode 默认同步：TaskTool 创建 background job 后立即等待其结果；完成时返回 `<task>` 样式的渲染 output，错误或取消转为 tool error。调用方的模型回合因此可以直接把结果纳入后续推理。

后台运行是 feature flag `OPENCODE_EXPERIMENTAL_BACKGROUND_SUBAGENTS` 保护的实验功能。`background: true` 时，工具立即返回 running 状态；`BackgroundJob` 完成 watcher 通过 `inject` 调用父 session 的 `prompt()`，写入 synthetic text，其中包含 child session id、状态和输出。这给父 Agent 一个新的可处理输入，而不是让模型轮询 child。

同一 child session 若已由 BackgroundJob 运行，后续 task 调用可 `extend` 其 job，向运行中的 child 追加上下文。同步等待中监听父 tool abort；中断时同时 cancel child prompt 与 background job。

### 4.5 事件与 UI 数据模型

在正常 LLM 工具调用路径，`SessionTools.resolve` 将 tool context 绑定到当前 session/message/call id，并把 permission ask 归属到该 tool call。`TaskTool` 通过 `ctx.metadata` 写入 `parentSessionId`、`sessionId`、model 和后台 job 信息；这些元数据与 `parentID` 共同让 UI 将 task tool call 与 child session 建立稳定关联。

此外 `SessionPrompt.handleSubtask` 允许上游以 `SubtaskPart` 触发相同的 TaskTool 执行：它会创建 assistant message 与 running tool part、执行前后触发 plugin hook，并在结束时回写 completed/error part。说明 `task` 不只是一个模型 function schema，也是 session timeline 中的一等 tool lifecycle。

### 4.6 OpenCode 的设计取舍

优点是概念和实现复用度高：一份 session 生命周期、一个 task tool、一个 agent 配置目录即可承载同步与后台子任务。代价是协作控制面较窄：在本次范围中没有 Codex 那样的通用 `send_message`、`followup_task`、`wait`、`list_agents`、`interrupt_agent` 工具族；异步工作需要借助 BackgroundJob 与 synthetic prompt 重新进入父 Agent loop。

## 5. 逐维度对比

| 维度 | Codex | OpenCode | 影响 |
|---|---|---|---|
| 子代理抽象 | `AgentControl` 管理的 thread + `SessionSource` | `Session` 的 child record + Agent.Info | Codex 更适合通信树；OpenCode 更贴近会话树/UI |
| LLM 暴露 | 多个协作 function tools | 单一 `task` tool，描述内列可选类型 | 前者更可编排，后者模型工具面更小 |
| 发现与注册 | roles、tool spec、AgentControl metadata | Agent registry + permission-filtered task description | OpenCode 的角色目录更声明式 |
| 上下文 | 新 thread 或完整历史 fork | 新/恢复 child session，传显式任务 prompt | fork 适合并行审阅；新会话更节省上下文 |
| 模型选择 | 默认继承当前 turn，可按 spawn 覆盖 | Agent 模型覆盖，否则继承当前 assistant message model | 都支持专门模型，但作用点不同 |
| 权限 | 刷新父 turn 的审批、profile、sandbox、cwd | 父 deny 向下传递，child profile 决定 allow | Codex 偏运行时一致性；OpenCode 偏最小授权角色 |
| 嵌套 | depth 检查；V2 可继续构建协作树 | 默认 deny task，显式放开后仍受 depth 限制 | OpenCode 默认更保守 |
| 并发 | reservation、V2 residency、execution limiter | BackgroundJob 支持并行；未见等价总配额 | Loom 应单独实现全局容量控制 |
| 回传 | status watcher → mailbox/agent message | 同步 tool output；后台 synthetic parent prompt | Codex 可不依赖父等待；OpenCode 结果更自然进入会话历史 |
| 取消 | interrupt/steer 与 thread 生命周期分离 | AbortSignal → cancel child prompt + job | 都有取消，Codex 更适合长驻 agent |
| 可观测性 | CollabAgentToolCall、agent status、父子边、analytics | tool part metadata、child session、plugin hooks | 两者的外部契约都应带稳定 session/thread id |

## 6. 关键时序与失败路径

### 6.1 Codex：非阻塞派生与完成回传

```text
Parent turn             AgentControl                Child thread
    | spawn_agent()           |                          |
    |------------------------>| reserve slot/depth       |
    |                         | create/fork thread       |
    |                         |------------------------->| start initial turn
    |<------------------------| child id + Running        |
    | continue own work       |                          | model/tool loop
    |                         |<-------------------------| final AgentStatus
    |                         | completion watcher        |
    |<------------------------| mailbox/result message    |
    | consume at turn boundary|                          |
```

创建失败会被映射为 tool error，并把父 turn 内的协作 tool item 完结为失败状态。若 child 完成 watcher 无法获取 parent thread 或状态不是 final，则不会虚构完成消息；这是避免把不完整结果写回父 history 的必要条件。

### 6.2 OpenCode：同步 task 与后台提升

```text
Parent session              TaskTool                  Child session
     | task()                  |                           |
     |------------------------>| permission/depth/create   |
     |                         |-------------------------->| SessionPrompt.prompt()
     |                         |                           | model/tool loop
sync |                         |<--------------------------| completed text
     |<------------------------| tool output               |
     |                         |                           |
bg   |<------------------------| running(sessionId, jobId) |
     | continue parent loop    |                           |
     |                         |<--------------------------| completion
     |<------------------------| synthetic prompt injection|
```

后台 flag 未启用时，`background: true` 直接失败，而不是静默降级为同步调用。同步等待中父 tool 被 abort 时，release 分支取消 child session 和 BackgroundJob；这防止父停止后子任务成为孤儿执行。

### 6.3 需要在 Loom 中维持的三个不变量

1. 每个子代理必须有稳定 child session/thread id，并与父 tool call 的 metadata 同时落盘；不能依赖时间窗口猜测关联。
2. 每个已创建子代理都应恰有一个可观察的终态：completed、failed 或 cancelled；父会话不能因缺失终态而永久 waiting。
3. 权限、深度、全局并发与取消均须在运行时强制，而不是仅在 prompt 中要求模型遵守。

## 7. 对 Loom 的启示

现有 [ACP 子代理契约设计](../design/acp-subagent-contract.md) 已覆盖 OpenCode 前端兼容所需的 `parentID`、tool metadata、`agent/list` 和级联删除。本次调研补充了下列优先级建议。

### 7.1 P0：稳定拓扑与结果生命周期

- 在 agent spawn 前分配稳定子 session id；在父 `agent` tool 的 start/update metadata 中同步写入 `parentSessionId`、`subSessionId`、profile/agent、model 和状态。
- 子代理完成、失败、取消都应生成明确终态事件；对后台任务，使用父 session 的 mailbox 或 synthetic message 自动通知，不要求父模型轮询。
- session repository 保存 `parent_id`，删除父节点时事务化递归清理 children，避免 checkpoint / ACP metadata 形成孤儿。

### 7.2 P1：将 profile 变成真实能力边界

- 为 profile 增加 `primary | subagent | all` 模式，并将可派生 profile 放入 `agent/list` / tool description。
- 子会话不得仅因为父 agent 有某项 allow 权限而自动获得该能力；至少继承父 deny 与目录边界，allow 应由 child profile 显式声明。
- 默认禁止子代理再派生与写全局 todo，只有 profile 显式配置时才解除；再用深度限制作第二道防线。

### 7.3 P2：从「后台 registry」升级为调度器

- 将并发上限做成 workspace/运行时级的 semaphore 或 reservation，而不是单个 tool 调用的 timeout。
- 区分已创建、排队、运行、完成、失败、取消六类状态；容量耗尽时返回结构化可恢复错误。
- 为 future 的 `send_message`、`followup`、`wait`、`interrupt` 预留 stable agent handle。初版不必一次暴露完整 Codex 工具族，但内部数据模型不应只保存一个 task output 字符串。

### 7.4 不建议照搬的部分

- 不要在第一期引入 Codex V1/V2 双协议、复杂 fork 历史裁剪或 residency eviction；Loom 当前首先需要稳定 ACP 可观测性。
- 不要把 full-history fork 设为默认。它会显著放大 token 与敏感上下文传播；应作为明确参数，且在 profile/权限边界下评估。
- 不要仅依赖 tool output 文本解析 child id。它应是对 ACP metadata 缺失时的兼容 fallback，而不是唯一关联机制。

## 8. 源码索引与未验证项

### 8.1 Codex 源码索引

| 文件 | 关键符号 | 本文用途 |
|---|---|---|
| `codex-rs/core/src/tools/handlers/multi_agents_spec.rs` | `create_spawn_agent_tool_v1/v2` | 多代理工具 schema 与模型暴露 |
| `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs` | `handle_spawn_agent` | 参数解析、depth、配置构建、tool lifecycle |
| `codex-rs/core/src/tools/handlers/multi_agents_common.rs` | `build_agent_spawn_config` | turn runtime 配置向 child 的继承 |
| `codex-rs/core/src/agent/control/spawn.rs` | `spawn_agent_internal`、`spawn_forked_thread` | reservation、创建/fork、初始输入 |
| `codex-rs/core/src/agent/control/execution.rs` | `AgentExecutionLimiter` | V2 运行容量限制 |
| `codex-rs/core/src/agent/control.rs` | `send_input`、`maybe_start_completion_watcher` | steer、完成订阅与回传 |
| `codex-rs/core/src/session/input_queue.rs` | mailbox queue | 结果在父 turn 的投递语义 |

### 8.2 OpenCode 源码索引

| 文件 | 关键符号 | 本文用途 |
|---|---|---|
| `packages/opencode/src/agent/agent.ts` | `Agent.Info`、内置 `general` / `explore` | profile、mode、默认权限 |
| `packages/opencode/src/tool/registry.ts` | `describeTask`、`tools` | 角色发现与 LLM tool 注入 |
| `packages/opencode/src/tool/task.ts` | `TaskTool.execute` | 创建、续用、后台、取消、输出 |
| `packages/opencode/src/agent/subagent-permissions.ts` | `deriveSubagentSessionPermission` | 子会话权限派生 |
| `packages/opencode/src/session/tools.ts` | `resolve` | tool context、权限和模型适配 |
| `packages/opencode/src/session/prompt.ts` | `handleSubtask`、`ops` | task 的 session lifecycle 与 plugin hooks |

### 8.3 未验证项

本文依据源码静态追踪，尚未在本机实际运行两套项目。下列问题需要集成测试或运行期 trace 确认：

- Codex V1 与 V2 在 feature flag 的不同组合下，实际默认启用的工具和容量策略；
- OpenCode `BackgroundJob` 在进程崩溃、父 session 被删除或多次 `extend` 时的持久化/恢复行为；
- 两者在 provider timeout、网络中断和重复 cancellation 下终态事件是否仍严格闭合；
- Codex fork 历史裁剪在多模态 input、MCP result 和长上下文压缩后的精确边界。

这些不确定项不影响本文对主创建链路、权限构成、状态回传路径和抽象边界的结论；若 Loom 准备实现 P2 调度器，应先以最小 mock provider 补一组端到端验证。
