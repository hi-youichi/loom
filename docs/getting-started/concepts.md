# 核心概念索引

Loom 框架的核心概念导航，快速了解各个模块的功能和用法。

## 核心概念

- [State Graph](../core/state-graph.md) — 基于状态输入输出的图模型，定义智能体的执行流程
- [Node and Middleware](../core/node-middleware.md) — 图节点抽象和中间件系统，实现灵活的节点逻辑处理

## 智能体运行模式

- [ReAct Mode](../core/react.md) — 思考-行动-观察循环模式，支持工具调用的迭代推理
- [DUP Mode](../core/dup.md) — 理解后行动模式，在执行前先充分理解任务意图
- [ToT Mode](../core/tot.md) — 思想树推理模式，通过多候选探索和回溯解决复杂问题
- [GoT Mode](../core/got.md) — 思想图执行模式，将任务分解为 DAG 结构并并行处理

## 工具与集成

- [LLM Client](../core/llm-client.md) — 模型集成客户端，支持多种 LLM 提供商和配置管理
- [Tool System](../tools/tool-system.md) — 工具扩展系统，为智能体添加外部能力调用
- [MCP Integration](../tools/mcp.md) — MCP 协议集成，标准化的工具通信接口
- [Agent Orchestration](../tools/orchestration.md) — 多智能体协作，实现复杂任务的分工合作

## 内存与状态

- [Checkpointer and Store](../memory/checkpointer-store.md) — 持久化存储系统，支持检查点保存和跨会话状态管理
- [Channels](../memory/channels.md) — 状态聚合通道，实现节点间的数据流转和状态更新
- [Streaming](../streaming/streaming.md) — 实时输出流，提供执行过程的动态反馈

## 部署

- [CLI](../deployment/cli.md) — 命令行界面，直接运行和测试智能体应用
- [Bot Runtime](../deployment/bot-runtime.md) — 机器人运行时，将智能体部署到 Telegram 等平台