# RFC: `loom review session` 命令

> 手动对指定会话执行后台审查（Review Agent），支持事后补审和批量处理。

## 一、背景

当前 Review Agent 设计为每轮对话结束后自动触发（daemon 线程）。但以下场景无法覆盖：

- **历史会话未审查**：Review 功能上线前积累的会话，或自动审查因 LLM 失败被跳过
- **手动重审**：用户发现记忆/技能缺失，想对某次重要会话重新审查
- **批量导入**：新项目接入时，需要从已有会话中批量提取技能和记忆

目前没有 CLI 命令支持手动触发指定 session 的审查。

## 二、命令设计

### 2.1 基本用法

```bash
# 对指定 session 执行审查
loom review session <session_id>

# 对指定 session 执行审查（详细输出）
loom review session <session_id> --verbose

# 试运行，只输出 Review Prompt 不调用 LLM
loom review session <session_id> --dry-run

# 只审查记忆（不处理技能）
loom review session <session_id> --memory-only

# 只审查技能（不更新记忆）
loom review session <session_id> --skills-only

# 指定审查模型（覆盖配置）
loom review session <session_id> --model openai/gpt-4.1
```

### 2.2 批量用法

```bash
# 审查最近 7 天所有未审查的会话
loom review sessions --recent 7d

# 审查所有未审查的会话
loom review sessions --all-unreviewed

# 按关键词搜索并审查相关会话
loom review sessions --query "rust debug"

# 试运行，列出将被审查的会话
loom review sessions --recent 7d --dry-run
```

### 2.3 查询用法

```bash
# 列出所有已审查的会话
loom review history

# 查看某个会话的审查结果
loom review show <session_id>

# 列出未审查的会话
loom review pending
```

### 2.4 完整子命令树

```
loom review
├── session <id>          # 审查单个会话
├── sessions              # 批量审查
├── history               # 查看审查历史
├── show <id>             # 查看某次审查结果
└── pending               # 列出待审查会话
```

## 三、数据流

### 3.1 单会话审查流程

```
loom review session <session_id>
    │
    ▼
SessionManager::cat_session(id) → Vec<ReActState>
    │
    ▼
提取 messages，截断至 max_session_chars（默认 24000）
    │
    ▼
构建 Review Prompt（COMBINED_REVIEW_PROMPT）
    │
    ▼
调用 LLM（使用配置模型或 --model 覆盖）
    │
    ├── memory_updates → 写入 USER.md / PROJECT.md / FACTS.md
    └── skill_updates  → 按优先级链处理技能
    │
    ▼
保存审查记录到 ~/.loom/data/review/history.jsonl
    │
    ▼
输出审查摘要
```

### 3.2 审查记录格式

每次审查完成后，追加一条记录到 `~/.loom/data/review/history.jsonl`：

```json
{
  "session_id": "01912abc-...",
  "reviewed_at": "2025-08-19T10:30:00Z",
  "trigger": "manual",
  "model": "openai/gpt-4.1",
  "flags": ["--skills-only"],
  "memory_updates": [
    {"file": "USER.md", "action": "append", "chars_added": 120}
  ],
  "skill_updates": [
    {"name": "debug-rust-errors", "action": "patch", "chars_changed": 45}
  ],
  "skipped": false,
  "skip_reason": null,
  "cost_usd": 0.03,
  "duration_ms": 4200
}
```

字段说明：
- `trigger`: `manual`（CLI 手动触发）、`auto`（对话结束自动触发）、`batch`（批量审查）
- `skipped`: 是否跳过（如会话内容太短、无有意义内容）
- `skip_reason`: 跳过原因

## 四、判定逻辑

### 4.1 已审查判定

通过 `review/history.jsonl` 判断某个 session 是否已审查。规则：

- 精确匹配 `session_id` + `trigger != "dry_run"`
- 同一 session 允许多次审查（如用户手动重审），但 `--all-unreviewed` 跳过已有成功审查记录的 session

### 4.2 跳过条件

以下情况跳过审查并记录：

| 条件 | 处理 |
|------|------|
| 会话不存在 | 报错退出 |
| 会话消息数 < 4（1 轮对话） | 跳过，记录 `skip_reason: "too_short"` |
| 会话消息总长 < 200 字符 | 跳过，记录 `skip_reason: "insufficient_content"` |
| `--memory-only` 且无可提取记忆 | 跳过 skill 步骤，仅输出 memory 结果 |
| LLM 调用失败 | 重试 3 次，失败后记录 `skip_reason: "llm_error"` |

## 五、与现有模块的关系

