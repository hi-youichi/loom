# GEPA 技能进化 — 完善方案

> 本文档补充 [gepa.md](gepa.md) 中缺失的细节，基于 [Hermes PLAN](../../hermes-agent-self-evolution/PLAN.md) 的最佳实践，适配 Levol 的纯 Rust + LLM API 架构。

## 一、评估数据集构建

### 1.1 数据来源

GEPA 最少需要 3 个样本即可开始优化，建议 15-30 个样本获得稳定效果。

| 来源 | 质量 | 成本 | 适用阶段 |
|------|------|------|----------|
| **A. 合成生成** | 中 | 低 (~$0.5) | 启动阶段 |
| **B. 会话挖掘** | 高 | 中 (~$1-2) | 有历史数据后 |
| **C. 手工黄金集** | 最高 | 人工 | 关键技能 |
| **D. 技能特定自动评估** | 高 | 中 | 有可编程验证的技能 |

### 1.2 来源 A：合成生成（启动阶段）

用强模型读取 SKILL.md，生成测试样本：

```
输入：技能文件内容
输出：15-30 个 (task_input, expected_behavior) 对

分割：60% train / 20% val / 20% holdout
```

**关键**：`expected_behavior` 应该是评分标准（rubric），而非精确文本。
- ✅ 好例子："应识别第 42 行的 SQL 注入漏洞"
- ❌ 坏例子："输出 'Found SQL injection on line 42'"

**Prompt 模板**（供合成生成使用）：
```
你是一个评估数据集构建专家。阅读以下技能文件，生成 {n} 个测试样本。

每个样本包含：
- task_input：用户可能给出的真实任务描述
- expected_behavior：评分标准（rubric），描述正确执行应该达到什么效果
- difficulty：Easy / Medium / Hard

技能文件：
{skill_content}
```

### 1.3 来源 B：会话挖掘（有历史后）

从 `sessions/*.jsonl` 中挖掘真实使用数据：

1. 搜索加载了目标技能的会话
2. 提取用户任务和 agent 完整回复
3. 用 LLM-as-judge 对每个 (task, response) 打分
4. 高分 → 正例；低分 → GEPA 反思分析的失败案例

```rust
// 会话挖掘伪代码
fn mine_sessions(skill_name: &str, sessions_dir: &Path) -> Vec<EvalExample> {
    let sessions = scan_sessions(sessions_dir);
    let relevant = sessions.filter(|s| s.contains_skill(skill_name));
    let scored = relevant.map(|s| llm_judge_score(s));
    scored.filter(|s| s.score > 0.6).into_examples()
}
```

### 1.4 来源 C：手工黄金集（关键技能）

手动编写的高质量测试集，存储为 JSONL：

```
~/.loom/data/evolution/datasets/<skill-name>/golden.jsonl
```

格式：
```json
{"task_input": "...", "expected_behavior": "...", "difficulty": "Medium"}
```

### 1.5 来源 D：技能特定自动评估

部分技能可以编程验证：

| 技能 | 自动评估方法 |
|------|-------------|
| debug-rust-errors | 植入 bug → 运行技能 → 检查测试是否通过 |
| code-review | 创建带植入问题的 PR → 检查是否被发现 |
| search | 搜索已知目标 → 检查是否找到 |

### 1.6 评分函数：LLM-as-judge + Rubric

```rust
// 评分维度
struct RubricScore {
    procedure_followed: f64,  // 是否遵循技能流程 (0-1)
    output_quality: f64,      // 输出是否正确/有用 (0-1)
    conciseness: f64,         // 是否简洁 (0-1)
}

// 综合评分（权重可配置）
fn fitness(scores: &[RubricScore], config: &RubricConfig) -> f64 {
    scores.iter().map(|s| {
        s.procedure_followed * config.w_procedure
        + s.output_quality * config.w_quality
        + s.conciseness * config.w_conciseness
    }).sum::<f64>() / scores.len() as f64
}
```

**长度惩罚**：接近字符限制的变体自动降分，防止进化漂移向冗长。

---

## 二、约束系统完善

### 2.1 约束层次

