# 进化子系统 — 缺失分析与实施方案

> 本文档梳理 `loom-evolution` crate 及其上游集成中所有缺失的模块，给出分阶段实施方案。
> 现有文档：[roadmap.md](roadmap.md)（Phase 规划）、[gepa.md](gepa.md)（GEPA 技术设计）、[gepa-comprehensive.md](gepa-comprehensive.md)（完善方案）。

---

## 一、现状总结

### 已实现（`loom-evolution` crate）

| 模块 | 文件 | 功能 | 完成度 |
|------|------|------|--------|
| types | `types.rs` | `EvalExample`, `EvolutionResult`, `ConstraintResult`, `ExecutionTrace`, `EvolutionConfig` 等核心类型 | ✅ 完整 |
| constraints | `constraints.rs` | 大小预算、结构完整（YAML frontmatter）、安全关键词保持 | ✅ 完整 |
| dataset | `dataset.rs` | `FsDatasetStore`: JSONL 读写 + train/val/holdout 分割 | ✅ 完整 |
| judge | `judge.rs` | LLM-as-judge 评分 prompt + 解析 + 适应度计算 | ✅ 完整 |
| optimizer | `optimizer.rs` | `GepaOptimizer`: 多轮候选生成 + 评估 + 反思循环 | ⚠️ 基本可用，有局限（见下） |
| deploy | `deploy.rs` | `RunStore`: 进化运行记录持久化 + 备份/回滚 | ✅ 完整 |

### optimizer.rs 已知局限

1. **每轮只生成 1 个候选**（`generate_candidates` 返回 `vec![candidate]`），GEPA 文档要求 `candidates_per_iter: 5`，缺少多样性和重组逻辑
2. **无反思步骤**：`mutation_prompt` 虽然接收 `failed_traces`，但 prompt 中没有要求 LLM 先分析失败原因再生成改进
3. **无语义保持约束**：`ConstraintConfig.check_semantic` 默认 false，缺少 embedding 相似度实现
4. **无成本追踪**：`EvolutionResult.cost_usd` 始终为 None
5. **无回归门控**：`regression_check` 始终为 None，golden tasks 测试未实现

### CLI 集成层 — 全部缺失

`cli/src/` 中没有任何 evolution 相关命令实现。`agent.rs:834` 的 `skill_registry: None` 是唯一的残留引用。

---

## 二、缺失模块详解

### M1: 会话查询扩展（Session Query Extensions）

**所属 Phase**: 2（Review Agent 前置依赖）

**现状**: `cli/src/session.rs` 已有完整的 `SessionManager`，基于 SQLite `memory.db` 的 `checkpoints` 表：
- `list_sessions()` — 列出所有会话（title、时间、checkpoint 数量）
- `show_session()` — 获取会话详情（首条用户消息、最后助手回复、消息总数）
- `cat_session()` — 返回完整 `ReActState` checkpoint 序列，反序列化后包含全部 `messages: Vec<Message>`

**不需要新建存储**。会话数据已在 `~/.loom/memory.db` 的 `checkpoints` 表中持久化，`cat_session()` 返回的 `ReActState.messages` 可直接被 Review Agent 和会话挖掘器消费。

**需要扩展的查询方法**（在 `SessionManager` 上新增）:

```rust
// cli/src/session.rs 扩展

impl SessionManager {
    /// 列出指定时间之后的会话（Review Agent 处理最近 N 天）
    pub fn list_sessions_since(&self, since: DateTime<Utc>) -> Result<Vec<SessionInfo>, String>;

    /// 按关键词搜索会话（会话挖掘器按技能名/工具名筛选）
    /// 注意：checkpoints 表的 payload 是序列化 blob，FTS5 无法直接索引。
    /// 实现方案：应用层过滤 — list_sessions_since → 逐个 cat_session → 内存搜索 messages。
    /// 如果性能不足，可额外维护一张 sessions_fts 虚拟表，在写入 checkpoint 时同步提取文本。
    pub fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<SessionInfo>, String>;
}
```

**Review Agent 数据流**: `SessionManager::cat_session(id)` → `Vec<ReActState>` → 提取 `messages` → 构建 Review Prompt

**会话挖掘器数据流**: `SessionManager::search_sessions(skill_name)` → 筛选相关会话 → `cat_session()` → 提取 (user_input, assistant_response) 对

**待确认**: `Message::Tool` 变体是否携带完整的工具调用输入/输出。如果不携带，需要在 `ReActState` 序列化时补充 tool_io 字段。

**工作量**: 0.5-1 天（仅扩展查询方法，无新存储）

