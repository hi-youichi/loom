# 进化子系统配置

`.loom/config.yaml` 或 `loom.yaml` 中与进化相关的所有配置项。

## 完整配置

```yaml
memory:
  enabled: true
  max_chars: 8000                # 单个记忆文件最大字符数
  max_memory_chars: 4000         # 注入 system prompt 的总记忆字符上限

skills:
  auto_create: true              # Review Agent 自动创建技能
  auto_create_threshold: 5       # 工具调用重复次数阈值

curator:
  stale_days_auto: 60            # Auto 技能未使用天数 → stale
  stale_days_manual: 30          # Manual 技能未使用天数 → stale
  archive_days: 90               # Stale 后归档天数
  overlap_threshold: 0.7         # Jaccard 相似度重叠阈值

review:
  enabled: true                  # 会话结束后是否自动审查
  max_session_chars: 12000       # 审查时截断会话长度
  auto_create_threshold: 5       # 自动创建技能阈值

evolution:
  enabled: true
  model: "openai/gpt-4.1"       # 用于优化的模型
  schedule: "manual"             # manual / weekly / monthly
  max_cost_per_run_usd: 10.0    # 每次运行最大成本
  max_iterations: 10             # GEPA 最大迭代轮数
  candidates_per_iter: 5         # 每轮候选数
  max_cost_usd: 10.0             # 成本上限（超出停止）

  constraints:
    max_size_ratio: 1.2          # 大小限制
    min_semantic_similarity: 0.7 # 语义保持阈值
    check_semantic: false        # 是否检查语义（需 embedding）

  rubric_weights:
    procedure: 0.3               # 流程遵循权重
    quality: 0.5                 # 输出质量权重
    conciseness: 0.2             # 简洁性权重

  dataset_path: null             # 自定义数据集路径（默认 ~/.loom/data/evolution/datasets/<skill>/）
```

## 配置项说明

### memory

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | true | 是否启用记忆系统 |
| `max_chars` | int | 8000 | 单个文件最大字符数，超出时按条目从旧到新移除 |
| `max_memory_chars` | int | 4000 | 注入 prompt 的总记忆字符上限，按 FACTS > PROJECT > USER 优先级截断 |

### skills

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `auto_create` | bool | true | Review Agent 是否自动创建技能 |
| `auto_create_threshold` | int | 5 | 检测到同一模式出现次数 ≥ 此值时自动创建 |

### curator

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `stale_days_auto` | int | 60 | Auto 技能未使用天数阈值 |
| `stale_days_manual` | int | 30 | Manual 技能未使用天数阈值 |
| `archive_days` | int | 90 | Stale 技能归档天数 |
| `overlap_threshold` | float | 0.7 | Jaccard 重叠报告阈值 |

### review

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | true | 是否在会话结束后自动运行 Review Agent |
| `max_session_chars` | int | 12000 | 截断会话内容长度 |
| `auto_create_threshold` | int | 5 | 自动创建技能阈值 |

### evolution

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | true | 是否启用进化 |
| `model` | string | "openai/gpt-4.1" | 用于优化的 LLM 模型 |
| `schedule` | string | "manual" | 触发方式：manual / weekly / monthly |
| `max_cost_per_run_usd` | float | 10.0 | 每次运行最大成本 |
| `max_iterations` | int | 10 | GEPA 最大迭代轮数 |
| `candidates_per_iter` | int | 5 | 每轮生成候选数 |
| `max_cost_usd` | float | 10.0 | 成本上限，超出后停止迭代 |

### evolution.constraints

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_size_ratio` | float | 1.2 | evolved/baseline 大小比上限 |
| `min_semantic_similarity` | float | 0.7 | 语义保持阈值（需 check_semantic: true） |
| `check_semantic` | bool | false | 是否启用语义检查 |

### evolution.rubric_weights

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `procedure` | float | 0.3 | 流程遵循度权重 |
| `quality` | float | 0.5 | 输出质量权重 |
| `conciseness` | float | 0.2 | 简洁性权重 |

## 环境变量

| 变量 | 说明 |
|------|------|
| `LOOM_HOME` | Loom 数据根目录（默认 `~/.loom`） |
| `HOME` | 用于计算默认路径 |

## 相关文档

- [使用指南](usage.md) — 快速开始和完整流程
- [命令参考](commands.md) — 所有 CLI 命令
- [技能系统](skills.md) — 技能文件格式
