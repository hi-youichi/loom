# Loom TUI 文档

本目录包含 Loom TUI（终端用户界面）的完整设计和实现文档。

## 文档结构

```
docs/tui/
├── README.md              # 本文档
├── design/                # 设计文档
│   ├── visual.md         # 视觉设计
│   └── interaction.md    # 交互设计
└── implementation/        # 实现文档
    └── project-plan.md   # 项目计划和实现细节
```

## 文档导航

### 设计文档

#### [视觉设计](design/visual.md)
- 界面布局和组件设计
- Agent 卡片设计
- 颜色方案和视觉样式
- 响应式设计原则

#### [交互设计](design/interaction.md)
- 输入模式（Normal/Editing）
- 快捷键和操作
- 状态转换
- 用户体验流程

### 实现文档

#### [项目计划](implementation/project-plan.md)
- 项目概述和目标
- 技术架构设计
- 实现计划和里程碑
- 技术选型和依赖

## 快速开始

### 了解 TUI 设计

1. 先阅读 [视觉设计](design/visual.md) 了解界面外观
2. 再阅读 [交互设计](design/interaction.md) 了解操作方式
3. 最后阅读 [项目计划](implementation/project-plan.md) 了解实现细节

### 开发者指南

如果你是开发者，想要：

- **理解架构** → 查看 [项目计划 - 技术设计](implementation/project-plan.md#2-技术设计)
- **实现功能** → 查看 [项目计划 - 实现计划](implementation/project-plan.md#4-实现计划)
- **添加新特性** → 参考设计文档并遵循现有模式

## 文档维护

### 更新原则

- **设计文档**：描述"是什么"和"为什么"，不涉及具体实现
- **实现文档**：描述"怎么做"，包括技术细节和代码示例

### 文档更新流程

1. 新功能设计 → 更新 `design/` 下的文档
2. 技术决策 → 更新 `implementation/project-plan.md`
3. 重大变更 → 同时更新设计和实现文档

## 相关资源

- [ratatui 官方文档](https://docs.rs/ratatui/)
- [crossterm 文档](https://docs.rs/crossterm/)
- [Tokio 异步运行时](https://tokio.rs/)

## 贡献

如果你发现文档有问题或需要补充，请：

1. 检查现有文档是否已有相关内容
2. 确定应该更新设计文档还是实现文档
3. 遵循现有文档的结构和风格
4. 提交 PR 并说明更新原因
