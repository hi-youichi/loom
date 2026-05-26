# CLI UX 改进方案 — 用户交互体验 Review

## 总体评价

现有方案对痛点的识别准确，技术路径可行，但在**四个维度**上与业界前沿存在明显差距：

1. **文件变更的交互式审批** — 完全缺失
2. **Markdown 渲染** — 未考虑
3. **成本/Token 可视化** — 仅覆盖 LLM usage 统计，缺乏会话级累计
4. **上下文窗口感知** — 未涉及

以下是逐阶段的详细 Review，每个阶段给出"肯定"、"问题"、"行业参考"和"增强建议"。

---

## 第一阶段 Review：进度指示器

### ✅ 肯定

- 正确识别了"假死感"是 #1 UX 痛点
- Spinner 设计简洁，`\r` 原地更新 + TTY 检测 + 回退方案合理
- 不引入新依赖的选择务实

### ⚠️ 问题

1. **Spinner 与后续输出冲突**：方案说"流式回复开始时清除 spinner"，但如果工具调用出错产生多行 stderr，spinner 行会被推上去而不是被替换。需要 `MultiProgress` 或至少确保 spinner 始终在最后一行。

2. **缺少工具执行耗时**：用户看到 `⠋ 执行工具: bash` 但不知道这个工具已经跑了 5 秒还是 30 秒。

3. **缺少并发工具调用的视觉**：多工具并行调用时（如 GOT agent），用户无法区分哪些工具在并行。

### 🏗️ 行业参考

- **Claude Code**：使用自定义 Ink（React-like TUI 框架），spinner 不仅显示状态还显示已用时间，如 `⠋ Thinking... (3s)`。
- **Gemini CLI** 的社区提案（Issue #21484）：提出工具调用层级可视化和决策流程图，区分"正在执行"和"等待审批"。
- **Aider**：工具执行时显示 `─` 分隔线 + 工具名称 + 耗时，简洁但信息完整。

### 💡 增强建议

```
⠋ 思考中... (3s)
⠋ 执行工具: bash "pytest tests/" ... (5s)
⠋ 思考中... (第 2 轮, 共 12s)
```

- Spinner 附加已用时间计数器
- 工具调用完成后显示耗时摘要：`✓ bash "pytest tests/" (2.3s)`
- 并行工具使用 `+N more...` 折叠

---

## 第二阶段 Review：结构化 stderr 面板格式

### ✅ 肯定

- `_CATEGORY  message` 前缀系统让信息有层次感
- `NO_COLOR` 支持是正确的兼容性考虑
- 统一格式化函数的提取方向正确

### ⚠️ 问题

1. **前缀过于技术化**：`_AGENT`、`_TOOLS`、`_CALL` 这些前缀对用户没有语义。普通用户不需要看到 `_CATEGORY` 前缀——这是内部调试格式，不是用户体验。

2. **缺少文件变更预览**：当 Agent 执行 `edit` 工具时，当前方案只显示 `_CALL edit: src/main.rs`。但用户最关心的不是"编辑了哪个文件"，而是"改了什么"。

### 🏗️ 行业参考

- **Claude Code**：工具调用使用可折叠（collapsible）区块。`Ctrl+O` 展开查看详细 tool trace。默认折叠为单行摘要。
- **Aider**：工具输出用颜色区分（`--tool-output-color`），文件变更直接显示 inline diff。
- **mdiff / agrev**：专门的终端 diff 查看 TUI，支持 side-by-side、unified view 切换和 syntax highlighting。

### 💡 增强建议

普通模式：
```
✏ src/main.rs +3 -1
  + use anyhow::Result;
  - fn main() {
  + fn main() -> Result<()> {
```

Verbose 模式才显示完整的 `_CALL/_DONE` 日志。

文件变更摘要（不侵入式）：
```
✏ src/main.rs  │  +3 -1  │  修改了错误处理
```

---

## 第三阶段 Review：思考与回复的视觉分离

### ✅ 肯定

