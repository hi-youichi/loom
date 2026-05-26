# Hermes vs Loom 自我进化能力对比

**分析日期：** 2025-08-19
**结论：** 两条完全不同的进化范式。Hermes 是 LLM-direct-edit，Loom 是 GEPA numerical-optimization。

---

## 1. 进化范式对比

| 维度 | Hermes | Loom |
|------|--------|------|
| 进化方式 | LLM 直接修改技能文件 | GEPA 数值优化 + dataset |
| 触发方式 | Per-turn background_review | 需 train.jsonl >= 5 行 |
| 进化粒度 | 每次对话持续优化 | 批量 dataset-driven |
| 进化对象 | SKILL.md / MEMORY.md / USER.md | 技能指令（instructions.md）|

---

## 2. Hermes 进化架构

### 2.1 双路径进化

**路径一：Per-turn Background Review**
- 触发频率：每 10 个 user turn 触发 memory review，每 10 个 tool-calling 迭代触发 skill review
- 方式：fork 新的 AIAgent，注入 review prompt，让 LLM 决定是否更新技能文件
- 反馈：生成 Self-improvement review 摘要

**路径二：Periodic Curator Pass**
- 触发频率：默认每 7 天运行一次
- 方式：LLM 批量合并窄技能为 umbrella，自动归档 stale 技能
- 报告：生成 JSON + Markdown 运行报告

### 2.2 Skill Manage 工具

Hermes 有结构化的 skill_manage 工具，6 个 action：
- create — 创建 SKILL.md（含 YAML frontmatter）
- edit — 全量替换
- patch — 模糊查找替换
- delete — 删除（支持 absorbed_into 合并）
- write_file — 写入支持文件
- remove_file — 删除支持文件

安全机制：
- 名称验证：^[a-z0-9][a-z0-9._-]*$
- 内容大小限制：100,000 字符
- 原子写入：tempfile + os.replace()
- Pin 保护：pinned 技能不可删除
- Security scan：可选安全扫描

### 2.3 Skill Provenance 追踪

- ContextVar _write_origin 追踪创建来源
- foreground 创建 → 用户请求
- background_review 创建 → agent-created

---

## 3. Loom 进化架构

### 3.1 GEPA 优化器

Loom 使用 GEPA（Generative Evolution with Policy Adaptation）优化器：
- 基于 JSONL dataset 进行训练
- 支持 train/holdout 数据分割
- 约束检查：size ratio、semantic similarity
- RunStore 持久化：runs + backups + rollback

### 3.2 Evolution Trigger

条件：
- lifecycle == Active
- train.jsonl 存在且 >= 5 行
- 由 background_review 触发（当前 evolve run CLI 是 stub）

### 3.3 与 Hermes 的差异

| 维度 | Hermes | Loom |
|------|--------|------|
| 数据来源 | 每次对话实时反馈 | 离线 JSONL dataset |
| 优化目标 | 技能文档质量 | 指令响应准确性 |
| 合并策略 | LLM-driven umbrella building | 无 |
| 归档策略 | active→stale→archived | active→stale（无 archive）|

---

## 4. Skill Lifecycle 管理对比

### Hermes

状态机：active → stale → archived（可逆）

Sidecar 数据：~/.hermes/skills/.usage.json
{created_by, use_count, view_count, patch_count, last_used_at, last_viewed_at, last_patched_at, created_at, state, pinned, archived_at}

自动转换：
- stale_after_days（默认 30 天）→ active → stale
- archive_after_days（默认 90 天）→ stale → archived

Counter bumps：
- bump_view：skill_view 调用时
- bump_use：技能被加载到 prompt 时
- bump_patch：skill_manage patch/edit 时

### Loom

状态机：Active → Stale（简化版）

缺失：
- 无 usage counter
- 无 sidecar JSON
- 无 archive/restore
- 无 bump_view/use/patch 机制

---

## 5. 安全机制对比

### Hermes

- Protected skills：bundled / hub / pinned 不可修改
- Do NOT capture 清单：环境故障、瞬态错误等
- Security scan：agent 创建的技能可选扫描
- Atomic write + rollback：失败时回滚
- Curator backup：运行前快照可恢复

### Loom

- 无对应安全机制
- evolve run CLI 是 stub
- 无技能保护
- 无回滚机制

---

## 6. 汇总：Loom 进化系统缺失

| 功能 | Hermes | Loom | 优先级 |
|------|--------|------|--------|
| evolve run CLI 实现 | 有 | stub | P0 |
| Skill Manage 工具 | 6 action | 无 | P1 |
| Skill Provenance | ContextVar 追踪 | 无 | P1 |
| Usage 统计 | sidecar JSON | 无 | P1 |
| Archive/Restore | 完整 | 无 | P2 |
| Curator Pass | LLM-driven 合并 | 无 | P2 |
| 安全扫描 | Security scan | 无 | P2 |
| 运行报告 | JSON + MD | 无 | P2 |

---

## 7. 建议

Hermes 的进化更适合持续迭代优化（per-turn feedback loop），Loom 的 GEPA 更适合批量训练（dataset-driven）。

**Hermes 优势**：
- 实时反馈，进化速度快
- LLM 直接判断改进方向
- 完整的生命周期管理

**Loom GEPA 优势**：
- 可离线训练
- 约束检查保证质量
- 可回滚到任意版本

**建议补足**：
1. 实现 evolve run CLI 实际逻辑
2. 添加 skill_manage 工具
3. 补充 usage 统计机制
4. 添加 curator pass 合并策略