---

### M2: 后台审查（Review Agent）

**所属 Phase**: 2

**现状**: 完全缺失。无 Review Prompt 构建、无 LLM 调用、无输出解析。

**需要实现**:

1. **Review Prompt 模板**: 参考 Hermes `_COMBINED_REVIEW_PROMPT`（`agent/background_review.py:147-230`）。每轮对话后 daemon 线程 fork agent，回放 messages 快照。Prompt 指示 agent 按 4 级优先级链操作技能：patch 已加载 → patch umbrella → 添加 support file → create 新 umbrella
2. **Review Agent 行为**: agent 自主决定 action（非 JSON 输出解析），直接调用 `skill_manage` 工具（action=create|edit|patch|write_file）和 memory 工具。Hermes 的 fork agent 通过 `is_background_review()` 标记创建来源
3. **Memory 写入**: 追加到 `USER.md` / `PROJECT.md` / `FACTS.md`
4. **Skill 自动创建**: 无阈值限制。每轮 review 都可创建技能，只要 review agent 判断有新学习值得保存。创建的技能标记为 `agent-created`，由 Curator 后续维护（consolidate/archive）
5. **异步调度**: 会话结束后 daemon thread 触发，不阻塞用户。Fork agent 继承父 agent 的 provider/model（命中 prefix cache）
6. **Curator 联动**: agent-created 技能由 `agent/curator.py` 的 consolidation pass 处理——合并狭窄技能为 umbrella、archive 过时技能。Curator 不碰 bundled/hub-installed/pinned 技能

**Review Prompt 核心结构**:

```
你是 Levol 的审查 Agent。分析以下对话，提取：

1. 用户偏好（语言、工具、风格）
2. 项目事实（技术栈、架构决策）
3. 可复用的工作模式（≥5次重复 → 技能候选）

## 会话内容
{session_jsonl}

## 输出格式（JSON）
{
  "memory_updates": [
    {"file": "USER.md", "action": "append", "content": "..."}
  ],
  "skill_suggestions": [
    {"name": "...", "description": "...", "triggers": [...], "body": "..."}
  ]
}
```

**文件位置**: `cli/src/run/review.rs`（新文件）

**依赖**: M1（`SessionManager` 扩展方法）

**工作量**: 2-3 天

---

### M3: 记忆系统（Memory）

**所属 Phase**: 2（与 M2 协同）

**现状**: 文档设计完整（`docs/evolution/memory.md`），但无代码实现。

**需要实现**:

```
~/.loom/data/memory/
├── USER.md          # 用户画像
├── PROJECT.md       # 项目上下文
└── FACTS.md         # 通用事实
```

**接口**:

```rust
pub struct MemoryStore {
    base_dir: PathBuf,
    config: MemoryConfig,
}

impl MemoryStore {
    pub fn load(&self, file: MemoryFile) -> Result<String>;
    pub fn append(&self, file: MemoryFile, content: &str) -> Result<()>;
    pub fn replace(&self, file: MemoryFile, content: &str) -> Result<()>;
    pub fn search(&self, query: &str) -> Result<Vec<MemoryMatch>>;
    pub fn truncate_to_limit(&self, file: MemoryFile, max_chars: usize) -> Result<()>;
    // 截断策略：保留文件头部的结构化元数据（YAML frontmatter），对正文部分
    // 按“从旧到新”移除条目（每条以 --- 分隔），直到总长度 <= max_chars。
    // 如果单条超过 max_chars，调用 LLM 生成摘要替换该条。
}

pub enum MemoryFile { User, Project, Facts }
```

**注入时机**: `cli/src/run/agent.rs` 的 system prompt 组装阶段，读取 memory 文件内容注入。
注入时需检查总长度，若超过 `max_memory_chars`（默认 4000），按优先级截断：FACTS > PROJECT > USER。

**CLI 命令**: `levol memory show/edit/search`

**工作量**: 1-2 天

---

### M4: 技能系统（Skill CRUD + 自动匹配）

**所属 Phase**: 3

**现状**: `agent.rs:768` 有 `skill_registry: None` 字段，无其他实现。

**需要实现**:

```
~/.loom/data/skills/
├── auto/                    # Review Agent 自动创建
│   └── debug-rust-errors/
│       └── SKILL.md
├── curated/                 # 用户手动创建/evolved
│   └── deploy-vercel/
│       └── SKILL.md
└── evolution/               # 进化子系统管理
    └── runs/                # 进化运行记录
```

**接口**:

