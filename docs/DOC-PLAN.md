# Loom 文档集规划

> 本文档是 Loom 框架面向开发者的文档集结构规划。
> 按照 agent-doc-writer-prompt.md 中定义的 4 层提示词结构执行。

---

## 1. 功能清单

| 功能域 | 核心能力 | 页面数 |
|--------|----------|--------|
| 状态图 | StateGraph、Node、Edge、Next、compile | 2 |
| Agent 运行模式 | ReAct / DUP / ToT / GoT | 4 |
| LLM 客户端 | LlmClient trait、ChatOpenAI、MockLlm | 1 |
| 工具系统 | ToolSource、BashTools、WebTools、MCP | 2 |
| 记忆与存储 | Checkpointer、Store、Channels | 1 |
| 流式输出 | StreamEvent、StreamMode、StreamWriter | 1 |
| CLI | 安装、配置、子命令、REPL | 1 |
| Bot Runtime | Headless 服务、容器化部署、Docker Compose | 1 |
| Agent 编排 | 多 Agent 协作、AgentTool、Orchestrator | 1 |

| Claude Code 兼容 | JSON 协议、Schema 类型、适配层 | 4 |

---

## 2. 文档地图

```
入门
  ├── Overview（入口页）                    [入门]
  ├── Quickstart（5 分钟跑通第一个 Agent）  [入门]
  └── 核心概念索引                         [入门]

核心概念
  ├── State Graph（状态图与编译）           [核心]
  ├── Node 与 Middleware                    [核心]
  ├── ReAct 运行模式                        [核心]
  ├── DUP 运行模式                          [核心]
  ├── ToT 运行模式                          [核心]
  ├── GoT 运行模式                          [核心]
  └── LLM 客户端与模型配置                  [核心]

工具与集成
  ├── 工具系统总览                          [工具]
  ├── MCP 集成                             [工具]
  └── Agent 编排（多 Agent 协作）           [工具]

记忆与存储
  ├── Checkpointer 与 Store                [存储]
  └── Channels（状态通道）                  [存储]

流式与可观测
  └── 流式输出与 StreamEvent               [流式]

Claude Code 兼容
  ├── JSON 协议参考                        [兼容]
  ├── JSON Schema 类型详解                 [兼容]
  ├── 兼容层设计                           [兼容]
  └── Schema Crate ADR                     [兼容]

部署与运维
  ├── CLI 安装与配置                        [部署]
  ├── Bot Runtime（Headless 部署）          [部署]
  └── 故障排查                              [部署]

进阶
  ├── API 深度参考                          [进阶]
  ├── 架构决策记录                          [进阶]
  └── 性能优化                              [进阶]
```

---

## 3. 阅读路径

```
推荐阅读顺序：

1. Quickstart
   → 先跑通一个可工作的 ReAct Agent

2. State Graph + Node 与 Middleware
   → 理解状态图的核心抽象，自定义节点

3. ReAct 运行模式
   → 最常用的运行模式，掌握 think/act/observe 循环

4. 工具系统总览 + MCP 集成
   → 给 Agent 添加工具能力

5. LLM 客户端与模型配置
   → 按需选择和配置模型

按需阅读：
- DUP / ToT / GoT → 需要更复杂推理策略时
- Checkpointer 与 Store → 需要持久化和断点续传时
- Agent 编排 → 需要多 Agent 协作时
- 流式输出 → 需要实时响应时
- Bot Runtime → 需要部署为 Headless 服务时
- CLI → 需要命令行工具时
```

---

## 4. 页面详细规划

### 入门

#### Overview（入口页）
- **模板**: 入口页（模板 1）
- **用途**: 决策导航，回答"我想做 X，从哪开始"
- **内容**: 导航表格（意图→入口→理由）、推荐阅读顺序、进阶入口
- **文件**: `docs/getting-started/overview.md`

#### Quickstart
- **模板**: 快速上手（模板 2）
- **用途**: 5 分钟跑通第一个 ReAct Agent
- **内容**: 安装 → 最小配置 → 完整可运行代码 → 验证输出
- **前置条件**: Rust 1.75+, cargo, OpenAI API Key
- **文件**: `docs/getting-started/quickstart.md`

#### 核心概念索引
- **模板**: 入口页变体
- **用途**: 一句话+链接到每个核心概念页
- **文件**: `docs/getting-started/concepts.md`

### 核心概念

#### State Graph（状态图与编译）
- **模板**: 概念页（模板 3）
- **覆盖**: StateGraph builder、add_node、add_edge、add_conditional_edges、compile、CompiledStateGraph.run
- **不覆盖**: Node 实现细节（见 Node 与 Middleware）、特定运行模式（见各模式页）
- **代码示例**: 构建→添加节点→连边→编译→运行的完整示例
- **关键决策**: 线性边 vs 条件边 vs 条件函数的选择
- **文件**: `docs/core/state-graph.md`

