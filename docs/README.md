# Loom — 自进化 AI Agent 框架

> 包裹在 AI Coding CLI（Loom / Codex）外面的进化层。给它加上**记忆**、**技能**、**进化**三种能力，越用越好。

## 你想做什么？

- 🆕 **我是新用户** → [5 分钟快速开始](getting-started/quickstart.md)
- 🔧 **我想集成 Loom 到我的项目** → [核心概念](getting-started/concepts.md) + [架构设计](design/architecture.md)
- 🚀 **我在部署/运维 Loom** → [CLI 命令参考](guide/cli.md) + [部署指南](deployment/cli.md)
- 🧠 **我想了解进化系统** → [后台审查](evolution/review.md) + [技能系统](evolution/skills.md) + [GEPA 优化](evolution/gepa.md)
- 🏗️ **我在开发 Loom 本身** → [技术栈](dev/tech-stack.md) + [模块概览](MODULES.md) + [编码规范](CODING_STANDARDS.md)
- 🐛 **我遇到了问题** → [故障排查](deployment/troubleshooting.md) + [常见 Bug](bugs/)

## TL;DR

你照常用 `loom chat` 写代码，Loom 在后台帮你：
- 记住你的偏好和项目上下文（**记忆**）
- 沉淀可复用的工作流（**技能**）
- 自动优化技能质量（**进化**）

## 核心概念

| 概念 | 是什么 | 存在哪 |
|------|--------|--------|
| **记忆** | 跨会话的用户偏好和项目事实 | `memory/USER.md`、`memory/PROJECT.md` |
| **技能** | 可复用的工作流（步骤+陷阱） | `skills/auto/<name>/SKILL.md` |
| **会话** | 完整对话记录（可搜索） | `sessions/*.jsonl` + SQLite FTS5 |
| **Review** | 会话结束后，AI 审查是否值得记住 | 自动运行，异步不阻塞 |
| **进化** | 用 GEPA 自动优化技能质量 | `loom-evolution` crate，可选装 |

## 会话生命周期

```
loom chat
  ├─ 1. 组装上下文 ─→ 注入记忆+技能到系统 prompt
  ├─ 2. 启动底层 CLI ─→ 透传 stdin/stdout，录制对话
  ├─ 3. 会话结束 ─→ 保存到 memory.db，还原 context 文件
  └─ 4. 后台 Review ─→ AI 判断是否更新记忆/技能（异步）
```

## 文档导航

### 🚀 使用 → [guide/](guide/)
- [getting-started.md](guide/getting-started.md) — 安装、初始化、第一次会话
- [cli.md](guide/cli.md) — 全部命令参考
- [config.md](guide/config.md) — 配置文件说明
- [backends.md](guide/backends.md) — Loom vs Codex 切换指南
- [hide-executing-tool.md](guide/hide-executing-tool.md) — 隐藏工具执行中间消息

### 🏗️ 设计 → [design/](design/)
- [architecture.md](design/architecture.md) — 三层架构设计
- [session-lifecycle.md](design/session-lifecycle.md) — 会话全流程
- [data-format.md](design/data-format.md) — 数据格式
- [decisions.md](design/decisions.md) — 关键设计决策
- [ai-company.md](design/ai-company.md) — AI Company 组织设计
- [hermes-vs-loom-comparison.md](design/hermes-vs-loom-comparison.md) — Hermes vs Loom 对比
- [evolution-comparison.md](design/evolution-comparison.md) — 进化系统对比
- [meta-agent-architecture.md](design/meta-agent-architecture.md) — 元 Agent 架构
- [task-integration.md](design/task-integration.md) — Task 系统集成
- [acp-background-review.md](design/acp-background-review.md) — ACP 后台审查
- [acp-goal-support.md](design/acp-goal-support.md) — ACP Goal 支持
- [goal-external-loop.md](design/goal-external-loop.md) — Goal 外循环
- [claude-code-compat.md](design/claude-code-compat.md) — Claude Code 兼容性
- [tui-product-design.md](design/tui-product-design.md) — TUI 产品设计
- [llm-audit-log.md](design/llm-audit-log.md) — LLM 审计日志

### 🔧 开发 → [dev/](dev/)
- [tech-stack.md](dev/tech-stack.md) — 技术选型 + 项目结构
- [backend-trait.md](dev/backend-trait.md) — Backend trait 实现指南
- [roadmap.md](dev/roadmap.md) — 路线图
- [act-node-architecture.md](dev/act-node-architecture.md) — Act 节点架构

#### 实现计划 → [dev/impl/](dev/impl/)
- [review-full-development.md](dev/impl/review-full-development.md) — 后台审查完整开发文档
- [review-command-summary.md](dev/impl/review-command-summary.md) — `loom review` 命令总结
- [cli-ux-improvement.md](dev/impl/cli-ux-improvement.md) — CLI UX 改进方案
- [goal-runner-event-output.md](dev/impl/goal-runner-event-output.md) — Goal Runner 事件输出

#### 设计细化 → [dev/design/](dev/design/)
- [tool-display-ux.md](dev/design/tool-display-ux.md) — 工具显示 UX 设计

### 🧬 进化 → [evolution/](evolution/)
- [README.md](evolution/README.md) — 进化系统概述 + 文件索引
- [review.md](evolution/review.md) — 后台审查机制
- [skills.md](evolution/skills.md) — 技能系统设计
- [memory.md](evolution/memory.md) — 记忆系统
- [curator.md](evolution/curator.md) — 技能定期维护
- [gepa.md](evolution/gepa.md) — GEPA 进化优化
- [gepa-comprehensive.md](evolution/gepa-comprehensive.md) — 进化方案完善
- [implementation-plan.md](evolution/implementation-plan.md) — 缺失分析与实施
- [tools.md](evolution/tools.md) — ReviewToolExecutor 工具白名单参考
- [config.md](evolution/config.md) — 进化配置参考
- [data-structures.md](evolution/data-structures.md) — Rust 数据结构
- [decisions.md](evolution/decisions.md) — 进化设计决策
- [roadmap.md](evolution/roadmap.md) — Phase 2-6 任务 + 风险
- [commands.md](evolution/commands.md) — 进化 CLI 命令

### 📋 RFC → [rfcs/](rfcs/)
- [README.md](rfcs/README.md) — RFC 索引
- [slash-command-registry.md](rfcs/slash-command-registry.md) — 斜杠命令注册
- [tool-display-ux-proposal.md](rfcs/tool-display-ux-proposal.md) — 工具显示 UX 提案
- [env-context.md](rfcs/env-context.md) — 环境上下文
- [rfc-profile-convert.md](rfcs/rfc-profile-convert.md) — Profile 转换

### 📊 审查报告 → [review/](review/)
- 代码审查与架构分析报告

### 📝 ADR → [adr/](adr/)
- 架构决策记录

### 🚀 部署 → [deployment/](deployment/)
- 部署相关文档

### 📡 流式 → [streaming/](streaming/)
- 流式输出相关文档

### 🧠 记忆 → [memory/](memory/)
- 记忆系统相关文档

### 🐛 问题 → [bugs/](bugs/)
- Bug 记录和分析

### 🔍 诊断 → [diagnostics/](diagnostics/)
- 诊断工具和故障排查

## 审计信息

- **最后审计**: 2025-07-30
- **文档总数**: 150+ 份
- **覆盖率**: 核心模块全部有文档
- **参考实现**: Hermes (Python) / Loom (Rust)
- **对标目标**: OpenAI 级别产品文档质量
