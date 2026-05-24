# 会话生命周期

一次 `levol chat` 的完整流程。

## 总览

```
levol chat
  │
  ├─ Phase 1: 组装上下文 (Pre-Session Assembly)
  │
  ├─ Phase 2: 启动底层 CLI + 录制对话 (Capture)
  │
  ├─ Phase 3: 保存会话 + 还原 context (Save & Restore)
  │
  └─ Phase 4: 后台审查 (Post-Session Review)
```

## Phase 1: 组装上下文

会话启动前，Assembler 读取所有数据并注入到项目的 context 文件。

### 步骤

1. 读取 `levol.yaml` 获取配置
2. 读取 `memory/USER.md` — 用户偏好
3. 读取 `memory/PROJECT.md` — 项目上下文
4. 扫描 `skills/auto/` 中 `lifecycle: active` 的技能，提取摘要
5. 将以上内容追加到项目的 context 文件（`CLAUDE.md` 或 `AGENTS.md`）末尾
6. 备份原始 context 文件内容

### 注入格式

```markdown
<!-- levol-context-start -->
## User Memory
- 用户偏好 GitHub Actions CI
- 使用 rust-toolchain.toml 管理 Rust 版本

## Active Skills
### rust-ci-setup
系统性排查 Rust 编译错误...
<!-- levol-context-end -->
```

### 为什么用追加-还原？

- **零配置**：底层 CLI 天然读取自己的 context 文件，不需要额外 flag
- **可靠**：追加在文件末尾，不破坏已有内容
- **还原**：会话结束后用备份还原，不影响用户的原始配置

## Phase 2: 启动底层 CLI + 录制对话

### 启动方式

通过 Backend trait 构建 Command，透传 stdin/stdout：

```rust
fn run_chat(backend: &dyn Backend, context: &str) -> Result<Session> {
    let backup = inject_context(backend.context_file(), context)?;
    let mut child = backend.chat_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let session = capture_session(&mut child, backend.parser())?;
    restore_context(backend.context_file(), &backup)?;
    Ok(session)
}
```

### 捕获方式

**方案 B（当前）**：Stdout pipe → 解析器 → JSONL

```
底层 CLI stdout ──→ OutputParser ──→ SessionMessage ──→ JSONL 文件
```

- 简单，够用
- 每个 Backend 实现自己的 `OutputParser`

**方案 A（后续升级）**：PTY 录制

```
底层 CLI PTY ──→ 全文录制 ──→ 正则/markdown 解析 ──→ SessionMessage
```

- 完整捕获，兼容所有输出格式
- 用 `portable-pty` crate

### 解析器接口

```rust
trait OutputParser {
    fn parse_user_input(line: &str) -> Option<String>;
    fn parse_assistant_response(lines: &[&str]) -> Option<String>;
    fn parse_tool_call(line: &str) -> Option<(String, Value)>;
    fn parse_tool_result(lines: &[&str]) -> Option<String>;
}
```

### 各 Backend 输出差异

| 维度 | Loom | Codex |
|------|------|-------|
| 非交互模式 | `echo X \| loom chat` | `codex --quiet` |
| Assistant 响应 | markdown 块 | markdown 块（带 `[codex]` 前缀） |
| Tool 调用 | `tool_name(args)` 格式 | `Running: command` 格式 |
| Tool 结果 | stdout 捕获 | stdout 捕获 |
| 退出码 | 待确认 | 0=成功, 1=error |

## Phase 3: 保存会话 + 还原 context

会话结束后：

1. 将录制的 `SessionMessage` 序列化写入 `sessions/<timestamp>.jsonl`
2. 更新 SQLite FTS5 搜索索引
3. 用备份还原 context 文件（删除追加的内容）

## Phase 4: 后台审查

会话保存后，异步启动 Review Agent（如果 `review.enabled`）。

详细流程见 [evolution/review.md](../evolution/review.md)。