每个进化变体必须通过所有约束才能被接受：

| 约束 | 层级 | 说明 |
|------|------|------|
| **C1: 大小预算** | 硬门控 | 进化后不超过 baseline 的 1.2 倍 |
| **C2: 改进阈值** | 硬门控 | evolved score > baseline score |
| **C3: 语义保持** | 软门控 | 余弦相似度 > 0.7（LLM embedding） |
| **C4: 结构完整** | 硬门控 | YAML frontmatter 字段不缺失 |
| **C5: 无安全退化** | 硬门控 | 不移除安全检查、错误处理 |

### 2.2 约束检查接口

```rust
// 扩展 ConstraintChecker trait
trait ConstraintChecker {
    fn check(&self, evolved: &str, baseline: &str) -> Vec<ConstraintResult>;

    // 新增：语义保持检查
    fn check_semantic_preservation(&self, evolved: &str, baseline: &str) -> ConstraintResult {
        let sim = cosine_similarity(embed(evolved), embed(baseline));
        ConstraintResult {
            name: "semantic_preservation",
            passed: sim >= 0.7,
            message: format!("Similarity: {:.3}", sim),
        }
    }

    // 新增：结构完整性检查
    fn check_structure(&self, evolved: &str) -> ConstraintResult {
        // 验证 YAML frontmatter 完整性
        let has_yaml = evolved.starts_with("---");
        let fields = parse_yaml_fields(evolved);
        let required = ["name", "description", "triggers", "lifecycle"];
        let missing: Vec<_> = required.iter()
            .filter(|r| !fields.contains(&r.to_string()))
            .collect();
        ConstraintResult {
            name: "structure_integrity",
            passed: missing.is_empty(),
            message: if missing.is_empty() {
                "All required fields present".into()
            } else {
                format!("Missing fields: {:?}", missing)
            },
        }
    }
}
```

---

## 三、进化层级（Tier System）

基于 Hermes PLAN，Levol 的进化目标分为 4 个层级：

| 层级 | 目标 | 价值 | 风险 | 优先级 |
|------|------|------|------|--------|
| **Tier 1** | 技能文件 (SKILL.md) | 最高 | 最低 | Phase 6 |
| **Tier 2** | 工具描述 | 中 | 低 | Phase 7 |
| **Tier 3** | 系统 prompt 片段 | 高 | 中 | Phase 8 |
| **Tier 4** | 代码文件 | 高 | 最高 | Phase 9 |

> 当前路线图只覆盖 Tier 1。Tier 2-4 作为后续扩展。

### Tier 2：工具描述进化

优化工具 schema 中的 description 字段，让 agent 选对工具：

```
评估数据：200-400 个 (任务描述, 正确工具, 正确参数) 三元组
约束：描述 ≤ 500 字符，必须准确描述工具功能
关键：所有工具描述联合评估，防止一个工具"偷走"另一个的选择率
```

### Tier 3：系统 Prompt 进化

优化 prompt_builder 的可进化片段（身份、记忆指引、技能指引等）：

```
评估数据：60-80 个行为测试场景
约束：每段不超过当前大小的 1.2 倍，总 prompt 在缓存边界内
关键：行为测试 + 基准门控双重验证
```

### Tier 4：代码进化

暂不纳入当前规划。需要强测试套件 + 人工审查作为护轨。

---

## 四、基准门控（Benchmark Gate）

### 4.1 门控流程

```
进化变体
    │
    ├──► 约束检查（大小/结构/语义）── GATE 1: 基本合规
    │
    ├──► 评估数据集打分 ────────────── FITNESS: 质量评分
    │
    ▼
Top-3 变体
    │
    ├──► 端到端测试（技能实际执行）── GATE 2: 功能正确
    │
    ├──► 回归测试（其他技能不受影响）── GATE 3: 无副作用
    │
    ▼
最佳变体 → 保存 + 用户确认
```

### 4.2 回归检查

Levol 没有 Hermes 那样的 TBLite benchmark，但可以用以下方式替代：

