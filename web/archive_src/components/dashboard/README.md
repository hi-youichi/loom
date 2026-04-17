# 仪表盘设计文档

## 概述

在侧边栏顶部仪表盘按钮点击后，展示 Agent 列表仪表盘视图。与文件列表互斥渲染，共享 220px 侧边栏宽度。

## 布局

单列信息流，从上到下：

```
┌──────────────────────┐
│ 📊 仪表盘            │  ← 标题行（已有）
├──────────────────────┤
│ 🟢 react       3次   │  ← 活跃 Agent
│ ⚪ dev          1次   │  ← 空闲 Agent
│ 🔴 linter      2次   │  ← 错误 Agent
├──────────────────────┤
│ 活跃 1 · 总计 6 次调用 │  ← 统计摘要
└──────────────────────┘
```

## 组件结构

```
FileTreeSidebar
├── 仪表盘标题行（已有）
├── DashboardView          ← 新增，仪表盘内容区
│   ├── AgentStats          ← 统计摘要
│   └── AgentList           ← Agent 列表
│       └── AgentItem[]     ← 单个 Agent 行
└── 文件列表标题行（已有）
    └── TreeContent
```

## 数据模型

```typescript
interface AgentInfo {
  name: string
  status: 'running' | 'idle' | 'error'
  callCount: number
  lastRunAt: string
  lastError?: string
}
```

## 数据来源

从 `useChat` hook 的 `run_start` 事件中提取 `agent` 字段，实时聚合：

- `run_start` → 新增 agent 或 callCount++，status 设为 running
- `run_end` → status 设为 idle
- 错误 → status 设为 error

在 `useChat` 中新增 `agents` 状态，返回给消费组件。

## 交互

- 点击 Agent 行：展开/折叠最近一次调用摘要
- 仪表盘与文件列表：通过顶部按钮切换，侧边栏内容区互斥渲染

## 视觉规格

- Agent 名称：`text-xs font-semibold`
- 调用次数：`text-xs text-muted-foreground`，右侧对齐
- 状态圆点：`size-2 rounded-full`
  - running: `bg-green-500`
  - idle: `bg-muted-foreground/40`
  - error: `bg-destructive`
- 统计摘要：`text-xs text-muted-foreground`，底部固定，`py-2 px-3 border-t`

## 实现步骤

1. 在 `useChat` 中新增 `agents` 状态和聚合逻辑
2. 新建 `src/components/dashboard/` 目录
3. 实现 `DashboardView`、`AgentList`、`AgentItem`、`AgentStats` 组件
4. 在 `FileTreeSidebar` 中添加视图切换逻辑（仪表盘 / 文件列表）
5. 点击仪表盘标题行切换到仪表盘视图，点击文件标题行切换回文件列表
