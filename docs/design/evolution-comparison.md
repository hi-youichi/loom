# Hermes Agent Self-Evolution vs Loom Evolution 对比分析方案

## 一、架构概览

| 维度 | Hermes Agent (Python/DSPy) | Loom Evolution (Rust) |
|------|---------------------------|------------------------|
| **编程语言** | Python 3 | Rust |
| **优化框架** | DSPy (GEPA/MIPROv2) | 自定义 GepaOptimizer |
| **核心抽象** | `dspy.Module` + `dspy.Signature` | `SkillModule` + 手写 prompt |
| **评估方式** | DSPy `Example` + `Prediction` | `EvalExample` + `RubricScore` |
| **状态管理** | DSPy context / LM 配置 | `EvolutionLlm` trait |

---

## 二、模块对应关系

### 2.1 入口与编排

| Hermes | Loom | 说明 |
|--------|------|------|
| `evolve_skill.py` | `cli/src/run/evolution_trigger.rs` | 主入口 |
| `EvolutionConfig` | `EvolutionConfig` | 配置类 |
| `evolve()` 函数 | `GepaOptimizer::optimize()` | 核心编排逻辑 |

### 2.2 数据集管理

| Hermes | Loom | 说明 |
|--------|------|------|
| `EvalDataset` | `FsDatasetStore` | 数据集存储 |
| `SyntheticDatasetBuilder` | `synthetic.rs` | 合成数据生成 |
| `GoldenDatasetLoader` | - | 金标准数据集 (Loom缺失) |
| `build_dataset_from_external` (sessiondb) | `miner.rs` | 会话挖掘 |
| `EvalExample` | `EvalExample` | 单条样本 |

### 2.3 优化器

| Hermes | Loom | 说明 |
|--------|------|------|
| `dspy.GEPA` / `dspy.MIPROv2` | `GepaOptimizer` | 优化器实现 |
| `SkillModule(dspy.Module)` | 内嵌 prompt 模板 | 候选生成 |
| `optimizer.compile()` | `generate_candidates()` | 编译/生成候选 |

### 2.4 评估系统

| Hermes | Loom | 说明 |
|--------|------|------|
| `LLMJudge` | `judge_prompt` | LLM评判 |
| `FitnessScore` | `RubricScore` | 评分结构 |
| `skill_fitness_metric()` | `average_fitness()` | 聚合评分 |
| `dspy.ChainOfThought(Signature)` | 手写 prompt | 评判 prompt |

### 2.5 约束系统

| Hermes | Loom | 说明 |
|--------|------|------|
| `ConstraintValidator` | `check_constraints()` | 约束检查入口 |
| `_check_size()` | C1: size_budget | 大小限制 |
| `_check_growth()` | 同上 (max_size_ratio) | 增长率 |
| `_check_skill_structure()` | C4: structure_integrity | 结构完整性 |
| `_check_non_empty()` | implied | 非空检查 |
| `run_test_suite()` | - | pytest 测试 (Hermes独有) |

---

## 三、关键差异深度分析

### 3.1 DSPy Signature 模式 vs 手写 Prompt

**Hermes (DSPy):**
```python
class JudgeSignature(dspy.Signature):
    task_input: str = dspy.InputField(desc="The task")
    expected_behavior: str = dspy.InputField(desc="Rubric")
    agent_output: str = dspy.InputField(desc="The agent's response")
    skill_text: str = dspy.InputField(desc="The skill instructions")
    correctness: float = dspy.OutputField(desc="Score 0.0-1.0")
    procedure_following: float = dspy.OutputField(desc="Score 0.0-1.0")
    conciseness: float = dspy.OutputField(desc="Score 0.0-1.0")
    feedback: str = dspy.OutputField(desc="Feedback")
```

**Loom (手写 Prompt):**
```rust
pub fn judge_prompt(skill_text: &str, example: &EvalExample) -> String {
    format!(r#"你是一个技能评估专家。评估以下技能...
    ...
    {{"procedure_followed": 0.0-1.0, "output_quality": 0.0-1.0, "conciseness": 0.0-1.0, "reasoning": "..."}}
    ..."#)
}
```

**差异:**
- DSPy 自动处理字段序列化/反序列化
- DSPy 提供类型安全的输入输出
- 手写 prompt 更灵活但需要手动解析 JSON

### 3.2 GEPA 优化循环

**Hermes:**
```python
optimizer = dspy.GEPA(metric=skill_fitness_metric, max_steps=iterations)
optimized_module = optimizer.compile(baseline_module, trainset=trainset, valset=valset)
# DSPy 内部处理候选生成、评估、选择
```

**Loom:**
```rust
for i in 0..self.config.max_iterations {
    let candidates = self.generate_candidates(&current_content, &failed_traces, iteration, None).await?;
    for candidate in &candidates {
        let score = self.evaluate_with_traces(...).await?;
        if score > best_score && constraints_passed {
            best_content = candidate.content.clone();
        }
    }
}
```

**差异:**
- Hermes 使用 DSPy 内置的 GEPA，自动化程度高
- Loom 需要手动实现候选生成逻辑
- Loom 的 `diverse_mutation_prompt` 提供了4种变异策略

