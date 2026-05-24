# GEPA 技能进化

用纯 Rust 实现的 GEPA 优化引擎，自动优化技能文本质量。

> 可选模块，默认关闭。需要额外的 API key。

## 概述

GEPA (Gradient-free Evolutionary Prompt Optimization with Aggregations) 是一种无需梯度的 prompt 优化方法。Levol 用它来优化 SKILL.md 的正文部分，使技能在实际使用中效果更好。

```
原始技能 ──→ 构建评估数据集 ──→ GEPA 多轮优化 ──→ 约束验证 ──→ 用户确认 ──→ 替换
(baseline)                                                        (evolved)
```

## 触发方式

```bash
# 手动进化单个技能
levol skills evolve debug-rust-errors

# 运行所有待进化技能
levol evolve run

# 查看进化历史
levol evolve status
```

自动触发（如果 `evolution.enabled`）：
```yaml
evolution:
  schedule: "0 3 * * 0"  # 每周日凌晨 3 点
```

## 执行流程

```
levol skills evolve <name>
    │
    ▼
1. 加载 skills/auto/<name>/SKILL.md (baseline)
    │
    ▼
2. 构建评估数据集
    │   ├── synthetic: LLM 生成测试用例
    │   ├── golden: 人工标注的参考案例
    │   └── sessiondb: 从历史会话中挖掘
    │
    ▼
3. GEPA 优化循环 (N iterations)
    │   每轮: 变异 → 评估 → 反思 → 选择 → 重组
    │
    ▼
4. 约束验证
    │   ├── 大小 ≤ max_skill_size
    │   ├── 增长 ≤ max_growth (20%)
    │   └── 结构完整 (有 frontmatter + name + description)
    │
    ▼
5. Holdout 评估 (baseline vs evolved)
    │
    ▼
6. 如果改进 > 0: 保存到 evolution/runs/<name>/<date>/
    │
    ▼
7. 用户确认后替换原技能
```

## 约束系统

为防止进化产生退化，设置以下约束：

| 约束 | 值 | 说明 |
|------|-----|------|
| 最大大小 | 15KB | 技能文件不超过此大小 |
| 最大增长 | 20% | 进化后大小不超过 baseline 的 1.2 倍 |
| 结构完整 | 必须有 | 至少包含 frontmatter、name、description |
| 最小改进 | > 0% | evolved score 必须高于 baseline |

## 评估数据来源

| 来源 | 说明 | 适用场景 |
|------|------|----------|
| `synthetic` | LLM 生成测试用例 | 通用，无历史数据时 |
| `golden` | 人工标注参考案例 | 精确，但需要人工投入 |
| `sessiondb` | 从历史会话挖掘 | 最贴近实际，但需要足够数据 |

默认使用 `synthetic`，积累足够数据后切换到 `sessiondb`。

## 进化记录

每次运行的结果保存在 `evolution/runs/<skill-name>/<date>/`：

```
evolution/runs/debug-rust-errors/
├── 20250819/
│   ├── baseline.md       # 原始技能
│   ├── evolved.md        # 优化后的技能
│   └── metrics.json      # 评估指标
└── history.json          # 历史趋势
```

### metrics.json

```json
{
  "skill_name": "debug-rust-errors",
  "baseline_score": 0.72,
  "evolved_score": 0.85,
  "improvement": 0.13,
  "baseline_size": 2456,
  "evolved_size": 2890,
  "constraints_passed": true,
  "elapsed_seconds": 142.5,
  "iterations": 10
}
```

## 配置

```yaml
evolution:
  enabled: false           # 默认关闭
  engine: gepa             # gepa | miprov2
  iterations: 10           # 优化迭代次数
  eval_source: synthetic   # synthetic | golden | sessiondb
  schedule: "0 3 * * 0"   # cron 表达式
```

---

## 技术实现

### 架构：`loom-evolution` 抽象 Crate

独立的 GEPA 优化引擎 crate，通过 trait 抽象外部依赖，`loom` 通过实现 trait 接入。

```
loom  ──depends-on──►  loom-evolution
                         │
                         ├─ trait EvolutionLlm  (loom::llm::LlmClient 适配)
                         ├─ trait SkillProvider  (loom::SkillRegistry 适配)
                         ├─ trait DatasetStore   (文件系统实现)
                         └─ GEPA 核心算法 (纯逻辑，无 IO 知识)
```

### Crate 结构

```
loom-evolution/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── traits.rs               # 核心 trait (EvolutionLlm, SkillProvider, DatasetStore)
│   ├── types.rs                 # 纯数据类型 (FitnessScore, EvalExample, etc.)
│   ├── config.rs                # EvolutionConfig
│   ├── optimizer/
│   │   ├── mod.rs
│   │   ├── gepa.rs              # GEPA 核心循环，依赖 trait
│   │   ├── mutation.rs          # LLM 变异策略
│   │   └── selection.rs         # 选择 + 重组
│   ├── constraints/
│   │   ├── mod.rs
│   │   └── validator.rs         # 内建约束 (size/growth/structure)
│   └── dataset/
│       ├── mod.rs
│       └── synthetic.rs         # 合成数据生成，依赖 EvolutionLlm
└── tests/
    ├── mock_llm.rs              # MockLlm 测试辅助
    ├── test_constraints.rs
    └── test_gepa_loop.rs
```

### 核心 Trait (`traits.rs`)

