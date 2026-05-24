# 技能进化使用指南

> 让你的 AI agent 技能越用越好。基于 GEPA 算法自动优化技能质量。

## 什么是技能进化？

技能进化是 Loom 的核心能力之一。它会：

1. 收集技能的实际使用数据
2. 用 LLM-as-judge 评估技能表现
3. 自动生成改进版本
4. 通过约束检查确保不退化
5. 由用户确认后替换

```
你的技能（baseline）
    ↓
构建评估数据集（测试用例）
    ↓
GEPA 多轮优化（失败→反思→改进）
    ↓
约束验证（大小/结构/安全）
    ↓
你确认 → 替换技能 ✅
```

## 快速开始

### 1. 开启进化功能

在 `loom.yaml`（或项目目录下的 `.loom/config.yaml`）中添加：

```yaml
evolution:
  enabled: true
  model: "openai/gpt-4.1"
  schedule: "weekly"
  max_cost_per_run_usd: 10.0
```

### 2. 进化一个技能

```bash
# 进化指定技能
loom skills evolve my-skill-name

# 查看进化历史
loom evolve status

# 运行所有待进化技能
loom evolve run
```

### 3. 确认或拒绝进化结果

```bash
# 查看进化对比
loom evolve compare my-skill-name

# 接受进化（替换当前技能）
loom evolve accept my-skill-name

# 拒绝进化（保留原始技能）
loom evolve reject my-skill-name
```

## 评估数据集

进化需要测试用例来评估技能表现。有三种来源：

### 来源 A：自动生成（推荐起步）

不需要手动操作。进化运行时，系统会用 LLM 读取技能文件，自动生成 15-30 个测试用例：

- **task_input**：模拟用户可能给出的任务描述
- **expected_behavior**：评分标准（rubric），而非精确输出

### 来源 B：手工黄金集（关键技能）

为重要技能手动编写高质量测试用例：

```bash
# 创建数据集目录
mkdir -p ~/.loom/data/evolution/datasets/my-skill/

# 编写 golden.jsonl
echo '{"task_input":"修复编译错误","expected_behavior":"应定位错误位置并给出修复建议","difficulty":"Medium"}' > ~/.loom/data/evolution/datasets/my-skill/golden.jsonl
```

### 来源 C：会话挖掘（有历史后）

从历史会话中自动提取真实使用数据，用 LLM 打分，高分样本作为正例。

## 约束系统

进化不会"野蛮生长"。每个改进版本必须通过：

| 约束 | 说明 |
|------|------|
| *大小预算* | 进化后不超过原始的 1.2 倍 |
| *结构完整* | YAML frontmatter 字段（name, description）不缺失 |
| *安全保持* | 不会移除已有的安全检查和错误处理 |
| *改进阈值* | 进化版本评分必须高于原始版本 |

## 版本管理与回滚

每次进化都会自动备份原始技能：

```bash
# 查看可用备份
loom evolve backups my-skill-name

# 回滚到某个版本
loom evolve rollback my-skill-name --version 20250615_103000
```

## 配置参考

```yaml
evolution:
  enabled: true                    # 是否开启进化
  model: "openai/gpt-4.1"         # 用于优化的模型
  schedule: "weekly"               # weekly / monthly / manual
  max_cost_per_run_usd: 10.0      # 每次运行最大成本
  max_iterations: 10               # GEPA 最大迭代轮数
  candidates_per_iter: 5           # 每轮候选数
  auto_accept: false               # 自动接受（建议 false）
  triage_threshold: 0.7            # 成功率低于此值触发进化

  constraints:
    max_size_ratio: 1.2            # 大小限制
    min_semantic_similarity: 0.7   # 语义保持阈值
    check_semantic: false          # 是否检查语义（需 embedding）

  rubric_weights:
    procedure: 0.3                 # 流程遵循权重
    quality: 0.5                   # 输出质量权重
    conciseness: 0.2               # 简洁性权重
```

## 成本预估

| 操作 | 预计成本 |
|------|----------|
| 自动数据集生成 | ~$0.50 |
| 单次进化（10 轮） | ~$2-10 |
| 回归测试 | ~$1-3 |
| **完整进化周期** | **~$5-15** |

建议：从小数据集（10-15 个样本）开始，效果好再扩展。

## 进化层级

当前支持 Tier 1（技能文件），后续将扩展：

| 层级 | 目标 | 状态 |
|------|------|------|
| Tier 1 | 技能文件 (SKILL.md) | ✅ 已实现 |
| Tier 2 | 工具描述 | 📋 规划中 |
| Tier 3 | 系统 prompt 片段 | 📋 规划中 |
| Tier 4 | 代码文件 | ⏸️ 暂缓 |

## 工作原理：GEPA 算法

GEPA（Gradient-free Evolutionary Prompt Optimization with Aggregations）的核心流程：

```
Round 1: 生成候选 → 评估 → 记录失败轨迹
Round 2: 读取失败轨迹 → 反思原因 → 生成改进候选
Round 3: 重复...
```

关键特点：
- **最少 3 个样本**即可开始优化
- **失败驱动**：从失败案例中学习，不是随机突变
- **多轮反思**：每轮分析失败原因，针对性改进
- **5-10 轮通常收敛**

## 常见问题

**Q: 进化会让技能变差吗？**
不会。所有进化变体必须通过约束检查 + 评分高于 baseline + 用户确认，才会替换。

**Q: 需要多少测试用例？**
最少 3 个即可开始。建议 15-30 个获得稳定效果。

**Q: 每次进化要花多少钱？**
约 $5-15，取决于数据集大小和迭代轮数。可在配置中设置 `max_cost_per_run_usd` 上限。

**Q: 能回滚吗？**
可以。每次进化自动备份原始技能，用 `loom evolve rollback` 随时恢复。

## 相关文档

- [GEPA 技术详解](gepa.md) — 算法原理和实现细节
- [进化方案完善](gepa-comprehensive.md) — 数据集构建、约束系统、部署流程
- [命令参考](commands.md) — 所有 CLI 命令
- [配置参考](config.md) — 所有配置项
- [技能系统](skills.md) — 技能文件格式和生命周期
