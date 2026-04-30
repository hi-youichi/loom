# RFC 索引

本目录包含 Loom 框架的 RFC（Request for Comments）文档，记录重大设计决策和架构变更。

## 状态定义

| 状态 | 含义 |
|------|------|
| Draft | 草案，正在讨论中 |
| Accepted | 已接受，等待实施 |
| Implemented | 已实施并合入主分支 |
| Rejected | 已拒绝，不计划实施 |
| Superseded | 已被新 RFC 替代 |

## RFC 列表

| RFC | 标题 | 状态 |
|-----|------|------|
| [rfc-model-in-react-state](./rfc-model-in-react-state.md) | ReAct State 中嵌入 Model 信息 | Draft |
| [limit-tool-output-truncation-scope](./limit-tool-output-truncation-scope.md) | 限定工具输出截断范围 | Draft |
| [tier-resolver-cohesion](./tier-resolver-cohesion.md) | Tier Resolver 内聚性重构 | Draft |

## 编写规范

每个 RFC 应包含以下部分：

1. **元信息**：标题、作者、日期、状态
2. **摘要**：一句话描述变更目的
3. **动机**：为什么需要这个变更
4. **详细设计**：实现方案
5. **替代方案**：考虑过的其他方案
6. **测试计划**：验证方案

## 生命周期

```
Draft → Accepted → Implemented
  ↓
Rejected
  ↓
Superseded (by newer RFC)
```
