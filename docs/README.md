# Levol — 自进化 CLI Wrapper

> 包裹在 AI Coding CLI（Loom / Codex）外面的进化层。给它加上**记忆**、**技能**、**进化**三种能力，越用越好。

## TL;DR

你照常用 `loom chat` 或 `codex` 写代码，Levol 在后台帮你：
- 记住你的偏好和项目上下文（**记忆**）
- 沉淀可复用的工作流（**技能**）
- 自动优化技能质量（**进化**）

底层 CLI 零修改，Rust 单二进制，纯文件系统存储。

## 核心概念

| 概念 | 是什么 | 存在哪 |
|------|--------|--------|
| **记忆** | 跨会话的用户偏好和项目事实 | `memory/USER.md`、`memory/PROJECT.md` |
| **技能** | 可复用的工作流（步骤+陷阱） | `skills/auto/<name>/SKILL.md` |
| **会话** | 完整对话记录（可搜索） | `sessions/*.jsonl` + SQLite FTS5 |
| **Review** | 会话结束后，AI 审查是否值得记住 | 自动运行，异步不阻塞 |
| **进化** | 用 GEPA 自动优化技能质量 | `loom-evolution` crate，可选装 |

## 架构

```
┌─────────────────────────────────────┐
│  Levol 编排层                        │
│  Assembler / Reviewer / Curator     │
├─────────────────────────────────────┤
│  Backend Adapter（可插拔）           │
│  ┌─────────┐  ┌──────────────────┐  │
│  │  Loom   │  │  Codex (OpenAI)  │  │
│  └─────────┘  └──────────────────┘  │
├─────────────────────────────────────┤
│  数据层（纯文件系统）                │
│  memory/  skills/  sessions/         │
└─────────────────────────────────────┘
```

## 会话生命周期

```
levol chat
  ├─ 1. 组装上下文 ─→ 注入记忆+技能到 CLAUDE.md / AGENTS.md
  ├─ 2. 启动底层 CLI ─→ 透传 stdin/stdout，录制对话
  ├─ 3. 会话结束 ─→ 保存 JSONL，还原 context 文件
  └─ 4. 后台 Review ─→ AI 判断是否更新记忆/技能（异步）
```

## 文档导航

### 使用视角 → [guide/](guide/)
- [getting-started.md](guide/getting-started.md) — 安装、初始化、第一次会话
- [cli.md](guide/cli.md) — 全部命令参考
- [config.md](guide/config.md) — levol.yaml 配置项说明
- [backends.md](guide/backends.md) — Loom vs Codex 切换指南

### 设计视角 → [design/](design/)
- [architecture.md](design/architecture.md) — 三层架构设计
- [session-lifecycle.md](design/session-lifecycle.md) — 会话全流程详解
- [data-format.md](design/data-format.md) — 数据格式设计
- [decisions.md](design/decisions.md) — 关键设计决策记录

### 实现视角 → [dev/](dev/)
- [tech-stack.md](dev/tech-stack.md) — 技术选型 + 项目结构 + 接口定义
- [backend-trait.md](dev/backend-trait.md) — Backend trait + 写新 Adapter 指南
- [roadmap.md](dev/roadmap.md) — 路线图 + 风险 + Hermes 对比

### 进化子系统 → [evolution/](evolution/)
- [README.md](evolution/README.md) — 进化系统概述 + 文件索引
- [skills.md](evolution/skills.md) — 技能系统设计
- [review.md](evolution/review.md) — 后台审查机制
- [curator.md](evolution/curator.md) — 技能定期维护
- [gepa.md](evolution/gepa.md) — DSPy+GEPA 进化优化
- [gepa-comprehensive.md](evolution/gepa-comprehensive.md) — 进化方案完善（数据集、约束、门控、部署、成本）
- [memory.md](evolution/memory.md) — 记忆系统
- [commands.md](evolution/commands.md) — 进化相关 CLI 命令
- [config.md](evolution/config.md) — 进化子系统配置参考
- [data-structures.md](evolution/data-structures.md) — Rust 数据结构
- [decisions.md](evolution/decisions.md) — 进化相关设计决策
- [roadmap.md](evolution/roadmap.md) — Phase 2-6 任务 + 风险 + Hermes 对比