#### Node 与 Middleware
- **模板**: 概念页（模板 3）
- **覆盖**: Node\<S\> trait、Next 枚举（Continue/Node/End）、RunContext、NodeMiddleware
- **不覆盖**: 具体 Node 实现（见运行模式页）
- **代码示例**: 自定义 Node 实现、自定义 Middleware
- **文件**: `docs/core/node-middleware.md`

#### ReAct 运行模式
- **模板**: 概念页（模板 3）
- **覆盖**: ThinkNode → tools_condition → ActNode → ObserveNode 循环、ReActState、ReactRunner、build_react_runner
- **不覆盖**: 其他运行模式、工具实现细节
- **代码示例**: 最小 ReAct、自定义最大迭代、工具调用流程
- **最佳实践**: 单一职责 Agent、工具描述的重要性
- **文件**: `docs/core/react.md`

#### DUP 运行模式
- **模板**: 概念页（模板 3）
- **覆盖**: UnderstandNode 在 plan/act/observe 前的作用
- **不覆盖**: ReAct 基础（见 ReAct 页）
- **关键对比**: DUP vs ReAct 对比表格
- **文件**: `docs/core/dup.md`

#### ToT 运行模式
- **模板**: 概念页（模板 3）
- **覆盖**: ThinkExpandNode（多候选展开）、ThinkEvaluateNode（评估选择）
- **关键对比**: ToT vs ReAct 对比表格
- **文件**: `docs/core/tot.md`

#### GoT 运行模式
- **模板**: 概念页（模板 3）
- **覆盖**: PlanGraph（LLM 生成 DAG）、ExecuteGraph（ReAct 子任务执行）
- **关键对比**: GoT vs ToT vs ReAct 对比表格
- **文件**: `docs/core/got.md`

#### LLM 客户端与模型配置
- **模板**: 概念页（模板 3）
- **覆盖**: LlmClient trait、ChatOpenAI、MockLlm、ChatOpenAICompat、ToolChoiceMode、LlmResponse
- **不覆盖**: 具体 API 参数细节（见 API 参考页）
- **关键决策**: OpenAI vs OpenAI 兼容 vs Mock 的选择
- **文件**: `docs/core/llm-client.md`

### 工具与集成

#### 工具系统总览
- **模板**: 概念页（模板 3）
- **覆盖**: ToolSource trait、BashToolsSource、WebToolsSource、StoreToolSource、normalize_tool_output
- **不覆盖**: MCP（见 MCP 集成页）
- **关键对比**: 各 ToolSource 对比表格
- **文件**: `docs/tools/tool-system.md`

#### MCP 集成
- **模板**: 概念页（模板 3）
- **覆盖**: McpToolSource、McpToolAdapter、register_mcp_tools、MCP 配置
- **不覆盖**: 工具系统基础（见工具系统总览）
- **文件**: `docs/tools/mcp.md`

#### Agent 编排
- **模板**: 概念页（模板 3）
- **覆盖**: AgentTool（ReactRunner → Tool）、多 Agent 协作模式、Orchestrator agent
- **不覆盖**: 单 Agent 运行（见 ReAct 页）
- **代码示例**: 两个 Agent 协作的完整示例
- **文件**: `docs/tools/orchestration.md`

### 记忆与存储

#### Checkpointer 与 Store
- **模板**: 概念页（模板 3）
- **覆盖**: Checkpointer trait、MemorySaver、SqliteSaver、Store trait、InMemoryStore
- **关键对比**: 内存 vs SQLite vs LanceDB 对比表格
- **文件**: `docs/memory/checkpointer-store.md`

#### Channels（状态通道）
- **模板**: 概念页（模板 3）
- **覆盖**: LastValue、Topic、EphemeralValue、BinaryOperatorAggregate
- **关键对比**: 各 Channel 类型对比表格
- **文件**: `docs/memory/channels.md`

### 流式与可观测

#### 流式输出与 StreamEvent
- **模板**: 概念页（模板 3）
- **覆盖**: StreamEvent、StreamMode、StreamWriter、MessageChunk
- **文件**: `docs/streaming/streaming.md`

### 部署与运维

#### CLI 安装与配置
- **模板**: 快速上手变体
- **覆盖**: cargo install、config.toml 配置、.env、子命令（react/dup/tot/got/tool/models/mcp）、REPL 模式
- **文件**: `docs/deployment/cli.md`

#### Bot Runtime（Headless 部署）
- **模板**: 概念页（模板 3）
- **覆盖**: bot-runtime 架构、Docker Compose 部署、bot.toml 配置、环境变量
- **文件**: `docs/deployment/bot-runtime.md`