1. **技能级回归**：进化技能 A 时，在 holdout 集上同时测试技能 B、C，确保无退化
2. **对话级回归**：用一组标准任务（golden tasks）测试 agent 整体表现
3. **配置快照对比**：进化前后，agent 的 context 注入行为不变

```rust
struct RegressionGate {
    golden_tasks: Vec<GTask>,        // 标准任务集（~20 个）
    skills_to_monitor: Vec<String>,  // 监控的技能列表
    tolerance: f64,                   // 容忍退化幅度（默认 2%）
}

impl RegressionGate {
    async fn check(&self, evolved_skill: &str) -> GateResult {
        // 1. 运行 golden tasks，记录通过率
        // 2. 对比 baseline 通过率
        // 3. 退化超过 tolerance → REJECT
        // 4. 无退化或改进 → PASS
    }
}
```

---

## 五、部署流程

### 5.1 进化记录与版本管理

每次进化运行保存完整记录：

```
~/.loom/data/evolution/runs/<skill-name>/<timestamp>/
├── baseline.md          # 原始技能
├── evolved.md           # 优化后技能
├── metrics.json         # 评分数据
├── dataset.jsonl        # 使用的评估数据集
├── traces/              # GEPA 执行轨迹（用于反思分析）
│   ├── candidate_001.json
│   ├── candidate_002.json
│   └── ...
└── diff.md              # 人类可读的变更摘要
```

### 5.2 metrics.json 格式

```json
{
  "skill_name": "debug-rust-errors",
  "timestamp": "2025-06-15T10:30:00Z",
  "optimizer": "GEPA",
  "iterations": 10,
  "candidates_evaluated": 47,
  "baseline_score": 0.62,
  "evolved_score": 0.78,
  "holdout_score": 0.75,
  "baseline_size": 2450,
  "evolved_size": 2890,
  "size_ratio": 1.18,
  "dataset_source": "synthetic",
  "dataset_size": 20,
  "cost_usd": 3.50,
  "constraints_passed": ["size_budget", "improvement", "semantic", "structure"],
  "regression_check": "passed",
  "accepted": true
}
```

### 5.3 部署步骤

```
1. 进化完成 → 保存到 evolution/runs/
2. 生成人类可读 diff 摘要
3. 用户确认：
   - `levol evolve accept <name>` → 替换当前技能
   - `levol evolve reject <name>` → 保留原始，归档进化结果
   - `levol evolve compare <name>` → 并排对比
4. 接受后：原始技能自动备份到 evolution/backups/<name>/<timestamp>.md
5. 回滚：`levol evolve rollback <name> --version <timestamp>`
```

### 5.4 回滚机制

```rust
fn rollback_skill(name: &str, version: &str) -> Result<()> {
    let backup = evolution_dir.join("backups").join(name).join(format!("{}.md", version));
    let current = skills_dir.join(name).join("SKILL.md");
    fs::copy(&backup, &current)?;
    log::info!("Rolled back {} to version {}", name, version);
    Ok(())
}
```

---

## 六、成本分析

| 操作 | 预计成本 | 频率 |
|------|----------|------|
| 合成评估数据集生成（20 样本） | ~$0.50 | 一次性 |
| 会话挖掘 + LLM 打分 | ~$1-2 | 定期 |
| GEPA 单次进化（10 轮） | ~$2-10 | 按需/定期 |
| 回归测试（golden tasks） | ~$1-3 | 每次进化后 |
| **单技能完整进化周期** | **~$5-15** | - |

**建议**：从 10-15 个样本的小数据集开始，效果好再扩展。

---

## 七、持续监控（Phase 5+）

### 7.1 性能指标采集

```rust
struct SkillMetrics {
    name: String,
    success_rate: f64,       // 技能加载后任务成功率
    usage_count: u32,        // 使用次数
    avg_score: f64,          // LLM-judge 平均分
    last_evolved: DateTime,  // 上次进化时间
    trend: Trend,            // Improving / Stable / Declining
}
```

### 7.2 自动分诊

