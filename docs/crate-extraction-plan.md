# Loom Crate 拆分方案

> 版本: 0.2.1 | 日期: 2025-08-19 | 最后更新: 2025-06-03

## 1. 背景

当前 workspace 有 19 个 crate，但核心 `loom` crate 承载了 268 个文件、65,315 行代码（agent 模式、工具系统、LSP、memory、压缩等），是一个典型的 "god crate"。本文档描述将其拆分为职责清晰的独立 crate 的方案。

**注**: `loom-llm` 的拆分已先于本计划完成（8,463 行，29 文件已迁移），`loom/src/llm/` 现为 ~76 行 re-export 薄壳。

## 2. 现有 Workspace 结构

```
loom_main/
├── loom/              # 核心框架 (268 文件，待拆分)
├── loom-llm/          # LLM 客户端抽象
├── loom-graph/        # 图执行原语
├── loom-pregel/       # Pregel BSP 运行时
├── loom-curator/      # 后台审查系统
├── loom-skill/        # Skill 发现/追踪
├── config/            # 配置管理 (env_config)
├── stream-event/      # 事件协议
├── model-spec-core/   # 模型/Provider 规格
├── loom-workspace/    # Workspace/Thread 关联
├── loom-acp/          # Agent Client Protocol 服务
├── cli/               # CLI 入口
├── serve/             # WebSocket 服务
├── telegram-bot/      # Telegram Bot
├── task-core/         # 任务管理核心
├── task-cli/          # 任务 CLI
├── task-mcp-server/   # 任务 MCP 服务
└── loom-examples/     # 示例
```

## 3. `loom` Crate 内部模块分析

```
loom/src/                               # 268 文件, 65,315 行
├── agent/             # 42 文件,  8,638 行 — Agent 模式 (ReAct, DUP, GoT, ToT)
│   ├── react/         # ReAct 模式 (Think → Act → Observe)
│   ├── dup/           # Depth-Up 推理
│   ├── got/           # Graph-of-Thoughts
│   └── tot/           # Tree-of-Thoughts
├── tools/             # 56 文件, 11,411 行 — 工具实现
│   ├── bash/          # Shell 执行
│   ├── file/          # 文件操作
│   ├── web/           # Web 抓取
│   ├── memory/        # 长期记忆工具
│   ├── shared/        # 共享工具基础设施
│   ├── lsp/           # LSP 工具
│   ├── telegram/      # Telegram 集成
│   ├── twitter/       # Twitter 集成
│   └── ...
├── tool_source/       # 17 文件,  2,983 行 — 工具抽象层 & MCP 集成
├── memory/            # 17 文件,  7,185 行 — 持久化 & 向量存储
├── background_review/ # 13 文件,  5,696 行 — 后执行分析
├── stream_display/    # 10 文件,  4,367 行 — 流式展示
├── lsp/               # 14 文件,  3,914 行 — LSP 客户端集成
├── cli_run/           #  3 文件,  3,628 行 — CLI 编排
├── model_spec/        #  9 文件,  1,264 行 — 模型规格解析
├── openai_sse/        #  4 文件,    900 行 — SSE 处理
├── compress/          #  7 文件,  1,007 行 — 上下文压缩
├── stream/            #  9 文件,    978 行 — 流类型
├── helve/             #  4 文件,  1,091 行 — 产品语义配置
├── worktree/          #  3 文件,  1,169 行 — Git worktree 隔离
├── protocol/          #  6 文件,  1,860 行 — 通信协议
├── state/             #  3 文件,  1,853 行 — State 类型
├── goal_runner/       #  6 文件,  1,527 行 — Goal 运行器
├── tier/              #  5 文件,    479 行 — Tier 分辨
├── prompts/           #  3 文件,    408 行 — Prompt 模板
├── export/            #  1 文件,    427 行 — 导出功能
├── config/            #  6 文件,    388 行 — 配置摘要
├── user_message/      #  2 文件,    362 行 — 用户消息存储
├── command/           #  4 文件,    319 行 — Slash 命令
├── profile_convert/   #  5 文件,    386 行 — 配置文件转换
├── services/          #  2 文件,    177 行 — 模型服务
├── cache/             #  3 文件,    251 行 — 缓存层
├── provider/          #  2 文件,     23 行 — Provider 配置
├── llm/               #  3 文件,    719 行 — LLM 集成封装 (re-export 薄壳)
└── 根文件             #  8 文件,  1,903 行 — lib.rs, traits.rs, skill.rs 等
```

