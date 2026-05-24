# OpenAI-compat 双重序列化 Bug 调查与修复

## 错误现象

调用 OpenAI 兼容接口时返回 400 错误：

```
OpenAI-compat stream error 400 Bad Request: invalid params, invalid function arguments json string,
tool_call_id: call_1f146c78-6449-6e0e-83ec-3a62ec26c3ec (2013) (type: bad_request_error)
```

## 根因分析

问题出在 `loom/src/llm/openai_compat.rs` 的 `ChatMessageRequest` 结构体：

```rust
// 修复前 — content 是 String 类型
struct ChatMessageRequest {
    role: String,
    content: Option<String>,  // <-- 问题根源
    ...
}
```

### 双重序列化过程

1. 构造 user 消息时，`content_value` 已经是 `serde_json::Value`
2. 调用 `.to_string()` 把它转成字符串，例如：
   - 文本 `"hello"` → `"\"hello\""`（多了双引号）
   - 多模态数组 → `"[{\"type\":\"text\",...}]"`（整个数组变成了字符串）
3. `serde_json` 序列化 `ChatMessageRequest` 时，又对这段字符串做了一次 JSON 转义
4. 最终 API 收到的 `content` 字段格式不符合 OpenAI 规范，返回 400

### 影响范围

- System 消息：`content: Some(s.clone())` — String 直接传给 `Option<String>`，无额外转义，**但类型不统一**
- User 消息：`content: Some(content_value.to_string())` — **双重序列化**，这是主要 bug
- Assistant 消息：`content: Some(c.into_owned())` — 同 System，类型不统一
- Tool 消息：`content: Some(content.to_display_string())` — 类型不统一

## 修复方案

将 `ChatMessageRequest.content` 改为 `Option<serde_json::Value>`，所有赋值处统一用 `serde_json::Value` 包裹。

### 修改文件

`loom/src/llm/openai_compat.rs`

### 1. 结构体定义

```rust
// 修复后
struct ChatMessageRequest {
    role: String,
    content: Option<serde_json::Value>,  // String → serde_json::Value
    ...
}
```

### 2. System 消息

```rust
// 修复前
content: Some(s.clone()),
// 修复后
content: Some(serde_json::Value::String(s.clone())),
```

### 3. User 消息

```rust
// 修复前
content: Some(content_value.to_string()),
// 修复后
content: Some(content_value),
```

### 4. Assistant 消息

```rust
// 修复前
Some(c.into_owned())
// 修复后
Some(serde_json::Value::String(c.into_owned()))

// 修复前
Some(payload.content.clone())
// 修复后
Some(serde_json::Value::String(payload.content.clone()))
```

### 5. Tool 消息

```rust
// 修复前
Message::Tool { ... }
content: Some(content.to_display_string()),
// 修复后
Message::ToolResult { ... }  // 类型名可能需要同步确认
content: Some(serde_json::Value::String(content.to_display_string())),
```

## JSON 案例

### 案例 1：纯文本 User 消息

用户发送 `"hello"` 时，`content_value` 已经是 `serde_json::Value::String("hello")`。

**修复前** — `.to_string()` 导致双重序列化：

```json
{
  "role": "user",
  "content": "\"hello\""
}
```

API 收到的 content 是 `"hello"`（带引号的字符串），而不是 `hello`。

**修复后** — 直接传 `serde_json::Value`：

```json
{
  "role": "user",
  "content": "hello"
}
```

### 案例 2：多模态 User 消息

用户发送图片+文本时，`content_value` 是一个 JSON 数组。

**修复前** — 整个数组被 `.to_string()` 变成字符串：

```json
{
  "role": "user",
  "content": "[{\"type\":\"text\",\"text\":\"describe this\"},{\"type\":\"image_url\",\"image_url\":{\"url\":\"https://example.com/img.png\"}}]"
}
```

API 收到的 content 是一个字符串（不是数组），无法解析。

**修复后** — 保留 JSON 数组结构：

```json
{
  "role": "user",
  "content": [
    {"type": "text", "text": "describe this"},
    {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
  ]
}
```

### 案例 3：Tool 消息

工具执行结果为 `"file not found"`。

**修复前**：

```json
{
  "role": "tool",
  "tool_call_id": "call_abc123",
  "content": "file not found"
}
```

看似正确，但 `content` 的 Rust 类型是 `String`，如果结果包含特殊字符（引号、换行）会被 serde 二次转义。

**修复后** — 统一用 `serde_json::Value::String`：

```json
{
  "role": "tool",
  "tool_call_id": "call_abc123",
  "content": "file not found"
}
```

序列化行为一致，特殊字符不会被双重转义。

### 案例 4：完整请求体对比

一个包含 system + user + tool_call + tool_result 的多轮对话。

**修复前**：

```json
{
  "model": "glm-4",
  "stream": true,
  "messages": [
    {"role": "system", "content": "you are helpful"},
    {"role": "user", "content": "\"read foo.rs\""},
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [{
        "id": "call_1f146c78",
        "type": "function",
        "function": {"name": "read", "arguments": "{\"path\":\"foo.rs\"}"}
      }]
    },
    {
      "role": "tool",
      "tool_call_id": "call_1f146c78",
      "content": "fn main() {}"
    }
  ]
}
```

注意 user 的 content 是 `"\"read foo.rs\""` — 多了一层引号，API 解析失败返回 400。

**修复后**：

```json
{
  "model": "glm-4",
  "stream": true,
  "messages": [
    {"role": "system", "content": "you are helpful"},
    {"role": "user", "content": "read foo.rs"},
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [{
        "id": "call_1f146c78",
        "type": "function",
        "function": {"name": "read", "arguments": "{\"path\":\"foo.rs\"}"}
      }]
    },
    {
      "role": "tool",
      "tool_call_id": "call_1f146c78",
      "content": "fn main() {}"
    }
  ]
}
```

## 验证方法

1. 构造包含纯文本和多模态内容的消息
2. 在 `build_request` 中打印序列化后的 JSON，确认 `content` 字段格式正确：
   - 纯文本：`"content": "hello"`（不是 `"content": "\"hello\""`)
   - 多模态：`"content": [{"type": "text", "text": "..."}]`（不是字符串）
3. 发送请求，确认 400 错误消失

## 相关文件

- `loom/src/llm/openai_compat.rs` — 主要修改文件
- `loom/src/message.rs` — Message 枚举定义
- `loom/src/llm/openai/mod.rs` — 另一个 OpenAI 客户端（async_openai），可作参考