### 3.3 多模型配置

**Hermes:**
```python
optimizer_model: str = "openai/gpt-4.1"
eval_model: str = "openai/gpt-4.1-mini"
judge_model: str = "openai/gpt-4.1"
```

**Loom:**
```rust
// 使用同一个 LLM，但 judge 和 mutation 使用不同 prompt
// 没有模型选择配置
```

**差距:** Loom 缺少多模型分级配置。

### 3.4 数据集来源

| 来源 | Hermes | Loom |
|------|--------|------|
| Synthetic | ✅ `SyntheticDatasetBuilder` | ✅ `synthetic.rs` |
| Golden | ✅ `GoldenDatasetLoader` | ❌ 缺失 |
| SessionDB | ✅ `build_dataset_from_external` | ⚠️ `miner.rs` (部分实现) |
| Real Examples | ✅ via session mining | ❌ 缺失完整管道 |

### 3.5 输出结构

**Hermes 输出:**
- `evolved_skill.md` - 进化后的技能文件
- `baseline_skill.md` - 原始副本
- `metrics.json` - 完整指标

**Loom 输出:**
- `EvolutionResult` 结构体含 `evolved_content`
- 通过 `RunStore` 持久化到文件系统

---

## 四、Loom 可改进方向

### P0 - 必须改进

#### 4.1 多模型分级支持

**现状:** 只有一个 LLM 配置

**目标:** 支持 evaluator/model/judge 三级模型配置

**涉及文件:**
- `loom-evolution/src/types.rs` - 添加 `EvaluationModels` 配置
- `loom-evolution/src/optimizer.rs` - 注入多个 LLM 实例

**方案:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationModels {
    pub judge: String,      // 用于 LLM-as-judge 评分
    pub generator: String,  // 用于合成数据生成
    pub optimizer: String,  // 用于候选变异（可用较小模型）
}

