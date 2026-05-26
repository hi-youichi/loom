# 后台审查 (Review Agent)

> Hermes `agent/background_review.py` 的 Rust 重写，核心实现在 `loom/src/background_review/`。

## 流程

```
每轮对话结束
    │
    ▼
spawn_background_review()  [loom/src/background_review/workflow.rs]
    │
    ▼
tokio::task::spawn 异步任务（不阻塞主对话）
    │
    ▼
截断会话内容（默认 24000 字符）+ 构建 Review Prompt
    │
    ▼
独立 LLM Client（ChatOpenAICompat, 默认 gpt-4o-mini）分析对话
白名单工具（ReviewToolExecutor 实现）：
  memory_get, memory_set,
  skills_list, skill_view, skill_create, skill_edit,
  skill_patch, skill_delete, skill_write_file, skill_remove_file
    │
    ├── memory 更新 → 写入 MemoryStore
    └── skill 操作 → 按优先级链执行（Prompt 中定义）
```

## 核心实现

### 架构

```
loom/src/background_review/       ← 核心库实现
  ├── workflow.rs                 — 异步调度, BackgroundReviewConfig, spawn
  ├── agent_loop.rs               — AgentReviewRunner, 对话循环
  ├── prompts.rs                  — 三套 Review Prompt (MEMORY/SKILL/COMBINED)
  ├── tools.rs                    — ReviewToolExecutor, 工具白名单
  ├── memory.rs                   — MemoryStore
  ├── skill_registry.rs           — SkillRegistry
  ├── security.rs                 — validate_skill_create/path (始终启用)
  ├── curator.rs                  — Curator 定期维护
  ├── evolution.rs                — EvolutionTrigger 触发进化
  ├── history.rs                  — 审查历史
  └── observability.rs            — 可观测性

cli/src/run/background_review.rs  ← 薄 wrapper (22行), 仅添加 CLI eprintln 输出
```

### 触发时机
- 每轮对话结束后，通过 `trigger_post_turn_review` 异步触发
- 创建独立 `ChatOpenAICompat` LLM client（独立 model/base_url/api_key）
- 运行在 `tokio::task::spawn` 异步任务，不阻塞主对话

### 工具白名单
Review agent 只能使用 `ReviewToolExecutor` 处理的工具，其他调用返回错误：

| 工具 | 用途 |
|------|------|
| `memory_get` | 读取记忆文件 |
| `memory_set` | 写入记忆文件 |
| `skills_list` | 列出已有技能 |
| `skill_view` | 查看技能内容 |
| `skill_create` | 创建新技能 |
| `skill_edit` | 编辑技能（全量） |
| `skill_patch` | 修补技能（增量） |
| `skill_delete` | 删除技能 |
| `skill_write_file` | 写入技能 support file |
| `skill_remove_file` | 删除技能 support file |

### Review Prompt 类型
1. `MEMORY_REVIEW_PROMPT`: 仅审查记忆——用户偏好、个人信息、行为期望
2. `SKILL_REVIEW_PROMPT`: 仅审查技能——更新/创建/修补技能文档
3. `COMBINED_REVIEW_PROMPT`: 同时处理记忆和技能（默认）

### 技能 Review 优先级链
(在 Prompt 中定义，LLM 自行按优先级执行)

```
1. PATCH 当前会话已加载的技能（通过 /skill-name 或 skill_view 加载的）
   ↓ 无匹配
2. PATCH 已有的 class-level umbrella 技能（通过 skills_list + skill_view 查找）
   ↓ 无匹配
3. 在已有 umbrella 下添加 support file
   ├── references/<topic>.md   — 会话细节、API 文档摘录、领域笔记
   ├── templates/<name>.<ext>  — 可复用的模板文件
   └── scripts/<name>.<ext>    — 可直接执行的脚本
   ↓ 无匹配
4. CREATE 新的 class-level umbrella 技能（SKILL.md）
   命名要求：类级别，禁止 PR 号/错误串/一次性任务名
```

### 触发信号（Review Prompt 中定义）
- 用户纠正了 agent 的风格/语气/格式/详细程度
- 用户表达了挫败感："stop doing X"、"太冗长了"、"不要这样格式化"
- 非平凡技巧、修复、变通方案、调试路径出现
- 已加载/查阅的技能被发现过时/缺失/错误

### 反模式保护（不保存的内容）
- 环境依赖的临时故障（缺二进制、路径不匹配、未安装包）
- 工具负面断言（"browser 工具不工作"、"X 工具坏了"）——会硬化为长期拒绝
- 已解决的瞬时错误（retry 成功 → 教训是 retry 模式，不是原始故障）
- 一次性任务叙事（"总结今天市场"、"分析这个 PR"）

### 安全扫描
- `security.rs` 中的 `validate_skill_create` 和 `validate_skill_path` **始终启用**
- 检测危险模式（`rm -rf`, `exec(`, `eval(`, 等）
- 检测注入模式（"ignore previous instructions", "jailbreak" 等）
- 与 Hermes 的 `guard_agent_created` 不同：没有开关，始终扫描

### 与 Hermes 的主要差异

| 维度 | Hermes (Python) | Loom (Rust) |
|------|-----------------|-------------|
| 调度 | `threading.Thread(daemon=True)` | `tokio::task::spawn` |
| LLM Client | Fork 父 agent 的 client | 独立 `ChatOpenAICompat` |
| 工具 | `memory`, `skill_manage` (抽象) | 10 个具体工具函数 |
| 安全 | `guard_agent_created` 可配置 | `security.rs` 始终启用扫描 |
| agent-created | `mark_agent_created()` 标记 | 未实现 |
| 错误重试 | 3次指数退避 | 直接 fail，无重试 |

## 配置

```yaml
review:
  enabled: true
  max_session_chars: 24000        # ⚠️ 默认 24000，非 12000
  max_iterations: 16
  model: gpt-4o-mini              # 独立 Review 使用的模型
  review_memory: true
  review_skills: true
```

`BackgroundReviewConfig` 定义在 `loom/src/background_review/workflow.rs:24-40`。

## 相关文档

- [记忆系统](memory.md) — 记忆文件格式
- [技能系统](skills.md) — 技能结构与 Curator
- [配置参考](config.md) — review 配置项