```
依赖：
├── SessionManager (M1)          — cat_session() 提供会话数据
├── MemoryStore (M3)             — 写入记忆文件
├── SkillRegistry (M4)           — 技能 CRUD
└── Review Prompt 模板 (M2)      — 构建 prompt + 解析输出

新增：
├── cli/src/run/review_cmd.rs    — 命令入口 + 参数解析
└── ~/.loom/data/review/         — 审查记录目录
    └── history.jsonl            — 审查历史
```

### 5.1 复用 Review Agent 逻辑

`cli/src/run/review.rs`（M2 模块）已有的核心函数：

```rust
pub async fn run_review(
    messages: &[Message],
    config: &ReviewConfig,
    memory_store: &MemoryStore,
    skill_registry: &SkillRegistry,
) -> Result<ReviewResult>
```

`review_cmd.rs` 的职责是：
1. 解析 CLI 参数
2. 从 SessionManager 加载会话数据
3. 调用 `run_review()`
4. 保存审查记录
5. 格式化输出

## 六、配置扩展

在 `review:` 段新增以下配置项：

```yaml
review:
  enabled: true
  max_session_chars: 24000
  auto_create_threshold: 5
  # --- 新增 ---
  model: null                       # 手动审查使用的模型（null 则使用全局默认）
  skip_min_messages: 4              # 消息数少于此值跳过
  skip_min_chars: 200               # 总字符数少于此值跳过
  history_path: ~/.loom/data/review/history.jsonl  # 审查记录路径
```

## 七、输出示例

### 7.1 正常审查

```
$ loom review session 01912abc-xxxx --verbose

Reviewing session: 01912abc-xxxx
  Messages: 24 | Duration: 8 min | Model: openai/gpt-4.1

Memory updates:
  + USER.md: appended 1 entry (120 chars)
  + PROJECT.md: appended 1 entry (85 chars)

Skill updates:
  ~ debug-rust-errors: patched (added error pattern for async traits)

Cost: $0.03 | Duration: 4.2s
```

### 7.2 跳过

```
$ loom review session 01912abc-xxxx

Skipped: session too short (2 messages, minimum is 4)
```

### 7.3 Dry run

```
$ loom review session 01912abc-xxxx --dry-run

[DRY RUN] Would review session: 01912abc-xxxx
  Messages: 24 | Truncated to: 12000 chars

Review prompt written to: ~/.loom/data/review/dry-run-01912abc-xxxx.md
```

### 7.4 批量

```
$ loom review sessions --recent 7d

Reviewing 12 sessions from the last 7 days...

  [1/12] 01912abc-xxxx — updated 1 memory, 0 skills
  [2/12] 01912def-xxxx — skipped (too short)
  [3/12] 01912ghi-xxxx — updated 0 memory, 1 skill (patched: deploy-vercel)
  ...
  [12/12] 01912xyz-xxxx — updated 2 memory, 1 skill (created: sql-migration)

Summary: 10 reviewed, 2 skipped | 5 memory updates, 3 skill updates
Total cost: $0.28 | Duration: 52s
```

## 八、实现计划

### 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `cli/src/run/review_cmd.rs` | 新增 | 命令入口、参数解析、输出格式化 |
| `cli/src/run/review.rs` | 修改 | 提取 `run_review()` 为可复用公共接口 |
| `cli/src/subcommands.rs` | 修改 | 注册 `review` 子命令 |
| `cli/src/args.rs` | 修改 | 新增 `Review` 子命令参数定义 |
| `docs/evolution/commands.md` | 修改 | 补充命令文档 |

### 工作量

| 任务 | 预估 |
|------|------|
| 命令参数定义 + 子命令注册 | 0.5 天 |
| review_cmd.rs 核心逻辑 | 1 天 |
| 审查记录持久化 (history.jsonl) | 0.5 天 |
| 批量审查逻辑 | 0.5 天 |
| 输出格式化 + verbose/dry-run | 0.5 天 |
| 测试 | 1 天 |
| **合计** | **3-4 天** |

### 前置条件

- M1（SessionManager 扩展）已完成
- M2（Review Agent 核心逻辑）已完成
- M3（MemoryStore）已完成
- M4（SkillRegistry）已完成

若 M2-M4 未完成，可先实现命令框架 + dry-run 模式，后续对接。

## 九、开放问题

1. **审查去重**：同一 session 多次审查产生的记忆/技能是否去重？建议是——Review Prompt 应接收当前记忆文件内容，避免重复追加。
2. **并发安全**：批量审查时是否并行？建议默认串行（`max_concurrent: 1`），避免文件写入冲突。
3. **审查撤销**：是否支持 `loom review undo <session_id>`？建议暂不支持，用户可手动编辑 memory/skill 文件。