```rust
pub struct SkillRegistry {
    base_dir: PathBuf,
}

impl SkillRegistry {
    pub fn list(&self) -> Result<Vec<SkillMeta>>;
    pub fn load(&self, name: &str) -> Result<SkillContent>;
    pub fn save(&self, name: &str, content: &SkillContent) -> Result<()>;
    pub fn delete(&self, name: &str) -> Result<()>;
    pub fn find_matching(&self, query: &str, threshold: f64) -> Result<Vec<SkillContent>>;
}

pub struct SkillContent {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub lifecycle: Lifecycle,     // Active, Stale, Archived
    pub source: Source,           // Auto, Manual, Evolved
    pub body: String,
    pub raw: String,
}
```

**自动匹配逻辑**: 用户消息 → 提取关键词 → 匹配 `triggers`（余弦相似度或精确匹配 ≥ `threshold`，默认 0.6）→ 注入匹配技能到 agent context。匹配时还应考虑项目上下文（如检测到 Cargo.toml 则提升 Rust 相关技能的权重）。

**CLI 命令**: `levol skills list/show/create/edit/delete`

**工作量**: 2-3 天

---

### M5: Curator（技能生命周期管理）

**所属 Phase**: 4

**现状**: 完全缺失。

**需要实现**:

1. **定期扫描**: 遍历 `skills/auto/` 所有 SKILL.md
2. **Stale 检测**: 30 天未用 → 标记 `lifecycle: stale`
3. **Archive 归档**: 90 天未用 → 移到 `skills/curated/`，标记 `lifecycle: archived`
4. **重叠检测**: 比较 triggers + description，相似度 > 阈值 → 报告
5. **状态持久化**: `~/.loom/data/curator/state.json`

**接口**:

```rust
pub struct Curator {
    skill_registry: SkillRegistry,
    config: CuratorConfig,
}

impl Curator {
    pub fn run(&self, use_llm: bool) -> Result<CuratorReport>;
}

pub struct CuratorReport {
    pub active: usize,
    pub stale: Vec<String>,
    pub archived: Vec<String>,
    pub overlapping: Vec<(String, String, f64)>,
}
```

**CLI 命令**: `levol curator run [--use-llm]`

**工作量**: 1-2 天

---

### M6: 合成数据集生成器

**所属 Phase**: 6（Week 2）

**现状**: 完全缺失。GEPA 优化器需要评估数据集才能运行。

**需要实现**:

```rust
// loom-evolution/src/synthetic.rs (新文件)

pub async fn generate_dataset(
    llm: &dyn EvolutionLlm,
    skill_content: &str,
    count: usize,
) -> Result<Vec<EvalExample>> {
    // 1. 构建 prompt: "根据以下技能文件，生成 N 个测试用例"
    // 2. 解析 LLM 输出为 Vec<EvalExample>
    // 3. 自动分配 Easy/Medium/Hard 难度
}
```

**Prompt 设计**:

```
你是一个测试用例生成器。根据以下技能文件，生成 {count} 个多样化的测试用例。

每个用例包含：
- task_input: 用户可能给出的任务描述（多样化，覆盖边界情况）
- expected_behavior: 评分标准（rubric），不是精确输出
- difficulty: Easy / Medium / Hard

## 技能文件
{skill_content}

输出 JSON 数组。
```

**工作量**: 1 天

---

### M7: 会话挖掘器

**所属 Phase**: 6（Week 2）

**现状**: 完全缺失。

**需要实现**:

```rust
// loom-evolution/src/miner.rs (新文件)

pub async fn mine_from_sessions(
    session_store: &dyn SessionStore,
    skill_name: &str,
    llm: &dyn EvolutionLlm,
    max_samples: usize,
) -> Result<Vec<EvalExample>> {
    // 1. 加载历史会话，筛选涉及该技能的会话
    // 2. 提取 (user_input, assistant_response) 对
    // 3. LLM 打分，筛选高质量样本
    // 4. 转换为 EvalExample 格式
}
```

**工作量**: 1-2 天

---

### M8: 回归门控（Regression Gate）

**所属 Phase**: 6（Week 2）

**现状**: `EvolutionResult.regression_check` 始终为 None。

**需要实现**:

