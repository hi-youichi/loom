# 流式 Markdown 渲染器设计

> **状态**：草案
> **日期**：2025-07
> **范围**：CLI 流式输出（reasoning + 助手消息）

## 1. 问题

当前 CLI 的流式输出将 LLM 的 Markdown 内容**原样输出**到终端，没有 ANSI 格式化：

| 内容类型 | 当前行为 | 期望行为 |
|---------|---------|---------|
| Thinking/Reason | `dim()` 灰色输出 | 保持 dim，可选 markdown |
| 助手消息 (Message) | 纯文本 `print!()` | **渲染 Markdown 为 ANSI** |

`render_markdown()` 已存在于 `loom/src/stream_display/markdown.rs`，但它是对**完整文本**做行级解析的，无法用于 token-by-token 的流式场景。

### 核心难点

流式输出是 chunk-by-chunk 到达的，一个 Markdown 构造可能被拆成多个 chunk：

```
chunk1: "这是"
chunk2: "**加"
chunk3: "粗**"
chunk4: "和"
chunk5: "*斜体*"
chunk6: "文本\n"
```

无法对单个 chunk 独立解析，需要**有状态的渲染器**跨 chunk 累积并渲染。

## 2. 方案：行缓冲状态机

### 2.1 架构

```
┌─────────────────────────────────────────────────────┐
│  LLM SSE Stream                                      │
│    ↓ MessageChunk (token-by-token)                   │
│  ┌─────────────────────────────────────────────────┐ │
│  │  StreamingMarkdownRenderer (状态机)              │ │
│  │                                                   │ │
│  │  ┌───────────┐    ┌──────────────┐              │ │
│  │  │ Line Buffer│───→│ Line Renderer│──→ ANSI out  │ │
│  │  │ (逐字符累积) │    │ (render_inline)│              │ │
│  │  └───────────┘    └──────────────┘              │ │
│  │         ↕  code_block toggle                     │ │
│  │  ┌──────────────────────────────┐               │ │
│  │  │ Code Block State             │               │ │
│  │  │ in_code_block: bool          │               │ │
│  │  │ code_lang: String            │               │ │
│  │  └──────────────────────────────┘               │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 2.2 状态机

```
                    ┌──────────┐
                    │  Normal  │ ← 默认状态
                    └────┬─────┘
                         │ 收到 '\n'
                         ↓
              ┌─────────────────────┐
              │  flush_line()       │
              │  渲染完整的一行     │
              └─────────────────────┘
                         │
              ┌──────────┼──────────────────┐
              ↓          ↓                  ↓
     ┌────────────┐ ┌──────────┐   ┌──────────────┐
     │ heading    │ │ list/quote│   │ plain line   │
     │ # ## ###   │ │ - > 1.   │   │ render_inline│
     └────────────┘ └──────────┘   └──────────────┘
                                     │
                          ┌──────────┼──────────┐
                          ↓          ↓          ↓
                    ┌─────────┐ ┌────────┐ ┌───────┐
                    │**bold** │ │`code`  │ │*italic*│
                    └─────────┘ └────────┘ └───────┘

     额外独立状态：
     ┌───────────────────────────────────────────┐
     │  in_code_block = true                     │
     │  收到 ``` → toggle                        │
     │  行内容直接 dim 输出，不做 inline 解析    │
     └───────────────────────────────────────────┘
```

### 2.3 关键设计决策

| 决策 | 理由 |
|------|------|
| **行级缓冲**（而非逐字符状态机） | Markdown 的行级构造（heading、list、blockquote）必须看到完整行首才能判定；`render_inline()` 已是完整实现 |
| **复用现有 `markdown.rs` 的解析和格式化函数** | 避免重复实现，保持一致的渲染效果 |
| **Thinking 不走渲染器** | Thinking 内容用 dim 灰色直接输出，不经过行缓冲 |
| **代码块内不做 inline 渲染** | 代码块内容直接 dim 输出，避免 `**` 等被错误解析 |

## 3. 数据结构