```rust
fn auto_triage(metrics: &[SkillMetrics]) -> Vec<TriageItem> {
    metrics.iter()
        .filter(|m| m.success_rate < 0.7 || m.trend == Trend::Declining)
        .map(|m| TriageItem {
            skill: m.name.clone(),
            priority: (1.0 - m.success_rate) * m.usage_count as f64,
            reason: format!("Success rate: {:.1}%, usage: {}", m.success_rate * 100.0, m.usage_count),
        })
        .sorted_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap())
        .collect()
}
```

### 7.3 定时调度

```yaml
# levol.yaml
evolution:
  enabled: true
  model: "openai/gpt-4.1"
  schedule: "weekly"              # weekly / monthly / manual
  max_cost_per_run_usd: 10.0     # 每次运行最大成本
  auto_accept: false              # 自动接受进化结果（需要 true 才会自动替换）
  triage_threshold: 0.7           # 成功率低于此值触发进化
  max_concurrent: 1               # 最多同时进化几个技能
```

---

## 八、GEPA 执行轨迹（Reflective Analysis）

GEPA 的核心优势是读取执行轨迹来理解"为什么失败"，而不仅是"失败了"。

### 8.1 轨迹采集

```rust
struct ExecutionTrace {
    candidate_id: String,
    task_input: String,
    skill_text: String,
    agent_response: String,
    score: f64,
    score_breakdown: RubricScore,
    failure_analysis: Option<String>,  // LLM 生成的失败原因分析
}
```

### 8.2 轨迹驱动优化

```
Round 1: 生成候选 → 评估 → 记录轨迹（包括失败分析）
Round 2: GEPA 读取失败轨迹 → 反思 → 生成改进候选
Round 3: 重复...

每轮：
- 候选从失败轨迹中学习，不是随机突变
- GEPA 的聚合机制把多个失败案例的经验综合
- 最少 3 个样本即可开始，5-10 轮通常收敛
```

---

## 九、与现有架构的集成

### 9.1 进化循环完整流程

```
用户使用 Levol
    │
    ▼
会话录制 (sessions/*.jsonl)
    │
    ▼
Review Agent (后台审查) → 沉淀记忆/技能
    │
    ▼
Skills 系统 (技能存储)
    │
    ▼
Curator (定期维护) → 标记 stale → 触发进化候选
    │
    ▼
GEPA 进化优化
    │
    ├─ 构建评估数据集（合成/挖掘/手工）
    ├─ 运行 GEPA 优化（多轮候选+评估）
    ├─ 约束检查（大小/语义/结构/安全）
    ├─ 回归门控（golden tasks）
    └─ 保存进化记录 → 用户确认/拒绝
    │
    ▼
优化后的技能回到技能池
    │
    ▼
下一次会话使用优化后技能 → 循环继续
```

### 9.2 loom-evolution crate 模块结构

```
loom-evolution/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── types.rs           # EvalExample, EvolutionResult, ConstraintResult, etc.
│   ├── optimizer.rs       # GepaOptimizer trait + 实现
│   ├── constraints.rs     # ConstraintChecker trait + 内置约束
│   ├── dataset/
│   │   ├── mod.rs
│   │   ├── synthetic.rs   # 合成数据集生成
│   │   ├── mining.rs      # 会话挖掘
│   │   └── store.rs       # FsDatasetStore
│   ├── eval/
│   │   ├── mod.rs
│   │   ├── judge.rs       # LLM-as-judge 评分
│   │   ├── rubric.rs      # RubricScore + fitness
│   │   └── regression.rs  # 回归门控
│   ├── trace.rs           # 执行轨迹采集与分析
│   └── deploy/
│       ├── mod.rs
│       ├── version.rs     # 版本管理
│       └── rollback.rs    # 回滚
└── tests/
```

---

## 十、开放问题

1. **评估数据集共享**：是否支持跨项目共享评估数据集？建议支持 `~/.loom/data/evolution/datasets/shared/`
2. **多模型进化**：是否针对不同模型分别进化？建议先单模型，后续通过 `evolution.model` 配置
3. **A/B 测试**：进化后是否自动 A/B 测试？建议 Phase 5+ 支持
4. **社区贡献**：是否支持社区提交评估数据集？建议通过 Skills Hub 实现
