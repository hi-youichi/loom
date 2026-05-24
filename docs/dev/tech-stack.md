# 技术选型

## 选型

| 组件 | 选择 | 理由 |
|------|------|------|
| 语言 | Rust | 单二进制，和 Loom 同生态 |
| CLI 框架 | clap | Rust 生态标准 |
| CLI 抽象 | Backend trait | 一套代码支持多个底层 CLI |
| 对话捕获 | Stdout pipe | 简单起步，可升级 PTY |
| 存储 | SQLite + FTS5 | 嵌入式，零依赖，全文搜索 |
| 配置 | serde_yaml | 人可读可编辑 |
| 分发 | 单二进制 | 无运行时依赖 |

> 进化引擎（`loom-evolution` crate）和数据结构（SkillMeta / ReviewResult / EvolutionResult）见 [evolution/data-structures.md](../evolution/data-structures.md)。

## 项目结构

```
levol/
├── Cargo.toml
├── src/
│   ├── main.rs                 # CLI 入口 (clap)
│   ├── cmd/                    # CLI 命令实现
│   │   ├── chat.rs             # 核心会话编排
│   │   ├── config.rs           # 配置命令
│   │   ├── skills.rs           # → evolution
│   │   ├── memory.rs           # → evolution
│   │   ├── curator.rs          # → evolution
│   │   ├── evolve.rs           # → evolution
│   │   ├── review.rs           # → evolution
│   │   └── sessions.rs         # 会话历史
│   ├── backend/                # Backend Adapter 层
│   │   ├── mod.rs              # Backend trait 定义
│   │   ├── loom.rs             # Loom adapter
│   │   └── codex.rs            # Codex adapter
│   ├── core/                   # 核心业务逻辑
│   │   ├── assembler.rs        # Pre-session context 组装
│   │   ├── capture.rs          # 对话流捕获 + 解析
│   │   ├── reviewer.rs         # → evolution
│   │   ├── curator_engine.rs   # → evolution
│   │   └── evolver.rs          # → evolution
│   ├── store/                  # 数据读写
│   │   ├── memory.rs           # → evolution
│   │   ├── skills.rs           # → evolution
│   │   ├── sessions.rs         # 会话存储 + SQLite
│   │   └── config.rs           # 配置管理
│   └── parser/                 # 输出解析器
│       ├── mod.rs              # OutputParser trait
│       ├── loom_output.rs      # Loom CLI 输出解析
│       ├── codex_output.rs     # Codex CLI 输出解析
│       └── review_output.rs    # Review 输出解析
├── reviewer/                   # Review Agent profile
├── evolution/                  # → loom-evolution crate
└── tests/
```

> 标记 `→ evolution` 的模块，其设计详见 [evolution/](../evolution/) 目录。

## 依赖

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
rusqlite = { version = "0.31", features = ["bundled-sqlcipher"] }
anyhow = "1"
thiserror = "1"
```
