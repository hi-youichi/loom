# Fix: ACP ToolCall rawOutput 丢失命令执行结果

## 问题

ACP 客户端收到 `bash`/`powershell` 等执行类工具的 `ToolCallUpdate` 时，`rawOutput` 字段可能只包含截断后的摘要文本（如 `"[Output saved to ~/.loom/output/bash_xxx.txt (12345 chars)]"`），而非完整的命令输出。

根因：`act_node.rs` 中 `normalize_tool_output` 会根据 observation budget 将长输出替换为 head-tail 摘要或文件引用，`StreamEvent::ToolEnd.result` 携带的是 normalize 后的 `display_text`，而 `stream_bridge.rs` 直接用它构造 ACP `rawOutput`，原始输出在中间丢失。

## 方案

在 `ToolEnd` 事件链路中新增 `raw_result` 字段，当 normalize 改变了输出内容时，将原始未截断文本一并传递到 ACP 层。

```
Before:
  BashTool.call() → raw text
    → normalize_tool_output() → display_text (可能截断)
    → StreamEvent::ToolEnd { result: display_text }
    → ToolCallUpdateFields.raw_output(display_text)  ← 丢失

After:
  BashTool.call() → raw text
    → normalize_tool_output() → display_text
    → StreamEvent::ToolEnd { result: display_text, raw_result: Some(raw_text) }
    → ToolCallUpdateFields.raw_output(raw_text)      ← 完整
    → ToolCallUpdateFields.content(display_text)     ← 展示用
```

## 改动文件

| 文件 | 改动 |
|------|------|
| `stream-event/src/event.rs` | `ProtocolEvent::ToolEnd` 增加 `raw_result: Option<String>` |
| `loom/src/stream/stream_event.rs` | `StreamEvent::ToolEnd` 增加 `raw_result: Option<String>` |
| `loom/src/agent/react/act_node.rs` | 成功路径保存原始文本，当 `display_text != raw_text` 时设置 `raw_result` |
| `loom/src/protocol/stream.rs` | 映射 `raw_result` 到 `ProtocolEvent` |
| `loom/src/export/mod.rs` | 导出时包含 `raw_result` |
| `loom-acp/src/stream_bridge.rs` | `StreamUpdate::ToolCallUpdated` 增加 `raw_output`；`rawOutput` 优先使用 `raw_output` |
| `loom/src/stream/writers/stream_writer.rs` | `emit_tool_end` 增加 `raw_result` 参数 |
| `loom/src/stream/tests/*.rs` | 测试补全 `raw_result` 字段 |
| `stream-event/src/event.rs` tests | 测试补全 `raw_result` 字段 |

## ACP 输出对比

### Before

```json
{
  "toolCallUpdate": {
    "toolCallId": "call-001",
    "status": "completed",
    "content": [{ "type": "content", "content": { "type": "text", "text": "[Output saved to ~/.loom/output/bash_abc.txt (12345 chars)]" } }],
    "rawOutput": "[Output saved to ~/.loom/output/bash_abc.txt (12345 chars)]"
  }
}
```

### After

```json
{
  "toolCallUpdate": {
    "toolCallId": "call-001",
    "status": "completed",
    "content": [{ "type": "content", "content": { "type": "text", "text": "[Output saved to ~/.loom/output/bash_abc.txt (12345 chars)]" } }],
    "rawOutput": "stdout:\n   Compiling loom v0.1.0\n   Compiling cli v0.1.5\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 37s\n..."
  }
}
```

- `content` 保持 normalize 后的展示文本（控制 token 消耗）
- `rawOutput` 包含完整未截断的命令输出（供客户端渲染或展开查看）
