# Backend Trait 开发指南

## 概述

Backend trait 是 Loom 适配不同底层 CLI 的核心抽象。想支持新 CLI，只需实现这个 trait。

## Backend Trait

```rust
#[async_trait]
trait Backend {
    fn name(&self) -> &str;
    fn context_file(&self) -> &str;
    fn chat_command(&self) -> Command;
    fn review_command(&self, prompt: &str) -> Command;
    fn parser(&self) -> Box<dyn OutputParser>;
}
```

### 方法说明

| 方法 | 说明 | Loom 示例 | Codex 示例 |
|------|------|----------|-----------|
| `name()` | Backend 名称 | `"loom"` | `"codex"` |
| `context_file()` | 注入上下文的目标文件 | `"CLAUDE.md"` | `"AGENTS.md"` |
| `chat_command()` | 构建启动会话的 Command | `loom chat` | `codex --quiet` |
| `review_command(prompt)` | 构建审查用的 Command | `loom agent run --profile reviewer --input <prompt>` | `codex --quiet --approval-mode full-auto <prompt>` |
| `parser()` | 返回对应的输出解析器 | `LoomParser` | `CodexParser` |

## OutputParser Trait

```rust
trait OutputParser {
    fn parse_user_input(line: &str) -> Option<String>;
    fn parse_assistant_response(lines: &[&str]) -> Option<String>;
    fn parse_tool_call(line: &str) -> Option<(String, Value)>;
    fn parse_tool_result(lines: &[&str]) -> Option<String>;
}
```

## 现有实现

### Loom Backend

```rust
struct LoomBackend { command: String }

impl Backend for LoomBackend {
    fn name(&self) -> &str { "loom" }
    fn context_file(&self) -> &str { "CLAUDE.md" }
    fn chat_command(&self) -> Command {
        Command::new(&self.command).arg("chat")
    }
    fn review_command(&self, prompt: &str) -> Command {
        Command::new(&self.command)
            .args(&["agent", "run", "--profile", "reviewer"])
            .arg("--input").arg(prompt)
    }
    fn parser(&self) -> Box<dyn OutputParser> {
        Box::new(LoomParser)
    }
}
```

### Codex Backend

```rust
struct CodexBackend { command: String }

impl Backend for CodexBackend {
    fn name(&self) -> &str { "codex" }
    fn context_file(&self) -> &str { "AGENTS.md" }
    fn chat_command(&self) -> Command {
        Command::new(&self.command).arg("--quiet")
    }
    fn review_command(&self, prompt: &str) -> Command {
        Command::new(&self.command)
            .arg("--quiet")
            .arg("--approval-mode").arg("full-auto")
            .arg(prompt)
    }
    fn parser(&self) -> Box<dyn OutputParser> {
        Box::new(CodexParser)
    }
}
```

## 添加新 Backend

以添加 Aider 支持为例：

1. 创建 `src/backend/aider.rs`
2. 实现 `Backend` trait
3. 实现 `AiderParser`（`OutputParser` trait）
4. 在 `levol.yaml` 中添加 `cli.aider` 配置
5. 在 `src/backend/mod.rs` 注册新 backend

```rust
// src/backend/aider.rs
struct AiderBackend { command: String }

impl Backend for AiderBackend {
    fn name(&self) -> &str { "aider" }
    fn context_file(&self) -> &str { ".aider.conf.yml" }  // 或自定义
    fn chat_command(&self) -> Command {
        Command::new(&self.command).arg("--no-auto-commits")
    }
    fn review_command(&self, prompt: &str) -> Command {
        Command::new(&self.command)
            .arg("--message").arg(prompt)
            .arg("--no-auto-commits")
    }
    fn parser(&self) -> Box<dyn OutputParser> {
        Box::new(AiderParser)
    }
}
```

## Backend 接口对比

| 能力 | Loom | Codex | 降级方案 |
|------|------|-------|----------|
| 交互式会话 | ✅ `loom chat` | ✅ `codex` | 直接用 |
| 非交互执行 | ❓ 待确认 | ✅ `codex --quiet` | Loom: echo pipe |
| 单次 Agent | `loom agent run` | ❌ | Codex: echo pipe |
| 结构化输出 | ❓ | ❌ | 解析 stdout |
| `--context-file` | ❓ | ❌ | 注入 context 文件 |
| 退出码 | ❓ | ✅ 0/1 | 检查进程状态 |

## 降级方案

如果底层 CLI 不支持某个能力：

1. **PTY 录制**：用 `portable-pty` 录制完整终端输出
2. **Hook 文件**：利用 CLI 的 hook 机制写入中间文件
3. **Shared FS**：预先在 context 文件中写好指令