```rust
// loom/src/stream_display/streaming_markdown.rs

pub struct StreamingMarkdownRenderer {
    /// 当前行缓冲区（累积到 '\n' 时整行渲染）
    line_buf: String,
    /// 是否在代码块内
    in_code_block: bool,
    /// 代码块语言标签
    code_lang: String,
}
```

## 4. 核心算法

### 4.1 push_chunk — 处理每个流式 chunk

```rust
impl StreamingMarkdownRenderer {
    pub fn push_chunk(&mut self, chunk: &MessageChunk) {
        // ── Thinking: 不做 markdown，直接 dim 输出 ──
        if chunk.kind == MessageChunkKind::Thinking {
            eprint!("{}", panel_format::dim(&chunk.content));
            let _ = std::io::Write::flush(&mut std::io::stderr());
            return;
        }

        // ── Message: 行缓冲 + markdown 渲染 ──
        for ch in chunk.content.chars() {
            if ch == '\n' {
                self.flush_line();
            } else {
                self.line_buf.push(ch);
            }
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}
```

### 4.2 flush_line — 渲染完整一行

```rust
fn flush_line(&mut self) {
    let line = std::mem::take(&mut self.line_buf);

    // ── 代码块围栏检测 ──
    if line.trim_start().starts_with("```") {
        if self.in_code_block {
            print!("{}", format_code_block_end());
            self.in_code_block = false;
        } else {
            let lang = line.trim_start().trim_start_matches('`').trim();
            self.code_lang = lang.to_string();
            print!("{}", format_code_block_start(&self.code_lang));
            self.in_code_block = true;
        }
        println!();
        return;
    }

    // ── 代码块内：dim 输出，不做 markdown ──
    if self.in_code_block {
        println!("{}", format_code_line(&line));
        return;
    }

    // ── 正常行：行级 + inline markdown 渲染 ──
    let rendered = self.render_line(&line);
    println!("{}", rendered);
}
```

### 4.3 render_line — 行级 Markdown 渲染

复用 `markdown.rs` 中的现有解析函数：

```rust
fn render_line(&self, line: &str) -> String {
    if let Some((level, content)) = parse_heading(line) {
        return format_heading(level, content);
    }
    if let Some(content) = parse_unordered_list_item(line) {
        return format_list_item("•", content);
    }
    if let Some((num, content)) = parse_ordered_list_item(line) {
        return format_list_item(&format!("{}.", num), content);
    }
    if let Some(content) = line.strip_prefix('>') {
        return format_blockquote(content.trim_start());
    }
    if is_horizontal_rule(line.trim()) {
        return format_horizontal_rule();
    }
    // 默认：inline 渲染（bold, italic, code, links）
    render_inline(line)
}
```

### 4.4 finish — 流结束清理

```rust
pub fn finish(&mut self) {
    // 刷新未换行的残余内容
    if !self.line_buf.is_empty() {
        self.flush_line();
    }
    // 自动闭合未关闭的代码块
    if self.in_code_block {
        println!("{}", format_code_block_end());
        self.in_code_block = false;
    }
}
```

## 5. 数据流对比

### 改造前（当前）

```
chunk.content → print!() → 终端（纯文本）
```

### 改造后

```
chunk.content → StreamingMarkdownRenderer.push_chunk()
                    │
                    ├─ Thinking → dim() → stderr（无 markdown）
                    │
                    └─ Message → line_buf 累积
                                    │
                                    ├─ '\n' 到达 → flush_line()
                                    │     ├─ 代码块围栏？ → toggle code_block
                                    │     ├─ 在代码块内？ → dim 输出
                                    │     └─ 正常行 → render_line() → ANSI 输出
                                    │
                                    └─ 未到 '\n' → 继续累积（不可见）