```rust
/// LLM 调用抽象。命名为 EvolutionLlm 避免与 loom::llm::LlmClient 冲突。
/// GEPA 只需 plain text completion，不需要 tool_call/streaming。
#[async_trait]
pub trait EvolutionLlm: Send + Sync {
    async fn complete(&self, req: LlmCompleteRequest) -> Result<LlmCompleteResponse>;
}

pub struct LlmCompleteRequest {
    pub model: String,
    pub system: Option<String>,
    pub user: String,
    pub temperature: Option<f64>,
}

pub struct LlmCompleteResponse {
    pub content: String,
    pub usage: Option<TokenUsage>,
}

/// 技能读写抽象
#[async_trait]
pub trait SkillProvider: Send + Sync {
    async fn load_skill(&self, name: &str) -> Result<SkillContent>;
    async fn save_skill(&self, name: &str, content: &SkillContent) -> Result<()>;
    async fn list_skills(&self) -> Result<Vec<SkillMeta>>;
}

pub struct SkillContent {
    pub frontmatter: String,
    pub body: String,
    pub raw: String,
}

pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

/// 评估数据集持久化抽象
#[async_trait]
pub trait DatasetStore: Send + Sync {
    async fn save_dataset(&self, skill_name: &str, dataset: &EvalDataset) -> Result<()>;
    async fn load_dataset(&self, skill_name: &str) -> Result<Option<EvalDataset>>;
}

/// 约束检查 hook（可选，可插拔）
pub trait ConstraintHook: Send + Sync {
    fn check(&self, evolved_text: &str, baseline_text: &str) -> Vec<ConstraintResult>;
}
```

### 纯数据类型 (`types.rs`)

`EvalExample`、`EvalDataset`、`FitnessScore`、`EvolutionResult`、`ConstraintResult`、`EvolutionConfig` 等。
详见 [data-structures.md](data-structures.md)。

### GEPA 核心循环 (`optimizer/gepa.rs`)

```rust
pub struct GepaOptimizer {
    config: Arc<EvolutionConfig>,
    llm: Arc<dyn EvolutionLlm>,
    constraints: Vec<Box<dyn ConstraintHook>>,
}

impl GepaOptimizer {
    pub fn new(config: Arc<EvolutionConfig>, llm: Arc<dyn EvolutionLlm>) -> Self;
    pub fn add_constraint(&mut self, hook: Box<dyn ConstraintHook>);

    pub async fn evolve(
        &self,
        baseline: &SkillContent,
        dataset: &EvalDataset,
    ) -> Result<EvolutionResult>;
}
```

GEPA 核心循环：

1. **初始化种群** — baseline body 作为 seed，LLM 变异生成 `population_size` 个候选
2. **循环 N 轮**：
   - *评估*: 每个候选在 trainset 上 LLM-as-judge 打分，收集 trace
   - *反思*: 分析失败 trace，LLM 生成改进建议
   - *变异*: 基于反思修改候选（改写段落/添加步骤/精简描述）
   - *选择*: 按适应度排名，保留 top-k
   - *重组*: 高分候选段落交叉，填充种群
3. **Holdout 评估** — baseline vs evolved
4. **约束验证**
5. **返回最优变体**

### 依赖 (`loom-evolution/Cargo.toml`)

```toml
[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
rand = "0.8"
chrono = { version = "0.4", features = ["serde"] }
```

不依赖 `reqwest`、`serde_yaml`、`clap` — 这些由 `loom` 侧提供。

### loom 侧适配

```rust
// loom/src/evolution/adapter.rs

// 关键适配器：将简单 completion trait 桥接到 loom 已有的 message-based LlmClient
struct LoomLlmClient { inner: Box<dyn loom::llm::LlmClient> }
impl EvolutionLlm for LoomLlmClient {
    // LlmCompleteRequest → &[Message] → inner.invoke() → 取 response.content
}

struct LoomSkillProvider<'a> { registry: &'a SkillRegistry }
impl SkillProvider for LoomSkillProvider<'_> { /* ... */ }

struct FsDatasetStore { base: PathBuf }
impl DatasetStore for FsDatasetStore { /* ... */ }

struct LoomConstraints { config: EvolutionConfig }
impl ConstraintHook for LoomConstraints { /* size, growth, structure checks */ }

// 入口
pub async fn evolve_skill(name: &str, config: &AppConfig) -> Result<EvolutionResult> {
    let llm = Arc::new(LoomLlmClient::from_config(config));
    let provider = LoomSkillProvider::from_registry(&registry);
    let store = FsDatasetStore::new(config.data_dir.join("evolution"));

    let mut optimizer = GepaOptimizer::new(Arc::new(evo_config), llm);
    optimizer.add_constraint(Box::new(LoomConstraints::new(evo_config)));

    let skill = provider.load_skill(name).await?;
    let dataset = build_or_load_dataset(&store, &skill, &evo_config).await?;
    optimizer.evolve(&skill, &dataset).await
}
```

### 实施阶段

| Phase | 内容 | 时间 | 依赖 |
|-------|------|------|------|
| **E1** | Crate 骨架 + traits + types + config | 1-2 天 | — |
| **E2** | 数据管道 (dataset, constraints) | 1-2 天 | E1 |
| **E3** | GEPA 核心 (optimizer, mutation, selection, judge) | 2-3 天 | E1 |
| **E4** | loom 适配层 + CLI + 集成测试 | 1-2 天 | E1-E3 |

总工期：~5-9 天。
