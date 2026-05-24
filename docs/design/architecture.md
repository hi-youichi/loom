# 架构设计

## 设计目标

Levol 是一个**编排层**，不替代底层 AI Coding CLI，而是给它加上自进化能力。核心设计目标：

1. **底层 CLI 零修改** — 所有进化逻辑在 Wrapper 层实现
2. **多 CLI 支持** — 通过 Backend Adapter 插拔 Loom、Codex 等
3. **纯文件系统** — 不依赖数据库服务，数据人可读可编辑
4. **单二进制** — Rust 编译，无运行时依赖

## 三层架构

```
┌─────────────────────────────────────┐
│  编排层 (Orchestration)              │
│                                     │
│  Assembler  — 会话前：组装上下文      │
│  Reviewer   — 会话后：审查并沉淀      │
│  Curator    — 定期：维护技能健康      │
│  Evolver    — 按需：优化技能质量      │
├─────────────────────────────────────┤
│  适配层 (Backend Adapter)            │
│                                     │
│  统一的 Backend trait               │
│  ┌─────────┐  ┌──────────────────┐  │
│  │  Loom   │  │  Codex (OpenAI)  │  │
│  └─────────┘  └──────────────────┘  │
├─────────────────────────────────────┤
│  数据层 (Data)                       │
│                                     │
│  纯文件系统 + SQLite FTS5            │
│  memory/  skills/  sessions/         │
└─────────────────────────────────────┘
```

### 编排层

编排层包含 4 个核心组件：

- **Assembler**：会话启动前，读取记忆和技能，注入到 context 文件
- **Reviewer**：会话结束后，AI 审查对话，决定是否更新记忆/创建技能
- **Curator**：定期维护技能生命周期（stale → archive → cleanup）
- **Evolver**：用 DSPy+GEPA 自动优化技能质量（可选）

### 适配层

Backend trait 抽象了底层 CLI 的差异：

```rust
trait Backend {
    fn context_file(&self) -> &str;          // "CLAUDE.md" 或 "AGENTS.md"
    fn chat_command(&self) -> Command;       // 启动会话
    fn review_command(&self, prompt: &str) -> Command;  // 后台审查
    fn parser(&self) -> Box<dyn OutputParser>;
}
```

每个底层 CLI 实现自己的 Backend + OutputParser。上层代码通过 `&dyn Backend` 调用，不感知具体 CLI。

### 数据层

所有数据存放在 `~/.loom/data/`，纯 Markdown/YAML/JSONL 文件：

- **记忆**：`USER.md`、`PROJECT.md` — 人可读，可直接编辑
- **技能**：`skills/auto/<name>/SKILL.md` — YAML frontmatter + Markdown 正文
- **会话**：`sessions/*.jsonl` — JSONL 格式，SQLite FTS5 索引
- **进化记录**：`evolution/runs/<skill>/<date>/` — baseline、evolved、metrics

## 数据流

```
                  ┌─── memory/USER.md ───┐
                  │                       │
levol chat ────→ Assembler ──→ 注入 context ──→ 启动底层 CLI
                                                       │
                                                   录制对话
                                                       │
                                                       ▼
                  ┌─── memory/ ──────────────────────┐  │
                  │    skills/  ←── Reviewer ←── 会话结束 │
                  │    sessions/                        │
                  └────────────────────────────────────┘
```

## 与 Hermes 的对比

| | Hermes | Levol |
|---|--------|-------|
| 语言 | Python | Rust + 可选 Python |
| 记忆 | MemoryManager + 7 Provider | 文件系统 (USER.md) |
| 技能创建 | skill_manage 工具 | LLM Review → 文件写入 |
| 平台 | Telegram/Discord Gateway | CLI 透传 |
| 依赖 | Python 全栈 | 单二进制 |
| 多 CLI | 无 | Backend Adapter |

**核心差异**：Levol 复用底层 CLI 的 Agent 能力，只做编排层，代码量 ~4000 行。
