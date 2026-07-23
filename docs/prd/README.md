# Loom 产品文档

本目录是 Loom 的产品事实基线。它描述用户价值、范围、验收标准和待决策事项；实现细节、架构权衡与测试方案仍放在 `docs/design/`、各 crate 文档和测试中。

## 推荐阅读顺序

1. [产品定位与体验原则](loom-product-positioning.md)：产品是什么、服务谁、为何存在。
2. [用户、任务与场景定义](user-personas-and-scenarios.md)：用户问题与验证假设。
3. [v0.4 路线图与发布计划](v0.4-roadmap-and-release-plan.md)：范围、优先级与完成定义。
4. [v0.4 核心 Agent PRD](v0.4-core-agent-prd.md)：CLI 与 ACP 的核心任务闭环。
5. [权限、安全与数据生命周期 PRD](permissions-security-and-data-lifecycle-prd.md)：授权、数据与隐私边界。
6. [记忆与技能系统 PRD](memory-and-skills-prd.md)：可控的上下文沉淀。
7. [v0.4 工作流自动化 PRD](v0.4-workflow-automation-prd.md)：多 Agent 工作流与实例体验。
8. [多入口体验规范](multi-surface-experience-spec.md)：CLI、ACP/IDE 与 Telegram Bot 的一致性。
9. [指标与研究计划](metrics-and-research-plan.md)：验证产品方向的指标、隐私规则和研究方法。

## 文档维护规则

- 面向用户的能力必须标为 Stable、Preview、Experimental 或 Planned；不能将设计稿表述为已发布能力。
- 需求变化先更新对应 PRD，再更新 README、帮助、实现与测试。
- 每个 PRD 的验收标准都应有测试、端到端脚本或记录在案的人工验证证据。
- 发现跨入口、权限、数据归属或术语不一致时，以本目录为讨论起点，并在决策后同步所有受影响文档。
