# 进化相关数据结构

> 完整技术选型和项目结构见 [dev/tech-stack.md](../dev/tech-stack.md)。

## loom-evolution crate 类型

GEPA 优化引擎的核心类型定义在 `loom-evolution` crate 中（[gepa.md](gepa.md)）。

### 评估数据集

```rust
// loom-evolution::types
struct EvalExample {
    task_input: String,
    expected_behavior: String,
    difficulty: Difficulty,    // Easy, Medium, Hard
    category: String,
}

struct EvalDataset {
    train: Vec<EvalExample>,
    val: Vec<EvalExample>,
    holdout: Vec<EvalExample>,
}
```

### 适应度评分

```rust
// loom-evolution::types
struct FitnessScore {
    correctness: f64,           // 0-1
    procedure_following: f64,   // 0-1
    conciseness: f64,           // 0-1
    length_penalty: f64,        // 0-0.3
    feedback: String,
}
// composite = max(0, 0.5*correctness + 0.3*procedure_following + 0.2*conciseness - length_penalty)
```

### 进化结果

```rust
// loom-evolution::types
struct EvolutionResult {
    skill_name: String,
    baseline_score: f64,
    evolved_score: f64,
    improvement: f64,
    baseline_size: usize,
    evolved_size: usize,
    constraints_passed: bool,
    elapsed_seconds: f64,
    iterations: usize,
}
```

## loom 侧数据结构

以下类型由 `loom` crate 定义，用于进化子系统与核心功能的集成。

### 技能元数据

```rust
// loom::skill — 已实现
struct SkillMetadata {
    name: String,
    description: String,
}
```

技能生命周期和来源由 `loom` 管理（[skills.md](skills.md)）：

```rust
enum Lifecycle { Draft, Active, Stale, Archived }
enum SkillSource { Auto, Manual, Evolved }
```

### Review 结果

```rust
struct ReviewResult {
    memory_updates: Vec<MemoryAction>,
    skill_updates: Vec<SkillAction>,
    summary: String,
}
```

详见 [review.md](review.md)。
