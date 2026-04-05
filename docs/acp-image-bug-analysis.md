# ACP Embedded Content 图片支持 Bug 分析与修复方案

## 问题描述

ACP 协议传入的图片数据在某些场景下未被正确处理，导致图片丢失或日志异常。

## 完整链路分析

```
ACP Client
  ↓ JSON-RPC (session/prompt)
  ↓ ContentBlock::Image { mime_type, data, uri }
loom-acp/src/content.rs (content_blocks_to_user_content)
  ↓ ContentPart::ImageBase64 { media_type, data }
  ↓ UserContent::Multimodal(parts)
loom-acp/src/agent.rs (prompt handler)
  ↓ RunOptions { message: UserContent }
loom/src/cli_run/agent.rs
  ↓ runner.stream_with_config(&opts.message)
loom/src/agent/react/runner/runner.rs
  ↓ build_react_initial_state(user_message: &UserContent)
  ↓ Message::user(user_message_owned)
  ↓ ReActState { messages }
loom/src/agent/react/think_node.rs
  ↓ llm.invoke(&state.messages)
loom/src/llm/openai_compat.rs
  ↓ Message::User(UserContent::Multimodal) → JSON parts
  ↓ ContentPart::ImageBase64 → { type: "image_url", image_url: { url: "data:image/png;base64,..." } }
LLM API
```

**结论：主链路（Image ContentBlock）已完整支持多模态传递。**

## Bug 列表

### Bug 1：Binary Embedded Resource 图片被丢弃

**严重程度**：高

**位置**：`loom-acp/src/content.rs:332-336`

**现象**：ACP 协议中 `Resource` 类型的 ContentBlock 可以携带图片（blob），但代码只处理了 `TextResourceContents`，对 `BlobResourceContents` 只打了 debug 日志就跳过了。

**当前代码**：

```rust
EmbeddedResourceResource::BlobResourceContents(blob_res) => {
    tracing::debug!(
        uri = %blob_res.uri,
        mime = ?blob_res.mime_type,
        "Skipping binary embedded resource"
    );
}
```

**修复方案**：判断 MIME 类型，如果是图片或音频则转为对应的 ContentPart：

```rust
EmbeddedResourceResource::BlobResourceContents(blob_res) => {
    let mime = blob_res.mime_type.as_deref().unwrap_or("");
    if mime.starts_with("image/") {
        parts.push(ContentPart::ImageBase64 {
            media_type: mime.to_string(),
            data: blob_res.blob.clone(),
        });
    } else if mime.starts_with("audio/") {
        parts.push(ContentPart::AudioBase64 {
            media_type: mime.to_string(),
            data: blob_res.blob.clone(),
        });
    } else {
        tracing::debug!(
            uri = %blob_res.uri,
            mime = ?blob_res.mime_type,
            "Skipping binary embedded resource"
        );
    }
}
```

### Bug 2：日志打印完整 base64 数据

**严重程度**：中

**位置**：`loom-acp/src/agent.rs:225-228`

**现象**：`content = ?user_content` 用 Debug 格式化把完整 base64 图片数据写入日志，导致日志文件暴增（单张图片可达数 MB）、日志不可读。

**当前代码**：

```rust
tracing::info!(
    session_id = %args.session_id,
    content = ?user_content,
    "User prompt"
);
```

**修复方案**：只记录摘要信息：

```rust
tracing::info!(
    session_id = %args.session_id,
    content_type = match &user_content {
        UserContent::Text(_) => "text",
        UserContent::Multimodal(parts) => {
            let has_image = parts.iter().any(|p| matches!(p, ContentPart::ImageBase64 { .. }));
            let has_audio = parts.iter().any(|p| matches!(p, ContentPart::AudioBase64 { .. }));
            if has_image && has_audio { "multimodal(image+audio)" }
            else if has_image { "multimodal(image)" }
            else if has_audio { "multimodal(audio)" }
            else { "multimodal" }
        }
    },
    text_len = user_content.as_text().len(),
    "User prompt"
);
```

### Bug 3：Image 空数据无警告日志（已修复）

**严重程度**：低

**位置**：`loom-acp/src/content.rs:291-307`

**现象**：当 Image ContentBlock 的 `data` 为空且 `uri` 为 `None` 时，图片被静默跳过，无任何日志。

**状态**：已在本次修复中添加 `tracing::warn!` 日志。

## ACP 图片协议格式

### Image ContentBlock（直接传图）

```json
{
  "type": "image",
  "mimeType": "image/png",
  "data": "iVBORw0KGgoAAAANSUhEUg..."
}
```

字段：
- `data`: Base64 编码的图片数据（String）
- `mimeType`: MIME 类型，如 `image/png`、`image/jpeg`、`image/gif`、`image/webp`
- `uri`: 可选，图片 URL 引用（Option\<String\>）

### Resource ContentBlock（嵌入资源传图）

```json
{
  "type": "resource",
  "resource": {
    "blob": "iVBORw0KGgoAAAANSUhEUg...",
    "uri": "file:///screenshot.png",
    "mimeType": "image/png"
  }
}
```

## 影响范围

| 修改 | 文件 | 行号 | 风险 |
|------|------|------|------|
| Bug 1 | `loom-acp/src/content.rs` | 332-336 | 低 — 新增分支，不影响已有逻辑 |
| Bug 2 | `loom-acp/src/agent.rs` | 225-228 | 低 — 纯日志改动 |
| Bug 3 | `loom-acp/src/content.rs` | 291-307 | 已修复 |

## 测试建议

### 单元测试

1. 验证 `BlobResourceContents` 图片正确转为 `ContentPart::ImageBase64`
2. 验证 `BlobResourceContents` 音频正确转为 `ContentPart::AudioBase64`
3. 验证 `BlobResourceContents` 非 image/audio 类型仍然跳过并打日志
4. 验证 Image 空数据 + 无 URI 输出警告日志

### E2E 测试

1. 启动 loom-acp 子进程，发送 `session/prompt` 携带 Image ContentBlock（base64）
2. 验证 prompt 成功处理，LLM 收到图片数据
3. 验证日志中不包含完整 base64 数据，只包含摘要信息
