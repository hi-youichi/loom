# ACP Content 合规性追踪

对照 [ACP Content 规范](https://agentclientprotocol.com/protocol/content)，追踪本地 `loom-acp` 的实现进度。

最后更新：2025-08-19

## ContentBlock 类型

| # | 类型 | 协议要求 | 状态 | 实现位置 | 备注 |
|---|------|---------|------|---------|------|
| 1 | **Text** | Agent MUST 支持 | ✅ 完成 | `content.rs` | 提取 `text` 字段 |
| 2 | **Image** | 需 `image` prompt capability | ✅ 完成 | `content.rs` | 支持 base64 data 和 URI 引用 |
| 3 | **Audio** | 需 `audio` prompt capability | ✅ 完成 | `content.rs` | 支持 base64 data |
| 4 | **Resource (Embedded)** | 需 `embeddedContext` capability | ✅ 完成 | `content.rs` | Text resource + Blob resource 均处理 |
| 5 | **Resource Link** | Agent MUST 支持 | ✅ 完成 | `content.rs` | 提取 uri/name/description/mime_type |

## 已修复问题

### ~~P1 — Resource Link 未处理~~ `[已修复 ✅]`

- **修复**: `as_text()` 和 `content_blocks_to_user_content()` 均提取 `uri` + `name` + `description` + `mime_type` 格式化为文本。
- **位置**: `content.rs` 的 `ContentBlockLike` impl 和 `content_blocks_to_user_content` 函数。

### ~~P2 — Blob Resource 静默丢弃~~ `[已修复 ✅]`

- **修复**: Blob resource 现在返回文本引用 `"--- Binary Resource ---\nURI: ...\nMIME: ...\nSize: ..."`，不再静默丢弃。
- **位置**: `content.rs` 的 `as_text()` 和 `content_blocks_to_user_content`。

### ~~P3 — 本地 ContentBlock 枚举与协议不对齐~~ `[已修复 ✅]`

- **修复**: 补充了 `Audio` 和 `ResourceLink` 变体；`Image` 字段对齐为 `data` + `mime_type` + `uri`。
- **位置**: `content.rs` 本地 `ContentBlock` 枚举。

## 待处理（低优先级）

### P4 — Capability 运行时检查缺失 `[低优先级]`

- `content_blocks_to_user_content` 不校验客户端是否声明了 `image`/`audio`/`embeddedContext`。
- initialize 时声明了全部 capability，实际不会触发问题。
- **建议**: 后续添加 capability 降级逻辑。

### P5 — Annotations 未使用 `[低优先级]`

- 协议中所有 ContentBlock 均有可选 `annotations` 字段，本地实现忽略。
- 暂无业务场景需要 annotations。
- **建议**: 待有需求时再处理。

## 进度汇总

| 指标 | 数量 |
|------|------|
| 总 ContentBlock 类型 | 5 |
| ✅ 完成 | 5 (Text, Image, Audio, Resource, ResourceLink) |
| 高优待修复项 | 0 |
| 低优待处理项 | 2 (Capability 检查, Annotations) |