impl Default for EvaluationModels {
    fn default() -> Self {
        Self {
            judge: "gpt-4.1".to_string(),
            generator: "gpt-4.1".to_string(),
            optimizer: "gpt-4.1-mini".to_string(),
        }
    }
}
```

#### 4.2 Golden 数据集加载器

**现状:** 只支持 Synthetic

**目标:** 支持加载手工标注的 JSONL 数据集

**涉及文件:** 新增 `loom-evolution/src/golden.rs`

**方案:**
```rust
pub async fn load_golden_dataset(
    path: &Path,
    llm: &dyn EvolutionLlm,
) -> Result<Vec<EvalExample>, Box<dyn Error + Send + Sync>> {
    // 1. 读取 golden.jsonl
    // 2. 验证格式
    // 3. 自动 split (50/25/25)
    // 4. 返回带 Split 标记的数据
}
```

---

### P1 - 重要改进

#### 4.3 约束系统增强

**现状:** 
- `size_budget` - 大小比例限制
- `structure_integrity` - frontmatter 验证
- `safety_preservation` - 安全关键词检测

**差距:**
- 没有像 Hermes 的 `run_pytest` 那样运行真实测试
- 缺少 `max_tool_desc_size` / `max_param_desc_size` 细粒度约束

**方案:**
```rust
// 新增约束类型
pub enum ConstraintType {
    SizeBudget { max_ratio: f64 },
    Structure { require_frontmatter: bool, required_fields: Vec<&'static str> },
    Safety { keywords: Vec<&'static str> },
    SemanticPreservation { min_similarity: f64 },  // 需要 embedding API
    TestSuite { hermes_repo: PathBuf },  // 运行 pytest
}
```

#### 4.4 变异策略扩展

**现状:** `diverse_mutation_prompt` 有4种策略

**目标:** 与 Hermes 的 GEPA 反射机制对齐

**方案:**
```rust
// 1. 基于失败案例生成反思分析
async fn generate_reflection(&self, failed_traces: &[ExecutionTrace]) -> Result<String> {
    let analyses: Vec<String> = failed_traces
        .iter()
        .take(3)
        .map(failure_analysis_prompt)
        .collect();
    // 并行调用 LLM 生成反思
}

// 2. 反思引导的变异
fn reflection_guided_mutation_prompt(...) -> String {
    format!(r#"基于以下反思分析，改进技能...
    ## 反思分析
    {reflection}
    ..."#)
}
```

#### 4.5 Session Store 集成

**现状:** `SessionStore` trait 已定义但未与 CLI 完整集成

**目标:** 实现 `FileSessionStore` 并连接到会话历史

**涉及文件:**
- `cli/src/run/session_store.rs` - 已有部分实现
- `cli/src/run/background_review.rs` - 已有 `run_evolution_if_eligible`

**方案:**
```rust
// 扩展 SessionStore 实现
impl SessionStore for FileSessionStore {
    fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<SessionInfo>, String> {
        // 搜索会话标题和内容
    }
    
    fn get_session_content(&self, session_id: &str) -> Result<String, String> {
        // 读取会话 JSON 文件
    }
}
```

---

### P2 - 优化改进

#### 4.6 早停策略增强

**现状:** 3轮无改进即停止

**目标:** 动态调整，基于分数方差判断

**方案:**
```rust
struct EarlyStopConfig {
    patience: u32,           // 连续无改进轮数
    min_improvement: f64,   // 最小有效改进阈值
    score_variance_window: usize,  // 方差计算窗口
}

// 当分数方差 < 阈值时提前终止
fn should_stop_early(&self, scores: &[f64], config: &EarlyStopConfig) -> bool {
    if scores.len() < config.score_variance_window {
        return false;
    }
    let recent = &scores[scores.len() - config.score_variance_window..];
    let variance = calculate_variance(recent);
    variance < 0.01  // 分数稳定认为收敛
}
```

#### 4.7 并行候选评估

**现状:** 串行评估所有候选

**目标:** 使用 Tokio 并发评估

**方案:**
```rust
// 使用 tokio::try_join! 并行评估
let evaluation_futures: Vec<_> = candidates
    .iter()
    .map(|c| self.evaluate_skill(&c.content, &train_examples))
    .collect();

let results = futures::future::join_all(evaluation_futures).await;
```

#### 4.8 成本追踪

**现状:** `total_cost` 始终为 0.0

**目标:** 实现 API token 计数

**方案:**
```rust
struct CostTrackingLlm {
    inner: Box<dyn EvolutionLlm>,
    total_tokens: AtomicUsize,
}

impl EvolutionLlm for CostTrackingLlm {
    async fn complete(&self, prompt: &str) -> Result<String, ...> {
        let before = self.total_tokens.load(Ordering::Relaxed);
        let result = self.inner.complete(prompt).await;
        // 从响应中解析 token 使用量
        // 更新计数器
        Ok(result)
    }
}
```

#### 4.9 DSPy-style Signature 抽象

**目标:** 用 trait + proc macro 简化 prompt 定义

**方案:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub inputs: Vec<InputField>,
    pub outputs: Vec<OutputField>,
    pub template: String,
}

pub trait Signature {
    const TEMPLATE: &'static str;
    type Inputs: Serialize;
    type Outputs: DeserializeOwned;
}

// 使用示例
pub struct JudgeSignature;
impl Signature for JudgeSignature {
    const TEMPLATE: &'static str = r#"评估技能...
    {{"procedure_followed": {{procedure_followed}}, ...}}"#;
    
    type Inputs = (skill_text, task_input, expected_behavior);
    type Outputs = RubricScore;
}
```

---

## 五、实施路线图

```
Phase 1: 核心对齐 (P0)
├── 4.1 多模型分级支持
├── 4.2 Golden 数据集加载器
└── 完成时间: 2-3天

Phase 2: 功能完善 (P1)
├── 4.3 约束系统增强 (含 TestSuite)
├── 4.4 变异策略扩展 (反思机制)
├── 4.5 Session Store 集成
└── 完成时间: 3-5天

Phase 3: 性能优化 (P2)
├── 4.6 早停策略增强
├── 4.7 并行候选评估
├── 4.8 成本追踪
└── 完成时间: 2-3天

Phase 4: 架构演进 (Future)
├── 4.9 DSPy-style Signature 抽象
└── 考虑使用 proc macro 简化实现
```

---

## 六、关键文件索引

| 功能 | Hermes 文件 | Loom 文件 |
|------|------------|-----------|
| 入口 | `evolution/skills/evolve_skill.py` | `cli/src/run/evolution_trigger.rs` |
| 配置 | `evolution/core/config.py` | `loom-evolution/src/types.rs` |
| 优化器 | `evolution/skills/evolve_skill.py:156-177` | `loom-evolution/src/optimizer.rs:56-220` |
| 候选生成 | DSPy 内置 | `optimizer.rs:291-332` |
| 评判 | `evolution/core/fitness.py:34-104` | `loom-evolution/src/judge.rs:7-34` |
| 约束 | `evolution/core/constraints.py` | `loom-evolution/src/constraints.rs` |
| 数据集 | `evolution/core/dataset_builder.py` | `loom-evolution/src/dataset.rs` |
| 合成 | `SyntheticDatasetBuilder` | `synthetic.rs` |
| 会话挖掘 | `external_importers.py` | `miner.rs` |
| 类型 | `EvalExample, FitnessScore` | `EvalExample, RubricScore` |

---

## 七、验收标准

### Phase 1 验收
- [ ] `EvolutionConfig` 支持 `EvaluationModels` 三级配置
- [ ] Golden 数据集可正确加载并自动 split
- [ ] 现有 Synthetic 数据集流程不受影响

### Phase 2 验收
- [ ] `TestSuite` 约束可运行 pytest 并检查结果
- [ ] 反思机制生成的反思内容被纳入变异 prompt
- [ ] Session Store 可从会话历史提取相关样本

### Phase 3 验收
- [ ] 并行评估通过 `cargo test`
- [ ] 成本追踪显示正确的 token 计数
- [ ] 早停策略在收敛时提前终止

---

*文档版本: 0.1*
*创建时间: 2026-05-25*