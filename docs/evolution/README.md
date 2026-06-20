# 进化子系统

进化是 Loom 的核心差异化能力，包含四个组件：

```
会话 ──→ Review（审查+沉淀）──→ Skills（技能存储）──→ Curator（定期维护）──→ GEPA（进化优化）
         ↑                                                                    │
         └────────────── 优化后的技能回到技能池 ←─────────────────────────────────┘
```

## 文件索引

| 文件 | 内容 |
|------|------|
| [skills.md](skills.md) | 技能文件格式、生命周期、匹配、CRUD |
| [review.md](review.md) | 后台审查流程、Review Prompt、输出格式 |
| [curator.md](curator.md) | 定期维护、生命周期规则、重叠检测 |
| [gepa.md](gepa.md) | DSPy+GEPA 进化优化、约束系统、评估数据 |
| [gepa-comprehensive.md](gepa-comprehensive.md) | 进化方案完善：数据集构建、约束系统、基准门控、部署、成本、监控 |
| [usage.md](usage.md) | 技能进化使用指南（面向用户） |
| [memory.md](memory.md) | 记忆文件格式（USER.md / PROJECT.md / FACTS.md） |
| [commands.md](commands.md) | 所有进化相关 CLI 命令 |
| [config.md](config.md) | 所有进化相关配置项（memory/skills/review/curator/evolution） |
| [data-structures.md](data-structures.md) | Rust 数据结构（SkillMeta / ReviewResult / EvolutionResult） |
| [decisions.md](decisions.md) | 进化相关设计决策（D3 / D6 / D7） |
| [roadmap.md](roadmap.md) | Phase 2-6 开发任务、风险、Hermes 对比 |
| [background-review-thread-spawn.md](background-review-thread-spawn.md) | Background Review 独立线程方案（与 Hermes 对齐） |

## 进化循环

1. **输入**：用户和底层 CLI 的一次完整对话
2. **Review**：AI 审查对话，判断是否有值得沉淀的内容
3. **沉淀**：如果有，自动更新记忆文件或创建/更新技能
4. **维护**：Curator 定期扫描技能，标记过期、检测重叠
5. **优化**：GEPA 自动测试并优化高频使用的技能
6. **反馈**：优化后的技能在下一次会话中生效，循环往复

## 设计原则

- **异步不阻塞**：Review 在后台运行，用户无需等待
- **可人工干预**：所有数据是文件，用户可以随时查看和手动修改
- **渐进式**：技能从简单开始，通过进化逐步优化
- **保守沉淀**：宁可漏掉一些，不要写入低质量内容