## 4. 模块耦合度分析

### 自包含模块 (低耦合，易提取)

| 模块 | 文件数 | 行数 | 外部依赖 |
|---|---|---|---|
| `lsp/` | 14 | 3,914 | state, tools (最少) |
| `cache/` | 3 | 251 | llm, state |
| `command/` | 4 | 319 | state, message |
| `model_spec/` + `tier/` + `provider/` + `services/` | 18 | 1,943 | model-spec-core, loom-llm |
| `prompts/` + `helve/` | 7 | 1,499 | config, model-spec |
| `worktree/` | 3 | 1,169 | file tools |
| `profile_convert/` | 5 | 386 | config |

### 中等耦合模块 (需定义 trait 接口)

| 模块 | 文件数 | 行数 | 外部依赖 |
|---|---|---|---|
| `memory/` | 17 | 7,185 | state, llm, graph, rusqlite |
| `compress/` | 7 | 1,007 | state, tools, graph |
| `protocol/` + `stream_display/` | 16 | 6,227 | stream-event, state |
| `stream/` + `openai_sse/` | 13 | 1,878 | stream-event, state |
| `export/` + `user_message/` | 3 | 789 | state, llm |

### 高耦合模块 (需核心 trait 解耦)

| 模块 | 文件数 | 行数 | 外部依赖 |
|---|---|---|---|
| `tools/` + `tool_source/` | 73 | 14,394 | state, llm, memory, graph |
| `agent/` | 42 | 8,638 | llm, graph, tools, memory, state |
| `background_review/` | 13 | 5,696 | agent, memory, tools, state |
| `cli_run/` + `goal_runner/` | 9 | 5,155 | agent, tools, config, state |

## 5. 拆分方案

不需要创建额外的 "核心类型" crate。采用**依赖倒置**原则：

- 每个提取出的 crate 自带所需的 trait 和类型定义
- `loom` 保持为 facade，负责实现各 crate 定义的 trait 并组装
- 依赖方向始终单向：子 crate 不依赖 `loom`，`loom` 依赖子 crate

**示例**: `loom-tools` 需要访问 State 时，不是抽一个共享 State crate，而是在 `loom-tools` 中定义 `ToolContext` trait，由 `loom` 在组装时传入具体实现。

### Phase 0: 已完成 (先于本计划)

| # | Crate | 来源模块 | 文件数 | 行数 | 状态 |
|---|---|---|---|---|---|
| 0 | `loom-llm` | 原 `llm/` 完整实现 | 29 | 8,463 | `[x]` 已完成 |

**说明**: `loom-llm` 通过依赖注入 (`with_tools(Vec<ToolSpec>)`) 解决了 `ToolSource` trait 循环依赖问题。`loom/src/llm/` 现为 ~719 行薄壳（含 574 行 `model_registry.rs` 运行时逻辑）。

### Phase 1: 低风险提取

| 新 Crate | 来源模块 | 文件数 | 行数 | 依赖 |
|---|---|---|---|---|
| `loom-lsp` | `lsp/` | 14 | 3,914 | loom-llm, lsp-types, jsonrpc-core |
| `loom-model-spec` | `model_spec/`, `tier/`, `provider/`, `services/` | 18 | 1,943 | model-spec-core, loom-llm |
| `loom-cache` | `cache/` | 3 | 251 | loom-llm |
| `loom-commands` | `command/` | 4 | 319 | stream-event |

### Phase 2: 中风险提取

| 新 Crate | 来源模块 | 文件数 | 行数 | 依赖 |
|---|---|---|---|---|
| `loom-memory` | `memory/` | 17 | 7,185 | loom-graph, loom-llm, rusqlite, sqlite-vec |
| `loom-prompts` | `prompts/`, `helve/` | 7 | 1,499 | loom-model-spec, serde_yaml |
| `loom-compress` | `compress/` | 7 | 1,007 | loom-graph, loom-llm |
| `loom-worktree` | `worktree/` | 3 | 1,169 | (无外部 crate 依赖) |
| `loom-protocol` | `protocol/`, `stream_display/` | 16 | 6,227 | stream-event |
| `loom-stream` | `stream/`, `openai_sse/` | 13 | 1,878 | stream-event |