```rust
// loom-evolution/src/regression.rs (新文件)

pub struct RegressionGate {
    golden_tasks: Vec<EvalExample>,
    threshold: f64,  // 默认 0.05，即允许 5% 的性能下降
}

impl RegressionGate {
    pub async fn check(
        &self,
        llm: &dyn EvolutionLlm,
        baseline: &str,
        evolved: &str,
    ) -> Result<RegressionResult> {
        // 1. 在 golden_tasks 上分别评估 baseline 和 evolved
        // 2. 如果 evolved 在任意 golden task 上退化 > threshold → 拒绝
    }
}

pub struct RegressionResult {
    pub passed: bool,
    pub baseline_scores: Vec<f64>,
    pub evolved_scores: Vec<f64>,
    pub max_regression: f64,
}
```

**golden tasks 来源**:
1. 用户手工编写的高优先级测试用例，放在 `~/.loom/data/evolution/datasets/<skill>/golden.jsonl`
2. M6 合成数据集生成器可通过 `generate_dataset(..., purpose: Golden)` 模式自动生成 golden 子集——从合成数据中挑选质量最高（judge 评分 ≥ 0.9）的样本，减少手工维护成本

**工作量**: 1 天

---

### M9: CLI 命令集成

**所属 Phase**: 6（Week 3）

**现状**: 完全缺失。CLI 中无任何 evolution 子命令。

**需要实现的命令**:

```bash
# 技能管理
levol skills list                           # 列出所有技能
levol skills show <name>                    # 查看技能详情
levol skills create <name>                  # 创建技能
levol skills edit <name>                    # 编辑技能
levol skills delete <name>                  # 删除技能

# 技能进化
levol skills evolve <name>                  # 进化单个技能
levol skills evolve <name> --source synthetic   # 指定数据来源
levol skills evolve <name> --iterations 5       # 指定迭代次数

# 进化管理
levol evolve run                            # 运行所有待进化技能
levol evolve status                         # 查看进化历史
levol evolve compare <name>                 # diff 对比
levol evolve accept <name>                  # 接受进化结果
levol evolve reject <name>                  # 拒绝进化结果
levol evolve backups <name>                 # 查看历史版本
levol evolve rollback <name> --version XXX  # 回滚

# Curator
levol curator run                           # 运行 curator
levol curator run --use-llm                 # LLM 辅助

# Memory
levol memory show                           # 查看所有记忆
levol memory edit                           # 编辑记忆
levol memory search <query>                 # 搜索记忆
```

**实现位置**: `cli/src/subcommands.rs`（扩展现有子命令框架）

**工作量**: 2-3 天

---

### M10: 配置解析与调度

**所属 Phase**: 6（Week 3）

**现状**: `.loom/config.yaml` 有 `evolution:` 段，但未被读取。

**需要实现**:

1. **配置解析**: 读取 `evolution:` 段，映射到 `EvolutionConfig`
2. **调度器**: 解析 `schedule: "weekly"` / cron 表达式，在后台线程定期触发 `levol evolve run`
3. **成本控制**: 解析 `max_cost_per_run_usd`，优化过程中累计 token 费用，超限中止

**调度方案**:

```rust
pub struct EvolutionScheduler {
    config: EvolutionConfig,
    optimizer: GepaOptimizer,
    skill_registry: SkillRegistry,
}

impl EvolutionScheduler {
    pub fn start(&self) -> JoinHandle<()> {
        // 1. 解析 schedule 为 cron 表达式
        // 2. 启动后台线程，按 cron 触发
        // 3. 每次触发：扫描所有 active 技能 → 评估是否需要进化 → 运行 GEPA
    }

    pub fn should_evolve(&self, skill: &SkillContent) -> bool {
        // 1. 检查是否有进化记录
        // 2. 检查距上次进化是否超过 schedule 间隔
        // 3. 检查 usage stats（如果成功率 < triage_threshold → 优先进化）
    }
}
```

**工作量**: 2 天

---

### M11: Optimizer 增强

**所属 Phase**: 6（Week 2，与 M6-M8 并行）

**现状**: `optimizer.rs` 基本可用但有多处局限。

**需要增强**:

1. **多候选生成**: 每轮生成 `candidates_per_iter`（默认 5）个候选，而非 1 个
2. **反思步骤**: 在 mutation prompt 中先要求 LLM 分析失败原因，再生成改进
3. **语义保持**: 集成 embedding API，计算 evolved vs baseline 的余弦相似度 > 0.7
4. **成本追踪**: 从 LLM 响应中提取 token usage，累计 `cost_usd`
5. **早停策略**: 连续 N 轮无改进 → 提前终止（当前只有 ≥3 轮 baseline 才停）

**工作量**: 2 天

---

## 三、实施顺序

按依赖关系排列，标注前置条件：