```

## 6. 集成方案

### 6.1 需要改动的文件

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `loom/src/stream_display/streaming_markdown.rs` | **新增** | 状态机实现 |
| `loom/src/stream_display/mod.rs` | 修改 | 新增 `pub mod streaming_markdown;` |
| `loom/src/stream_display/markdown.rs` | 修改 | 将解析和格式化函数的可见性提升为 `pub(crate)` |
| `loom/src/stream_display/event_handler.rs` | 修改 | `print_stream_chunk()` → 使用 renderer |
| `cli/src/run/agent.rs` | 修改 | `print_stream_chunk()` → 使用 renderer |

### 6.2 集成方式

`renderer` 实例存储在 `EventState` 中（跨 chunk 有状态）：

```rust
// 改造前
struct EventState {
    // ...
}

fn print_stream_chunk(chunk: &MessageChunk) {
    if chunk.kind == MessageChunkKind::Thinking {
        eprint!("{}", panel_format::dim(&chunk.content));
    } else {
        print!("{}", chunk.content);
    }
}
```

```rust
// 改造后
struct EventState {
    // ... 现有字段 ...
    markdown_renderer: StreamingMarkdownRenderer,
}

fn print_stream_chunk(chunk: &MessageChunk, renderer: &mut StreamingMarkdownRenderer) {
    renderer.push_chunk(chunk);
}
```

在 `StreamEvent::Messages` 分支中：

```rust
StreamEvent::Messages { chunk, .. } => {
    // ... 现有的 spinner/agent banner 逻辑 ...
    print_stream_chunk(chunk, &mut s.markdown_renderer);
}
```

在 agent run 结束时：

```rust
// 流结束后，刷新 renderer 残余
s.markdown_renderer.finish();
```

## 7. UX 影响

### 7.1 行缓冲的"延迟感"

用户不再看到逐字符出现，而是**逐行出现**。

**实际影响很小**：
- LLM token 通常每 20-50ms 到达
- 一行通常在 100-200ms 内填满，体感上几乎是"即时"出现
- 代码块（缩进式）和列表的视觉体验会更好

### 7.2 可选优化：超时冲刷

如果用户感知到明显的行级延迟，可以加入超时机制：

```rust
/// 如果长时间没有新 chunk 且 line_buf 非空，主动 flush
pub fn maybe_flush_stale(&mut self, timeout_ms: u64) {
    if !self.line_buf.is_empty()
        && self.last_chunk_time.elapsed() > Duration::from_millis(timeout_ms)
    {
        self.flush_line();
    }
}
```

## 8. Edge Cases

| 场景 | 处理方式 |
|------|---------|
| 流结束时最后一行没有 `\n` | `finish()` 强制 flush `line_buf` 残余 |
| 代码块未闭合就结束 | `finish()` 自动补 `format_code_block_end()` |
| `**` 跨 chunk 到达 | 行缓冲天然处理：累积到 `\n` 才渲染，此时 inline 标记已完整 |
| Thinking → Message 切换 | Thinking chunk 直接 dim 输出（不走 line_buf），Message chunk 走渲染器 |
| 空行 | `flush_line()` 输出空 `\n`，保持段落间距 |
| `chunk.content` 包含多行 | `for ch in content.chars()` 逐字符处理，`\n` 自然分割 |
| `\r\n`（Windows 换行） | 可选：`\r` 也触发 flush，或预处理 strip `\r` |
| 嵌套格式（`**bold `code` text**`） | `render_inline()` 递归处理，已有实现 |

## 9. 性能

| 指标 | 预估 |
|------|------|
| 内存 | 每行一个 `String` buffer，通常 < 1KB |
| CPU | 每行一次 `render_inline()`，复杂度 O(line_length) |
| 延迟 | 每行延迟 = 等待 `\n` 到达，通常 < 200ms |

## 10. 实现步骤

1. **将 `markdown.rs` 中的解析/格式化函数暴露为 `pub(crate)`**
2. **新建 `streaming_markdown.rs`，实现 `StreamingMarkdownRenderer`**
3. **在 `event_handler.rs` 中集成**（loom crate 的 `print_stream_chunk`）
4. **在 `cli/src/run/agent.rs` 中集成**（cli crate 的 `print_stream_chunk`）
5. **单元测试**：chunk 拆分、代码块、行缓冲、finish 清理
6. **集成测试**：端到端流式渲染验证