### Phase 3: 核心提取

#### 4a. `loom-tools` (73 文件, 14,394 行)

```
loom-tools/
├── src/
│   ├── lib.rs
│   ├── bash/          # Shell 工具
│   ├── powershell/    # PowerShell 工具
│   ├── file/          # 文件操作
│   ├── web/           # Web 抓取
│   ├── memory/        # 记忆工具
│   ├── shared/        # 共享基础设施
│   ├── telegram/      # Telegram 集成
│   ├── twitter/       # Twitter 集成
│   ├── skill/         # Skill 执行
│   ├── task/          # Task 工具
│   ├── todo/          # Todo 工具
│   ├── conversation/  # 对话工具
│   └── tool_source/   # 工具注册 & MCP 集成
└── Cargo.toml
```

**依赖**: loom-llm, loom-memory, mcp_client, mcp_core

`loom-tools` 定义 `Tool`/`ToolContext` 等 trait，`loom` 在组装时提供具体实现。

#### 4b. `loom-agent-patterns` (42 文件, 8,638 行)

```
loom-agent-patterns/
├── src/
│   ├── lib.rs
│   ├── react/         # ReAct 模式
│   ├── dup/           # Depth-Up 推理
│   ├── got/           # Graph-of-Thoughts
│   └── tot/           # Tree-of-Thoughts
└── Cargo.toml
```

**依赖**: loom-graph, loom-llm, loom-tools, loom-memory

### Phase 4: 应用层清理

| 新 Crate | 来源 | 说明 |
|---|---|---|
| `loom-background-review` | `background_review/` (13 文件, 5,696 行) | 依赖所有核心 crate，最后提取 |
| 精简后 `loom` | 胶水代码 + re-export + 核心类型 | 仅做组装和重新导出 |

**注意**: `cli_run/` (3,628 行) 和 `goal_runner/` (1,527 行) 属于应用层编排逻辑，将保留在 `loom` facade 或归入 `cli` crate。

## 6. 最终 Crate 依赖图

```
                    loom (facade, 含 State/Error/Message 等核心类型)
                   /    |    \      \
              loom-llm  |  loom-graph  stream-event
                  |     |       |          |
             loom-memory    loom-pregel    |
               |   |                      |
         loom-tools  loom-model-spec      |
            |                             |
    loom-agent-patterns                   |
            |                             |
    loom-background-review                |
            |                             |
        loom (facade)                     \
        /    |    \                   loom-protocol
     cli   serve  telegram-bot
```

## 7. 最终 Workspace 成员清单

```
workspace members = [
    # 核心类型
    "loom",            # facade (胶水 + re-export + State/Error/Message 等核心类型)

    # 基础设施
    "loom-llm",
    "loom-graph",
    "loom-pregel",
    "stream-event",
    "config",
    "model-spec-core",

    # 功能层
    "loom-memory",
    "loom-model-spec",
    "loom-tools",
    "loom-agent-patterns",
    "loom-lsp",
    "loom-cache",
    "loom-commands",
    "loom-prompts",
    "loom-compress",
    "loom-worktree",
    "loom-protocol",
    "loom-background-review",
    "loom-curator",
    "loom-skill",
    "loom-workspace",
    "loom-workspace/gh",

    # 应用层
    "cli",
    "serve",
    "loom-acp",
    "telegram-bot",
    "task-core",
    "task-cli",
    "task-mcp-server",
    "loom-examples",
]
```

## 8. 任务进度表

> 状态说明: `[ ]` 未开始 | `[~]` 进行中 | `[x]` 已完成 | `[-]` 已取消

### Phase 0: 已完成 (先于本计划)

| # | 任务 | 新 Crate | 来源模块 | 行数 | 状态 | 备注 |
|---|---|---|---|---|---|---|
| 0 | LLM 客户端抽象 | `loom-llm` | 原 `llm/` 完整实现 | 8,463 | `[x]` | 依赖注入解耦 ToolSource |