```
Phase A（基础层，无依赖）
├── M1: 会话查询扩展          0.5-1 天
├── M3: 记忆系统              1-2 天
├── M6: 合成数据集生成器      1 天
└── M11: Optimizer 增强       2-3 天   ← 提前，M6 需要多候选才能有效验证

Phase B（Review，依赖 M1+M3）
└── M2: 后台审查              3-5 天   ← 含 prompt 调优，估算上调

Phase C（技能，依赖 M2）
├── M4: 技能系统              2-3 天
└── M5: Curator              1-2 天

Phase D（进化增强，依赖 M6+M11）
├── M7: 会话挖掘器            1-2 天   ← 也依赖 M1 的 search_sessions
└── M8: 回归门控              1 天

Phase E（集成，依赖全部）
├── M9: CLI 命令              3-4 天   ← ~20 个子命令，估算上调
└── M10: 配置解析与调度        2 天
```

**总工作量**: ~18-28 天（1 人全职）

**关键路径**: M1 → M2 → M4 → M9 → M10

**最小可行路径（先跑通一次进化）**: M6 → M11 → M9（部分）→ 手动触发，约 6-8 天

---

## 四、文件变更清单

### 新增文件

| 文件 | 模块 | 说明 |
|------|------|------|
| `cli/src/run/review.rs` | M2 | Review Agent |
| `cli/src/run/memory.rs` | M3 | 记忆系统 |
| `cli/src/run/skill_registry.rs` | M4 | 技能注册表 |
| `cli/src/run/curator.rs` | M5 | Curator |
| `loom-evolution/src/synthetic.rs` | M6 | 合成数据集生成 |
| `loom-evolution/src/miner.rs` | M7 | 会话挖掘 |
| `loom-evolution/src/regression.rs` | M8 | 回归门控 |

### 修改文件

| 文件 | 变更 |
|------|------|
| `cli/src/session.rs` | M1 扩展 SessionManager 查询方法 |
| `cli/src/run/agent.rs` | 注入 Memory + SkillRegistry |
| `cli/src/subcommands.rs` | 新增 evolution/skills/curator/memory 子命令 |
| `cli/src/args.rs` | 新增 CLI 参数定义 |
| `loom-evolution/src/optimizer.rs` | 多候选、反思、成本追踪 |
| `loom-evolution/src/lib.rs` | 导出新增模块 |
| `loom-evolution/Cargo.toml` | 新增依赖（如需要） |
| `.loom/config.yaml` | 解析 evolution/skills/memory/curator 配置 |

---

## 五、风险与缓解


---

## 五、风险与缓解

| 风险 | 概率 | 缓解 |
|------|------|------|
| Review Agent 提取质量差 | 高 | 从简单规则起步（关键词匹配），LLM 辅助逐步迭代 |
| 技能自动匹配误触发 | 中 | 严格 triggers 匹配 + 置信度阈值 + 用户可关闭 |
| GEPA 优化成本超预期 | 中 | `max_cost_per_run_usd` 硬上限 + 每轮检查 |
| 会话 jsonl 文件过大 | 低 | 定期压缩归档，只保留最近 N 天 |
| 并发写入冲突 | 低 | 文件锁或 single-writer 模式 |
| LLM 调用失败（Review / 数据集生成 / 评估） | 中 | 通用降级：重试 3 次指数退避 → 降级为规则提取 / 跳过本轮 → 记录 failure 到 RunStore |
| 调度器生命周期 | 中 | CLI 工具非长驻进程，cron 后台线程随进程退出消失。建议改为 `levol evolve run --auto` 非守护模式，或依赖系统级 cron/launchd 触发 CLI 命令 |

---

## 六、错误处理与降级策略

所有涉及 LLM 调用的模块（M2 Review Agent、M6 合成数据集、M7 会话挖掘、M8 回归门控、M11 Optimizer）遵循统一模式：

1. **重试**: LLM 调用失败时，指数退避重试最多 3 次（间隔 2s / 4s / 8s）
2. **降级**: 重试耗尽后，根据模块选择降级行为：
   - M2 Review Agent → 跳过本次会话审查，记录 `review_skipped` 到日志
   - M6 合成数据集 → 返回已成功生成的部分样本 + 警告
   - M7 会话挖掘 → 跳过失败的会话，继续处理其他会话
   - M8 回归门控 → 保守拒绝（`passed: false`），避免未经验证的 evolved content 上线
   - M11 Optimizer → 终止当前迭代，返回已有最佳候选
3. **日志**: 所有失败和降级事件通过 `tracing::warn!` 记录，包含 module、error kind、降级决策
4. **用户通知**: CLI 层汇总失败信息，在命令结束时输出摘要
