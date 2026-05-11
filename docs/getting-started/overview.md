---
sidebar_position: 1
title: "Loom 框架概览"
description: "图智能代理框架"
---

# Loom 框架概览
图智能代理框架

## 快速导航

| 如果你想要 | 阅读此文 | 原因 |
|-----------|----------|------|
| 快速上手基础 Agent | [快速入门](./quickstart.md) | 学习 ReAct 模式和基本工具使用 |
| 构建复杂多步骤应用 | [DUP 模式](../core/dup.md) | 理解 Understand-Plan-Act-Observe 流程 |
| 实现多 Agent 协作 | [ReAct 模式](../core/react.md) | 掌握 ReAct 运行器和工具使用 |
| 部署 Headless 服务 | [Bot 部署](../deployment/cli.md) | 了解 CLI 和容器化部署流程 |
| 深度思考和推理 | [ToT 模式](../core/tot.md) / [GoT 模式](../core/got.md) | 探索树状和图状思维算法 |
| 集成外部工具和服务 | [Skills 系统](../skills.md) | 扩展 Agent 能力和生态系统 |

## 推荐阅读顺序

1. **[快速入门](./quickstart.md)** - 学习 StateGraph 基础和 ReAct 模式
2. **[Skills 系统](../skills.md)** - 掌握工具集成和扩展
3. **[内存管理](../memory/channels.md)** - 理解 Channels 和 Checkpointer
4. **[DUP 模式](../core/dup.md)** - 深入复杂任务规划
5. **[ReAct 模式](../core/react.md)** - 掌握 Agent 运行器
6. **[CLI 部署](../deployment/cli.md)** - 生产环境部署

## 高级内容

- **[LLM 集成](../core/llm-client.md)** - LlmClient trait、ChatOpenAI、MockLlm
- **[流式处理](../core/react.md)** - StreamEvent 和 StreamWriter 实时处理
- **[Node 中间件](../core/node-middleware.md)** - 节点拦截器和中间件
- **[API 参考](../advanced/api-reference.md)** - 完整 API 文档