#### 故障排查
- **模板**: 故障排查（模板 5）
- **覆盖**: 常见错误与解决方案
- **文件**: `docs/deployment/troubleshooting.md`

### 进阶

#### API 深度参考
- **模板**: API 参考（模板 4）
- **覆盖**: 所有公开 trait 和结构体的详细签名
- **文件**: `docs/advanced/api-reference.md`

### Claude Code 兼容

#### JSON 协议参考
- **模板**: 参考文档
- **覆盖**: Claude Code CLI `--output-format stream-json` 完整协议字段说明、输入协议、事件时序
- **文件**: `docs/reference/claude-code-json-protocol.md`

#### JSON Schema 类型详解
- **模板**: 参考文档
- **覆盖**: StreamJsonEvent、ResultEnvelope、Message、ContentBlock、ApiStreamEvent 等核心类型的结构、用途和序列化约束
- **文件**: `docs/reference/claude-code-schema-types.md`

#### 兼容层设计
- **模板**: 设计文档
- **覆盖**: 三层兼容架构（消费端 / 适配层 / 服务端）、Loom → Claude Code 单向转换映射、Headless Server 设计
- **文件**: `docs/design/claude-code-compat.md`

#### Schema Crate ADR
- **模板**: 架构决策记录
- **覆盖**: `claude-code-schema` crate 的选型理由、类型设计、序列化策略
- **文件**: `docs/adr/claude-code-schema.md`

#### 架构决策记录
- **模板**: 自定义
- **覆盖**: 已有的 RFC 和设计文档索引
- **文件**: `docs/advanced/architecture.md`

#### 性能优化
- **模板**: 概念页（模板 3）
- **覆盖**: 流式 vs 非流式、SQLite 调优、工具输出截断、并发控制
- **文件**: `docs/advanced/performance.md`

---

## 5. 术语表

| 术语 | 含义 | 不要叫 |
|------|------|--------|
| State Graph | 状态图，Loom 的核心抽象 | 状态机、流程图 |
| Node | 图中的节点，接收状态并返回更新后的状态 | 步骤、处理器 |
| Edge | 节点之间的连接 | 转换、迁移 |
| Next | 节点返回的路由指令 | 路由、跳转 |
| ReAct | Think→Act→Observe 循环运行模式 | 反应模式 |
| DUP | Understand→Plan→Act→Observe 运行模式 | — |
| ToT | Tree of Thought，多候选推理 | 思维树 |
| GoT | Graph of Thought，DAG 推理 | 思维图 |
| ToolSource | 工具提供者 trait | 工具工厂 |
| Checkpointer | 状态持久化 trait | 存储器 |
| Store | 键值存储 trait | 仓库 |
| Channel | 状态通道，控制状态聚合方式 | 通道 |
| MCP | Model Context Protocol | — |
| AgentTool | 将 Agent 包装为工具 | 工具 Agent |

---

## 6. 跨页面约定

- **代码场景**: 全文使用同一示例场景——"一个能回答技术问题并执行 bash 命令的助手"
- **变量命名**: agent、graph、runner、state、config
- **链接格式**: 链接文字用页面标题，不用"这里"或"点击这里"
- **语言**: 中文文档，代码注释中英混合
- **Rust 版本**: 所有示例基于 Rust 1.75+

---

## 7. 文件结构

```
docs/
  getting-started/
    overview.md
    quickstart.md
    concepts.md
  core/
    state-graph.md
    node-middleware.md
    react.md
    dup.md
    tot.md
    got.md
    llm-client.md
  tools/
    tool-system.md
    mcp.md
    orchestration.md
  memory/
    checkpointer-store.md
    channels.md
  streaming/
    streaming.md
  deployment/
    cli.md
    bot-runtime.md
    troubleshooting.md
  advanced/
    api-reference.md
    architecture.md
    performance.md
  reference/        (协议参考)
    claude-code-json-protocol.md
    claude-code-schema-types.md
  design/           (已有，保留)
    claude-code-compat.md
  adr/              (架构决策)
    claude-code-schema.md
    act-node-refactoring.md
  dev/              (已有，保留)
  rfcs/             (已有，保留)
  DOC-PLAN.md       (本文件)
```

---

## 8. 执行优先级

第一批（核心链路，让用户能跑通并理解）：
1. Overview
2. Quickstart
3. State Graph
4. ReAct 运行模式

第二批（工具与模型）：
5. 工具系统总览
6. LLM 客户端与模型配置
7. Node 与 Middleware

第三批（进阶模式）：
8. DUP
9. ToT
10. GoT
11. Agent 编排

第四批（基础设施）：
12. MCP 集成
13. Checkpointer 与 Store
14. Channels
15. 流式输出

第五批（部署与进阶）：
16. CLI
17. Bot Runtime
18. 故障排查
19. API 参考
20. 架构决策记录
21. 性能优化