### Phase 1: 低风险提取

| # | 任务 | 新 Crate | 来源模块 | 行数 | 状态 | 备注 |
|---|---|---|---|---|---|---|
| 1 | 提取 LSP 客户端 | `loom-lsp` | `lsp/` | 3,914 | `[x]` | ✅ 完成，46 个测试通过 |
| 2 | 提取模型规格 | `loom-model-spec` | `model_spec/` | 1,264 | `[x]` | ✅ 完成，28 个测试通过 (tier/provider/services 因耦合保留在 loom) |
| 3 | 提取缓存层 | `loom-cache` | `cache/` | 251 | `[x]` | ✅ 完成，5 个测试通过 |
| 4 | 提取 Slash 命令 | `loom-commands` | `command/` | 319 | `[-]` | ⏸ 推迟到 Phase 2+，依赖 compress/llm/error/message |

### Phase 2: 中风险提取

| # | 任务 | 新 Crate | 来源模块 | 行数 | 状态 | 备注 |
|---|---|---|---|---|---|---|
| 5 | 提取持久化层 | `loom-memory` | `memory/` | 7,185 | `[ ]` | 中风险，需定义存储 trait |
| 6 | 提取 Prompt 模板 | `loom-prompts` | `prompts/`, `helve/` | 1,499 | `[ ]` | 中风险 |
| 7 | 提取通信协议 | `loom-protocol` | `protocol/`, `stream_display/` | 6,227 | `[ ]` | 中风险 |
| 8 | 提取上下文压缩 | `loom-compress` | `compress/` | 1,007 | `[ ]` | 中风险，依赖 graph |
| 9 | 提取 Git Worktree | `loom-worktree` | `worktree/` | 1,169 | `[ ]` | 中风险 |
| 9b | 提取流式处理 | `loom-stream` | `stream/`, `openai_sse/` | 1,878 | `[ ]` | 中风险，依赖 stream-event |

### Phase 3: 核心提取

| # | 任务 | 新 Crate | 来源模块 | 行数 | 状态 | 备注 |
|---|---|---|---|---|---|---|
| 10 | 提取工具系统 | `loom-tools` | `tools/`, `tool_source/` | 14,394 | `[ ]` | 高风险，需定义 Tool trait |
| 11 | 提取 Agent 模式 | `loom-agent-patterns` | `agent/` | 8,638 | `[ ]` | 高风险，核心逻辑 |

### Phase 4: 应用层清理

| # | 任务 | 新 Crate | 来源模块 | 行数 | 状态 | 备注 |
|---|---|---|---|---|---|---|
| 12 | 提取后台审查 | `loom-background-review` | `background_review/` | 5,696 | `[ ]` | 高风险，依赖所有核心 crate |
| 13 | 精简 loom 为 facade | `loom` (瘦身) | 删除已提取代码 | — | `[ ]` | 保留 re-export + 核心类型 |

### 汇总

| Phase | 任务数 | 行数 | 已完成 | 进行中 | 未开始 |
|---|---|---|---|---|---|
| Phase 0 | 1 | 8,463 | 1 | 0 | 0 |
| Phase 1 | 4 | 6,427 | 3 | 0 | 1 |
| Phase 2 | 6 | 17,965 | 0 | 0 | 6 |
| Phase 3 | 2 | 23,032 | 0 | 0 | 2 |
| Phase 4 | 2 | 5,696+ | 0 | 0 | 2 |
| **总计** | **15** | **61,583+** | **4** | **0** | **11** |

## 9. 风险与注意事项

- **API 兼容性**: 所有新 crate 通过 `pub use` 在 `loom` 中重新导出，确保下游 crate (cli, serve, telegram-bot) 零改动
- **Feature flag**: `loom` 的 feature `lance` 和 `testing` 需要透传到对应子 crate
- **循环依赖**: 注意 `loom-tools` ↔ `loom-memory` 之间的潜在循环，通过依赖倒置（在各自 crate 中定义所需 trait）解耦
- **编译时间**: 拆分后增量编译会更快，但首次全量编译时间可能略增
- **测试**: 每个 crate 提取后立即运行 `cargo test --workspace` 确保无回归
