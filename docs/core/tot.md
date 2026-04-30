# ToT (Tree of Thoughts) 运行模式

基于树状思维推理的智能体运行模式，通过多候选思维生成和评估实现复杂任务的分支探索。

## ToT vs ReAct 对比

| 特性 | ToT (Tree of Thoughts) | ReAct (Reasoning + Acting) |
|------|----------------------|---------------------------|
| **推理方式** | 树状分支探索 | 线性循环推理 |
| **思维生成** | 每轮生成 2-3 个候选 | 每轮生成单一思维 |
| **选择机制** | 评估打分后选择最佳 | 按顺序执行 |
| **回溯能力** | ✅ 支持候选回溯 | ❌ 无回溯机制 |
| **适用场景** | 复杂推理、多种方案 | 工具调用、实时交互 |
| **计算成本** | 较高（多候选评估） | 较低（单路径执行） |

## 核心概念

### ToT 循环流程

ToT 采用 **Expand → Evaluate → Select** 循环模式：

1. **Expand (扩展节点)**: LLM 生成 2-3 个候选思维方案
2. **Evaluate (评估节点)**: 基于规则对候选方案打分
3. **Select (选择阶段)**: 选择得分最高的候选继续执行
4. **Backtrack (回溯机制)**: 失败时可尝试其他候选

### 核心组件

**TotState**: ToT 状态管理
```rust
pub struct TotState {
    pub core: ReActState,           // 核心执行状态
    pub tot: TotExtension,          // ToT 特定扩展
}

pub struct TotExtension {
    pub depth: u32,                 // 当前树深度
    pub candidates: Vec<TotCandidate>, // 当前候选列表
    pub chosen_index: Option<usize>,   // 选中的候选索引
    pub tried_indices: Vec<usize>,     // 已尝试的候选索引
    pub suggest_backtrack: bool,       // 是否建议回溯
}
```

**TotCandidate**: 单个候选方案
```rust
pub struct TotCandidate {
    pub thought: String,            // 思维内容
    pub tool_calls: Vec<ToolCall>,  // 关联工具调用
    pub score: Option<f32>,         // 评估分数
}
```

**TotRunnerConfig**: 运行配置
```rust
pub struct TotRunnerConfig {
    pub max_depth: u32,              // 最大探索深度
    pub candidates_per_step: u32,    // 每步候选数量 (2-3)
    pub research_quality_addon: bool, // 研究质量增强
}
```

## 代码示例

### 基础 ToT 智能体

```rust
use loom::agent::tot::{build_tot_runner, TotRunner};
use loom::agent::react::{ReactBuildConfig, TotRunnerConfig};
use loom::llm::{LlmClient, ChatOpenAI};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 配置 LLM 客户端
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    // 配置 ToT 运行器
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        tot_config: TotRunnerConfig {
            max_depth: 5,                // 最多 5 层深度
            candidates_per_step: 3,      // 每步生成 3 个候选
            research_quality_addon: false,
        },
        ..Default::default()
    };

    // 构建 ToT 运行器
    let runner = build_tot_runner(&config, Some(Box::new(llm)), true).await?;

    // 执行复杂推理任务
    let result = runner.invoke("分析人工智能在医疗领域的应用前景和挑战").await?;
    
    println!("最终回复: {:?}", result.last_assistant_reply());
    println!("探索深度: {}", result.tot.depth);
    
    Ok(())
}
```

### 自定义候选数量和深度

```rust
use loom::agent::tot::TotRunner;
use loom::agent::react::{ReactBuildConfig, TotRunnerConfig};
use loom::llm::{LlmClient, ChatOpenAI};
use loom::tool_source::MemoryToolSource;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    // 创建工具源
    let tool_source = Box::new(MemoryToolSource::new());

    // 自定义配置：较少候选但更深探索
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        tot_config: TotRunnerConfig {
            max_depth: 8,                // 增加最大深度
            candidates_per_step: 2,      // 减少候选数量
            research_quality_addon: false,
        },
        ..Default::default()
    };

    let runner = TotRunner::new(
        llm,
        tool_source,
        None,                           // 无检查点
        None,                           // 无存储
        None,                           // 无运行配置
        None,                           // 默认系统提示
        None,                           // 无审批策略
        None,                           // 无取消令牌
        false,                          // 不详细输出
        config.tot_config.max_depth,
        config.tot_config.candidates_per_step,
        config.tot_config.research_quality_addon,
    )?;

    let result = runner.invoke("设计一个可持续发展的城市规划方案").await?;
    println!("探索路径数: {}", result.tot.explored_paths.len());
    
    Ok(())
}
```

### 研究质量增强模式

```rust
use loom::agent::react::{ReactBuildConfig, TotRunnerConfig};
use loom::agent::tot::build_tot_runner;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 启用研究质量增强，适合学术研究任务
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        tot_config: TotRunnerConfig {
            max_depth: 6,
            candidates_per_step: 3,
            research_quality_addon: true,  // 启用研究增强
        },
        ..Default::default()
    };

    let runner = build_tot_runner(&config, None, true).await?;
    
    // 学术研究任务
    let result = runner.invoke(
        "系统性地综述机器学习在自然语言处理中的最新进展"
    ).await?;
    
    println!("研究深度: {}", result.tot.depth);
    println!("候选评估数: {}", result.tot.candidates.len());
    
    Ok(())
}
```

### 流式输出与候选监控