- TTY 暗色包裹 thinking 内容的方案简洁有效
- 状态追踪 `EventState` 添加 thinking→reply 过渡的逻辑正确

### ⚠️ 问题

1. **分隔线可能打断阅读流**：`────────────────────` 在频繁的 thinking→reply 切换中会产生大量视觉噪音。

2. **Markdown 回复没有渲染**：LLM 回复中包含的代码块、列表、表格在终端中是原始文本。这是 CLI agent 的通用痛点，但影响很大。

### 🏗️ 行业参考

- **mdriver crate**：Rust 的 streaming markdown 终端渲染器，支持语法高亮、流式输出。
- **termimad crate**：成熟的终端 markdown 渲染库，支持表格、代码块。
- **Claude Code**：使用 Ink 的自定义 markdown 渲染组件，流式渲染代码块和列表。

### 💡 增强建议

1. **考虑引入 `termimad` 或 `mdriver`**：在流式回复中对 markdown 进行渲染（代码块语法高亮、列表缩进、表格对齐）。这不需要全功能 TUI 框架，只需要在 `print_stream_chunk` 中加一层 markdown 渲染。

2. Thinking 区域用**侧边标记**而非分隔线：
```
│ 灰色 thinking 内容...
│ 继续思考...
回复内容正常显示
```

---

## 第四阶段 Review：统一 LLM Usage 格式

### ✅ 肯定

- 统一三种不同格式是必要的
- `↓`/`↑` 箭头符号直观

### ⚠️ 问题

1. **仅显示单轮 usage**：只看到当前这轮 LLM 的 token 数，但不知道整个会话累计了多少。

2. **缺少成本估算**：用户不知道这个会话花了多少钱。

3. **缺少上下文窗口使用率**：会话接近 context limit 时没有预警。

### 🏗️ 行业参考

- **Claude Code**（Issue #39187 社区请求）：请求添加 context window usage indicator（百分比），让用户知道距离 context limit 还有多远。
- **agent-token-meter**：专门监控 AI agent token 消耗的工具，显示实时 burn rate、累计成本、会话成本曲线。
- **Aider**：显示 `Cost: $0.02` 在每轮回复后。

### 💡 增强建议

在每轮后追加会话累计：
```
_USAGE  2.35s | 1.2K↓ 800↑ = 2.0K @ 850 t/s  │  会话: 15K tokens, ~$0.08
```

Context 预警（当使用率 > 80%）：
```
⚠ Context: 163K/200K (82%) — 考虑使用 /compact 压缩上下文
```

---

## 第五阶段 Review：REPL 增强

### ✅ 肯定

- `rustyline` 选择正确（成熟、跨平台、支持历史/补全）
- 非 TTY 回退方案考虑到位

### ⚠️ 问题

1. **仅用 `rustyline` 太保守**：现代 CLI agent 普遍采用全 TUI 方案（BubbleTea/Ink 模式），而 `rustyline` 只是 readline 增强版，无法实现：
   - 工具调用结果的折叠展开
   - 工具审批的交互式 diff 查看
   - 聊天历史的滚动浏览
   - 实时状态面板

2. **缺少斜杠命令自动补全**：`/reset`、`/compact`、`/goal` 等命令没有 tab 补全。

