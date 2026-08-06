# TUI 产品文档编写工作流

## 目标

根据产品架构文档（`product-architecture.md`），由 6 个 agent 并行编写各层的产品文档，最终汇总为完整的 TUI 产品文档。

## 分工

| Agent | 负责层 | 输出文件 | 内容范围 |
|-------|--------|----------|----------|
| Agent 1 | 基础设施层 | `docs/tui/product/infrastructure.md` | ratatui, crossterm, tokio, 终端检测, ANSI 工具 |
| Agent 2 | 终端层 | `docs/tui/product/terminal.md` | 内联视图, 事件系统, 历史行插入, ^Z 暂停, 光标管理 |
| Agent 3 | 渲染层 | `docs/tui/product/rendering.md` | Renderable trait, 布局组件, 历史渲染, 差异渲染, Spinner |
| Agent 4 | 交互层 | `docs/tui/product/interaction.md` | 输入框, 审批弹窗, 选择列表, 状态指示, 通知 |
| Agent 5 | 应用层 | `docs/tui/product/application.md` | App 主循环, 会话管理, 配置管理, 插件集成 |
| Agent 6 | 集成架构 | `docs/tui/product/integration.md` | 与现有系统集成, 条件编译, 事件适配 |

## 每个 agent 的输入

每个 agent 收到：
1. 产品架构文档（`product-architecture.md`）中对应层的完整内容
2. 该层的核心接口定义
3. 产品文档的编写模板

## 产品文档格式

每个产品文档采用以下模板：

```markdown
# 产品文档：[层名称]

## 概述

[该层在整个系统中的定位和作用]

## 用户价值

[该层为最终用户提供了什么价值]

## 核心组件

[每个组件的产品视角描述 - 是什么、做什么、为什么需要]

## 与其他层的关系

[该层如何与上下层协作]

## 关键设计决策

[产品层面的设计决策，非技术实现]

## 使用场景

[用户在使用中会接触到该层的哪些部分]
```

## 执行流程

1. 所有 6 个 agent 并行启动
2. 每个 agent 读取架构文档，提取对应层内容
3. 按照模板编写产品文档
4. 写入对应文件
5. 汇总 agent 检查完整性和一致性