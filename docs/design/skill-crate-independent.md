# Skill Crate 独立化方案

## 背景

将 Loom 的 skill 系统拆分为独立的 crate，使核心逻辑可独立测试和复用。

## 最终方案：四层独立 Crate

```
┌─────────────────────────────────────────────────────────────┐
│                         loom (运行时)                        │
│  - 依赖 loom-skill, loom-curator                             │
│  - 整合所有模块，提供完整功能                                 │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ 依赖
                              │
            ┌─────────────────┴─────────────────┐
            │                                   │
            ▼                                   ▼
┌─────────────────────────┐       ┌─────────────────────────┐
│     loom-curator         │       │       loom-llm          │
│  - 依赖 loom-skill       │       │  - 依赖 loom-skill      │
│  - 依赖 loom-llm         │       │  - LlmClient trait      │
│  - Curator 后台维护      │       │  - ChatOpenAI 实现      │
│  - LLM Review 逻辑       │       │  - Retry / Streaming    │
└─────────────────────────┘       └─────────────────────────┘
            │                                   ▲
            │                                   │
            │                                   │
            └─────────────────┬─────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      loom-skill (核心)                      │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │  discovery  │  │   storage   │  │    usage    │          │
│  │ SkillRegistry│  │SkillStorage │  │SkillUsage   │          │
│  │             │  │ Registry   │  │   Store     │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│                         │                                   │
│                    ┌─────────┐                              │
│                    │  utils  │                              │
│                    │ frontmatter                            │
│                    └─────────┘                              │
└─────────────────────────────────────────────────────────────┘
```

## 依赖链

```
loom ────► loom-curator ────► loom-skill
    │
    └──► loom-llm
              ▲
              └── loom-curator (使用 LlmClient trait)
```

## 各 Crate 职责

### 1. loom-skill (核心) ✅ 已完成

**职责**: 纯 skill 逻辑，无外部 loom 依赖

**模块**:
- `utils.rs` — frontmatter 解析、YAML 工具、平台匹配
- `discovery.rs` — SkillRegistry 扫描发现
- `storage.rs` — SkillStorageRegistry 持久化 CRUD
- `usage.rs` — SkillUsageStore 使用统计

**依赖**: `serde`, `serde_json`, `serde_yaml`, `thiserror`, `chrono`, `tracing`, `tokio`, `env_config`

### 2. loom-llm (待开发)

**职责**: LLM 客户端抽象和实现

**模块**:
- `client.rs` — LlmClient trait 定义
- `chat_openai.rs` — OpenAI API 实现
- `openai_compat.rs` — OpenAI 兼容 API 实现
- `retry.rs` — RetryLlmClient 重试包装
- `error.rs` — 错误分类和重试策略

**依赖**: `loom-skill`（用于 skill 相关功能）, `reqwest`, `tokio`, `serde`, `thiserror`

**关键设计**: `LlmClient` trait 定义在 loom-llm 中，由 loom 实现具体调用

### 3. loom-curator (待开发)

**职责**: 后台 skill 维护和自演化

**模块**:
- `curator.rs` — Curator 主逻辑
- `llm_review.rs` — LLM review 逻辑
- `prompts.rs` — Review prompt 模板（对齐 Hermes）
- `state.rs` — CuratorState 持久化

**依赖**: `loom-skill`, `loom-llm`

**关键设计**: 使用 `loom-llm::LlmClient` trait，不依赖 loom

### 4. loom (运行时)

**职责**: 整合所有模块，提供完整 agent 运行时

**更新内容**:
- 依赖 `loom-skill`, `loom-llm`, `loom-curator`
- 实现 `LlmClient` trait（调用具体 API）
- 提供 Tool trait 实现（SkillTool）
- 后台维护（调用 Curator）

## Hermes 对齐

| Hermes 文件 | Loom Crate | 说明 |
|-------------|------------|------|
| `skill_utils.py` | `loom-skill/utils.rs` | ✅ 已实现 |
| `skill_bundles.py` | `loom-skill` (待实现) | Skill 组合命令 |
| `skills_tool.py` | `loom/tools/skill.rs` | 工具实现（保留在 loom） |
| `skill_usage.py` | `loom-skill/usage.rs` | ✅ 已实现 |
| `curator.py` | `loom-curator` | ✅ 架构设计完成 |
| `background_review.py` | `loom-curator/prompts.rs` | ✅ 架构设计完成 |
| `message.py` | `loom-llm` | LLM 消息类型 |

## 开发进度

| Crate | 状态 | 文件 |
|-------|------|------|
| `loom-skill` | ✅ 完成 | `loom-skill/` |
| `loom-llm` | ⏳ 待开发 | - |
| `loom-curator` | ⏳ 待开发 | - |
| `loom` 整合 | ⏳ 待开发 | - |

## 下一步

1. **创建 loom-llm**
   - 定义 `LlmClient` trait
   - 迁移 `ChatOpenAI`, `OpenAICompat`, `Mock`, `Retry` 实现
   - 更新 workspace `Cargo.toml`

2. **创建 loom-curator**
   - 依赖 `loom-skill` 和 `loom-llm`
   - 迁移 Curator、LLM Review、Prompts 逻辑
   - 更新 workspace `Cargo.toml`

3. **更新 loom 整合**
   - 添加 `loom-skill`, `loom-llm`, `loom-curator` 依赖
   - 实现 `LlmClient` trait for `ChatOpenAI`
   - 删除旧文件（`skill.rs`, `skill_registry.rs`, `skill_usage.rs` 等）
   - 验证编译通过