3. **缺少多行输入的体验优化**：`\` 续行是传统 Unix 风格，现代做法是 `{}` 块输入或 `Shift+Enter`（需要 TUI 支持）。

### 🏗️ 行业参考

- **Claude Code**：使用 Ink（React-like terminal UI），支持：
  - `Ctrl+O` transcript viewer（查看完整 tool trace）
  - `Shift+Tab` 切换 permission mode
  - `Escape` 中断
  - Markdown 渲染 + 代码高亮
  - 实时 token counter

- **OpenCode-rs**：Rust 实现，使用 BubbleTea (charm.sh) TUI 框架：
  - 分栏布局：左侧聊天、右侧文件预览
  - 内置 diff viewer
  - 会话管理面板

- **Aider**：使用 `prompt_toolkit`（Python），支持：
  - 命令补全
  - 多种颜色主题（`--dark-mode`/`--light-mode`）
  - 聊天历史文件持久化

### 💡 增强建议

**短期（第五阶段原方案）**：
- `rustyline` + tab 补全 `/` 命令 + 上下方向键历史
- 这是务实选择，快速改善 REPL 体验

**长期（第七阶段建议新增）**：
- 引入 `ratatui`（原 `tui-rs`）或 `bubbletea` (charm.sh for Rust) 构建 TUI
- 实现 Claude Code 式的交互体验

---

## 第六阶段 Review：输出详细度分级

### ✅ 肯定

- 三级系统（quiet/normal/verbose）是合理的
- 向后兼容 `--verbose` 标志

### ⚠️ 问题

1. **缺少 `--debug` 级别**：三级可能不够。开发者排查问题时需要看到 ReAct state dump，但不想看到所有 verbose 工具详情。

2. **`--quiet` 模式下的行为需要明确**：`--quiet` 是否抑制 spinner？如果工具调用失败，quiet 模式下是否显示错误？

### 💡 增强建议

| 参数 | spinner | 工具摘要 | Usage | Diff 预览 | 错误 | State dump |
|------|---------|---------|-------|----------|------|------------|
| `-q` | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ |
| (默认) | ✓ | ✓ | 汇总 | ✓ | ✓ | ✗ |
| `-v` | ✓ | 详情 | 详情 | ✓ | ✓ | ✗ |
| `-vv` | ✓ | 详情 | 详情 | ✓ | ✓ | ✓ |

---

## 🆕 缺失的重要功能

以下功能在当前方案中完全缺失，但在业界已成为 CLI agent 的标配或强烈需求：

### F1：工具审批流（Permission Mode）

**行业参考**：Claude Code 的 permission mode（`Shift+Tab` 切换）：
- **Default**：文件编辑和 bash 命令需要确认
- **Auto-edit**：文件编辑自动批准，bash 需要确认
- **Full auto**：全部自动执行
- **Plan**：只做规划不执行

**建议**：添加 `--trust-level` 或 `--permission-mode` 参数：
```
--permission-mode ask     # 默认：每次工具调用都确认
--permission-mode auto    # 自动执行所有工具
--permission-mode plan    # 只规划不执行
```

工具审批时显示 diff：
```
⏸ bash: rm -rf node_modules
  [y] 执行  [n] 拒绝  [e] 编辑  [a] 全部自动
```

### F2：Markdown 渲染

**行业参考**：mdriver crate — streaming markdown 终端渲染

**建议**：在 `print_stream_chunk` 中引入 markdown 渲染层：
- 代码块：语法高亮 + 背景色
- 列表：正确缩进
- 表格：终端表格对齐
- 链接：下划线显示

**依赖**：`termimad` 或 `mdriver` crate

### F3：会话成本追踪

**行业参考**：
- agent-token-meter：实时 burn rate + 累计成本
- Claude Code Issue #39187：context window usage indicator

**建议**：在 REPL 提示符或状态栏中持续显示：
```
loom (react) [15K ctx / $0.08] > 
```

### F4：交互式 Diff 预览和审批

**行业参考**：
- mdiff：TUI diff reviewer，支持 side-by-side、unified view、inline annotations
- agrev：agent trace + diff 交叉引用

**建议**（长期）：当 agent 执行 `edit` 工具时，展示 inline diff 并等待用户确认：
```
── src/main.rs ────────────────────────
 1  use std::io;
 2  
 3 - fn main() {
 3 + fn main() -> Result<(), Box<dyn std::error::Error>> {
 4      println!("Hello");
 5 - }
 5 +     Ok(())
 6 + }