```rust
use loom::agent::tot::{build_tot_runner, TotRunner};
use loom::agent::react::{ReactBuildConfig, TotRunnerConfig};
use loom::stream::StreamEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        tot_config: TotRunnerConfig::default(),
        ..Default::default()
    };

    let runner = build_tot_runner(&config, None, true).await?;

    let result = runner.stream_with_callback(
        "比较不同编程范式的优缺点",
        Some(|event| {
            match event {
                StreamEvent::TotExpand { candidates } => {
                    println!("🌱 生成的候选方案:");
                    for (i, candidate) in candidates.iter().enumerate() {
                        println!("  候选 {}: {}", i + 1, candidate);
                    }
                }
                StreamEvent::TotEvaluate { chosen, scores } => {
                    println!("📊 候选评估结果:");
                    for (i, score) in scores.iter().enumerate() {
                        let marker = if i == chosen { "✓" } else { " " };
                        println!("  {} 候选 {}: 分数 {:.2}", marker, i + 1, score);
                    }
                }
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;

    println!("🎯 最终选择: 候选 {}", result.final_state.tot.chosen_index.unwrap() + 1);
    
    Ok(())
}
```

### 直接使用 TotRunner

```rust
use loom::agent::tot::{TotRunner, TotState};
use loom::agent::react::TotRunnerConfig;
use loom::llm::{LlmClient, ChatOpenAI};
use loom::tool_source::MemoryToolSource;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    let tool_source = Box::new(MemoryToolSource::new());

    let tot_config = TotRunnerConfig {
        max_depth: 4,
        candidates_per_step: 2,
        research_quality_addon: false,
    };

    let runner = TotRunner::new(
        llm,
        tool_source,
        None, None, None, None, None, None, false,
        tot_config.max_depth,
        tot_config.candidates_per_step,
        tot_config.research_quality_addon,
    )?;

    let result = runner.invoke("分析区块链技术的商业应用前景").await?;

    // 分析探索过程
    println!("总探索深度: {}", result.tot.depth);
    println!("已尝试候选: {:?}", result.tot.tried_indices);
    
    if let Some(reason) = result.tot.path_failed_reason {
        println!("路径失败原因: {}", reason);
    }
    
    Ok(())
}
```

## ToT 流程图

```
用户消息
    ↓
START 节点
    ↓
┌─────────────────┐
│ ThinkExpandNode │ ← 生成 2-3 个候选思维方案
└────────┬────────┘
         │
         ↓
┌──────────────────┐
│ ThinkEvaluateNode│ ← 评估候选方案并选择最佳
└────────┬─────────┘
         │
         ├─→ tot_tools_condition 判断
         │   ├─ 有工具调用 → ActNode
         │   └─ 无工具调用 → END
         │
         ↓
┌─────────────────┐
│   ActNode       │ ← 执行选中的工具调用
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│  ObserveNode    │ ← 整合结果，决定回溯或继续
└────────┬────────┘
         │
         ├─→ tot_observe_condition 判断
         │   ├─ 建议回溯 → BacktrackNode
         │   └─ 继续探索 → ThinkExpandNode
         │
         ↓
┌─────────────────┐
│ BacktrackNode   │ ← 尝试下一个候选方案
└────────┬────────┘
         │
         └─→ 回到 ActNode
```

## 候选评估机制

**ThinkEvaluateNode** 使用基于规则的评分系统：

```rust
// 评分因素：
fn score_candidate(candidate: &TotCandidate, user_query: &str) -> f32 {
    let mut score = 0.0;
    
    // 1. 思维长度评分 (10-2000 字符为佳)
    score += if (10..=2000).contains(&candidate.thought.len()) { 0.5 } else { 0.2 };
    
    // 2. 工具调用评分 (有工具调用更优)
    score += if !candidate.tool_calls.is_empty() { 0.5 } else { 0.3 };
    
    // 3. 搜索意图惩罚 (查询包含搜索词但无工具调用)
    if has_search_intent(user_query) && candidate.tool_calls.is_empty() {
        score -= 0.25;
    }
    
    // 4. 主题重叠奖励 (与用户查询的相关性)
    score += topic_overlap_bonus(user_query, &candidate.thought);
    
    score
}
```

## 最佳实践

### 配置选择
- **简单推理**: `max_depth: 3`, `candidates_per_step: 2`
- **复杂分析**: `max_depth: 5-8`, `candidates_per_step: 3`
- **学术研究**: 启用 `research_quality_addon: true`
- **快速原型**: 减少候选数量以降低成本

### 性能优化
- 合理设置 `max_depth` 避免无限探索
- 监控 `tot.explored_paths` 了解探索范围
- 使用流式输出实时查看候选生成过程
- 对简单任务禁用 ToT，使用 ReAct 更高效

### 场景选择
**适合 ToT 的场景**:
- 多方案比较和选择
- 复杂问题分解和分析
- 需要回溯和修正的推理
- 学术研究和系统分析

**适合 ReAct 的场景**:
- 实时对话和交互
- 简单工具调用链
- 单步推理任务
- 成本敏感的应用

---

## 相关概念

- **ReAct 运行模式**: 线性循环推理模式
- **GoT (Graph of Thoughts)**: 图状思维推理模式
- **DUP (Decomposition-Usage-Policy)**: 任务分解策略
- **LLM 客户端**: 底层模型调用接口

---

**下一页**: [ReAct 运行模式](./react.md) | [GoT 运行模式](./got.md) | [LLM 客户端](./llm-client.md)