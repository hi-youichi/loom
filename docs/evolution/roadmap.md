# 进化子系统开发路线图

## 路线图

| Phase | 内容 | 时间 | 依赖 | 完成标准 |
|-------|------|------|------|----------|
| **2** | 后台审查：Review Agent + 自动写记忆/技能 | 1 周 | Phase 1 | ≥1 会话后自动沉淀记忆或技能 |
| **3** | 技能系统：CRUD + 自动加载匹配 | 1 周 | Phase 2 | 技能自动匹配并注入 context |
| **4** | Curator：生命周期管理 + 重叠检测 | 3-5 天 | Phase 3 | 能检测 stale 技能并标记 |
| **6** | GEPA 进化 Tier 1：技能文件优化 | 2-3 周 | Phase 3 | ≥1 技能评分提升 ≥10%，无回归 |
| **7** | GEPA 进化 Tier 2：工具描述优化 | 2 周 | Phase 6 | 工具选择准确率提升 ≥5% |
| **8** | GEPA 进化 Tier 3：系统 prompt 优化 | 2 周 | Phase 7 | 行为测试评分提升，基准不退化 |
| **9** | 持续监控 + 自动分诊 | 1-2 周 | Phase 6 | 自动识别弱技能并触发进化 |

> Phase 0-1 属于 Levol 核心开发，详见 [dev/roadmap.md](../dev/roadmap.md)。

---

## Phase 6 详细任务（GEPA Tier 1：技能进化）

### Week 1：基础设施

- [ ] 搭建 `loom-evolution` crate 骨架
- [ ] 实现 `types.rs`：EvalExample, EvolutionResult, ConstraintResult, ExecutionTrace
- [ ] 实现 `FsDatasetStore`：JSONL 读写 + train/val/holdout 分割
- [ ] 实现 `ConstraintChecker`：大小预算、改进阈值、结构完整、语义保持
- [ ] 实现 `LLMJudge`：rubric 评分 + 长度惩罚

### Week 2：优化器 + 评估

- [ ] 实现 `GepaOptimizer`：多轮候选生成 + 评估 + 反思
- [ ] 实现合成数据集生成器（LLM 生成 test cases）
- [ ] 实现会话挖掘器（从 sessions/*.jsonl 挖掘真实样本）
- [ ] 实现回归门控（golden tasks 测试）
- [ ] 单元测试 + 集成测试

### Week 3：集成 + 验证

- [ ] 实现 `levol skills evolve <name>` CLI 命令
- [ ] 实现 `levol evolve run/status/accept/reject/rollback` 命令
- [ ] 实现进化记录保存 + 版本管理
- [ ] 选 2-3 个目标技能运行端到端进化
- [ ] 验证门控：≥1 技能评分提升 ≥10%，无回归

### 完成标准（Gate）

1. ≥1 技能在 holdout 集上评分提升 ≥10%
2. 所有约束通过（大小/语义/结构）
3. 回归门控通过（golden tasks 不退化）
4. 进化 diff 人类可读且合理
5. 优化管线可复用（可指向任意技能运行）

---

## Phase 7 详细任务（GEPA Tier 2：工具描述进化）

- [ ] 构建工具选择评估数据集（200-400 三元组）
- [ ] 工具描述 → DSPy Signature 参数包装
- [ ] GEPA 适配：候选生成 → 评估 → 联合评估所有工具
- [ ] 约束：描述 ≤ 500 字符，准确描述功能
- [ ] 验证：工具选择准确率 ≥5% 提升

---

## Phase 8 详细任务（GEPA Tier 3：系统 Prompt 进化）

- [ ] 识别可进化 prompt 片段（身份/记忆/技能指引等）
- [ ] 构建行为测试场景（60-80 个）
- [ ] Section-as-DSPy-parameter 包装
- [ ] 行为评估 + 基准门控双重验证
- [ ] 约束：每段不超当前 1.2 倍，总 prompt 在缓存边界内

---

## 核心风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| 进化产生退化 | 高 | 中 | 约束系统 + 回归门控 + 用户确认 |
| 评估数据集质量差 | 中 | 高 | 多来源（合成+挖掘+手工），LLM-judge 交叉验证 |
| GEPA 收敛慢或无效 | 中 | 中 | 最少 3 样本启动，10-15 样本稳定；MIPROv2 作为 fallback |
| 成本过高 | 低 | 中 | 每次运行 max_cost_usd 上限，小数据集起步 |
| 语义漂移 | 中 | 中 | 语义保持约束（余弦相似度 > 0.7） |

---

## 进化层级总览

| 层级 | 目标 | Phase | 状态 |
|------|------|-------|------|
| Tier 1 | 技能文件 (SKILL.md) | Phase 6 | 📋 规划中 |
| Tier 2 | 工具描述 | Phase 7 | 📋 规划中 |
| Tier 3 | 系统 prompt 片段 | Phase 8 | 📋 规划中 |
| Tier 4 | 代码文件 | Phase 9+ | ⏸️ 暂缓 |

---

## 与 Hermes 对比

| 维度 | Hermes | Levol |
|------|--------|-------|
| 语言 | Python (DSPy native) | Rust (trait 抽象 + LLM API) |
| 评估框架 | batch_runner + TBLite/YC-Bench | LLM-judge + golden tasks |
| 进化引擎 | DSPy+GEPA (native) | loom-evolution crate (Rust 等价) |
| 部署方式 | Git PR + GitHub | 本地文件 + 用户确认 |
| 基准门控 | TBLite + TerminalBench2 | Golden tasks + 技能级回归 |
| 代码进化 | Darwinian Evolver (AGPL) | 暂不支持 |
| 成本 | ~$2-10/run | 预估 ~$5-15/run（含回归测试） |

### 关键差异

1. **纯 Rust**：不依赖 Python/DSPy，通过 LLM API 直接实现 GEPA 算法
2. **轻量基准**：无重量级 benchmark，用 golden tasks 替代
3. **本地优先**：进化记录和版本管理在本地，无需 Git PR
4. **渐进式**：技能从简单开始，通过进化逐步优化