──────────────────────────────────────
[y] 接受  [n] 拒绝  [e] 编辑  [v] vim 打开
```

### F5：上下文压缩提醒

**行业参考**：Claude Code 的自动 compact

**建议**：当 context 使用率超过阈值时，自动提醒或自动触发 compact：
```
⚠ Context: 180K/200K (90%) — 自动压缩上下文... (/compact 跳过)
```

---

## 推荐优先级调整

| 原方案阶段 | 调整建议 |
|-----------|---------|
| 第一阶段：Spinner（原 P0）| **保持 P0**，增加耗时显示 |
| 第二阶段：面板格式（原 P1）| **降为 P2**，前缀对用户意义不大，改为更视觉化的摘要 |
| 第三阶段：思考/回复分离（原 P2）| **降为 P3** |
| 第四阶段：统一 Usage（原 P2）| **保持 P2**，增加会话累计和 context 预警 |
| **新增 F3：成本追踪** | **提升为 P1** |
| **新增 F1：工具审批流** | **提升为 P1** — 这是用户信任的基石 |
| **新增 F2：Markdown 渲染** | **P2** |
| 第五阶段：REPL 增强（原 P3）| **保持 P3**，rustyline 足够 |
| 第六阶段：详细度分级（原 P3）| **保持 P3** |
| **新增 F5：Context 压缩提醒** | **P2** |
| **新增 F4：交互式 Diff** | **P4**（长期，需要 TUI 框架）|

### 建议实施路线

**UX v2（快速见效）**：
1. Spinner + 耗时（第一阶段增强）
2. 成本追踪（F3）
3. 工具审批流 - 基础版（F1）
4. 统一 Usage + Context 预警（第四阶段增强）

**UX v3（深度体验）**：
5. Markdown 渲染（F2）
6. 文件变更摘要预览
7. 详细度分级（第六阶段）
8. REPL 增强（第五阶段）

**UX v4（终端 TUI）**：
9. 交互式 Diff 预览（F4）
10. 全 TUI 界面（`ratatui`/`bubbletea`）

---

## 技术选型建议

| 需求 | 推荐方案 | 替代方案 |
|------|---------|---------|
| Spinner | 自实现（`\r` + TTY 检测）| `indicatif` crate（功能更全，但较重）|
| Markdown 渲染 | `termimad` | `mdriver`（更新，streaming 原生）|
| REPL | `rustyline 14` | — |
| 颜色 | `colored` 或 `console` crate | 手写 ANSI（当前方案）|
| Diff 格式化 | `similar` crate | 手写 unified diff |
| Token 计数 | 自实现（从 LLM response 解析）| `tiktoken-rs` |
| TUI（长期）| `ratatui` | `bubbletea` (Go 风格)、`telex-tui`（React-like）|

---

## 与竞品对比总结

| 功能 | Loom 当前 | Loom 方案 | Claude Code | Aider | Gemini CLI |
|------|----------|----------|-------------|-------|------------|
| 进度指示 | ✗ | ✓ Spinner | ✓ Ink TUI | ✓ 简单 | ✗（社区提案）|
| 工具调用可视化 | ✗ | ✓ 前缀面板 | ✓ 折叠区块 | ✓ 颜色区分 | ✗ |
| Markdown 渲染 | ✗ | ✗ | ✓ | 部分 | ✗ |
| 文件变更预览 | ✗ | ✗ | ✓ 折叠 diff | ✓ inline diff | ✗ |
| 工具审批 | ✗ | ✗ | ✓ 4 级 | ✓ 确认 | ✗ |
| 成本追踪 | 仅单轮 | 仅单轮 | ✓ 累计 | ✓ 每轮成本 | ✗ |
| Context 预警 | ✗ | ✗ | 社区请求 | ✓ | ✗ |
| 命令历史 | ✗ | ✓ rustyline | ✓ Ink | ✓ prompt_toolkit | ✓ |
| 详细度分级 | verbose 开/关 | 三级 | 三级 | 多级 | — |
| 主题/颜色 | ✗ | `NO_COLOR` | ✓ 自动检测 | ✓ dark/light | ✗ |

**关键差距**：工具审批流和 Markdown 渲染是与 Claude Code 体验差距最大的两个维度。
