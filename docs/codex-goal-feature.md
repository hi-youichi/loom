---
sidebar_position: 2
title: "Codex /goal 功能"
description: "Codex CLI Ralph Loop 模式"
---

# Codex `/goal` 功能文档

## 概述

`/goal` 是 OpenAI 在 Codex CLI 0.128.0 中内置的 **Ralph Loop** 模式。它让 Codex 能够持续迭代执行任务，直到目标达成或预算耗尽，无需用户逐轮干预。

## 快速开始

### 启用功能

在 `~/.codex/config.toml` 中添加：

```toml
[features]
goals = true
```

### 基本命令

| 命令 | 说明 |
|------|------|
| `/goal <description>` | 创建目标并开始循环 |
| `/goal pause` | 暂停当前目标 |
| `/goal resume` | 恢复已暂停的目标 |
| `/goal clear` | 清除目标 |

### 状态

- `pursuing` — 目标进行中
- `paused` — 目标已暂停
- `achieved` — 目标已达成
- `unmet` — 目标未达成
- `budget-limited` — 预算耗尽

## 技术方案

### 循环架构

```
用户输入 /goal <描述>
    ↓
Codex 进入循环模式
    ↓
每个 turn 结束时自动注入：
  - goals/continuation.md → 引导继续执行
  - goals/budget_limit.md → 检查预算
    ↓
Agent 调用 update_goal 模型工具更新状态
    ↓
循环直到达成目标、预算耗尽、或手动清除
```

### 五阶段循环

```
┌──────┐    ┌──────┐    ┌──────┐    ┌──────┐    ┌──────────┐
│ Plan │ →  │ Act  │ →  │ Test │ →  │Review│ →  │ Iterate  │
└──────┘    └──────┘    └──────┘    └──────┘    └────┬─────┘
                                                    ↓
                              (回到 Plan 继续下一轮迭代)
```

| 阶段 | 职责 |
|------|------|
| Plan | 将大目标分解为可执行的子任务，定义成功标准 |
| Act | 修改代码、安装依赖、执行 shell 命令 |
| Test | 运行单元测试、linter、构建命令，收集失败信息 |
| Review | 评估当前进度是否接近目标，识别新障碍 |
| Iterate | 根据 Review 结果生成下一个 Plan |

### 关键组件

1. **`update_goal` 模型工具** — Agent 调用此工具报告状态
2. **`continuation.md`** — 循环继续提示，自动注入到每个 turn
3. **`budget_limit.md`** — 预算检查提示，控制软停止
4. **App Server API** — 持久化层，支持跨进程继续
5. **TUI 控制** — 终端界面支持 pause/resume/clear

### 退出条件

| 条件 | 行为 |
|------|------|
| 目标达成 | Agent 自评成功标准满足 → 输出总结，退出 |
| 预算耗尽 | 触发 `budget_limited` → 写进度报告，退出 |
| 手动清除 | 用户输入 `/goal clear` 或 Ctrl+C |

## 使用场景

### 适用场景

- **重构任务**：如 "将项目从 Pydantic v1 迁移到 v2，确保所有测试通过"
- **大型迁移**：跨多个文件的结构化变更
- **迭代修复**：需要多轮测试-修复-验证的复杂问题

### 不适用场景

- 需要频繁人工确认方向的探索性任务
- 单次简单问答或代码解释

## 设计理念

> "The Ralph loop's intelligence is in the loop, not in the agent. The agent is fungible. The loop is what makes it autonomous."

关键点：
- 智能在循环控制层面，而非 prompt 层面
- Agent 可以被替换，循环逻辑才是自主性的核心
- 通过固化退出条件和验证机制，保证任务完成的确定性

## 与其他命令对比

| 命令 | 行为 | 持久性 |
|------|------|--------|
| `/plan` | 生成计划，等待用户确认 | 单次 |
| `/goal` | 持续循环直到完成或预算耗尽 | 跨会话 |
| `/resume` | 继续之前的会话 | 恢复上下文 |

## 参考链接

- [Codex CLI Features](https://developers.openai.com/codex/cli/features)
- [Run long horizon tasks with Codex](https://developers.openai.com/blog/run-long-horizon-tasks-with-codex)
- [Ralph Loop Pattern](https://ghuntley.com/ralph/)