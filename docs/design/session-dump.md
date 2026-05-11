---
sidebar_position: 1
title: "Session Dump 方案"
description: "会话转储设计方案"
---

# Session Dump 方案

## 背景

部分 OpenAI 兼容模型在 tool call 场景下返回非法 JSON arguments，导致后续请求被 API 拒绝：

```
invalid function arguments json string, tool_call_id: call_1f149f69... (400 Bad Request)
```

需要 CLI 支持导出 session 完整状态，便于定位具体哪个 message 的 tool call arguments 有问题。

## 命令

```bash
# 人类可读摘要（默认）
loom session dump <session_id>

# JSON 输出，重定向到文件
loom --json session dump <session_id> > session.json
loom --json --pretty session dump <session_id> > session.json
```

`--json` 和 `--pretty` 为全局 flag，沿用现有模式（`cli/src/args.rs`），与 `session list`、`session show` 行为一致。

## 输出结构

JSON 输出一个 `SessionDump` 对象：

```json
{
  "session_id": "01912abc...",
  "checkpoint_id": "01912def...",
  "step": 8,
  "source": "Loop",
  "created_at": "2025-08-19T10:00:00Z",
  "state": { ... }
}
```

- `state` 是 `ReActState` 的完整 JSON 序列化
- 只取最新 checkpoint（`ReActState.messages` 是 append-only，已包含完整对话历史）
- 完整 JSON Schema 见下方，常用 `jq` 查询也附在 Schema 后


人类可读模式输出：

```
Session: 01912abc...
Checkpoint: 01912def... (step 8, Loop)
Created: 2025-08-19 10:00:00
Messages: 23

Messages:
  [0] system    (234 chars)
  [1] user      (42 chars)
  [2] assistant tool_calls=1 (0 chars)
      tool_call[0]: read id=call_abc args=42 chars
  [3] tool      call_id=call_abc (128 chars)
  ...

Usage: prompt=1234 completion=567 total=1801
```

## 实现细节

### 数据流

```
checkpoints 表 (SQLite)
  → WHERE thread_id = ? ORDER BY metadata_created_at DESC LIMIT 1
  → payload (bytes)
  → serde_json::from_slice::<ReActState>()
  → SessionDump { session_id, checkpoint_id, step, source, created_at, state }
  → JSON 输出 / 人类可读输出
```

### 改动文件

| 文件 | 改动 |
|------|------|
| `cli/src/session.rs` | `SessionCommand` 新增 `Dump { session_id: String }` variant；`SessionManager` 新增 `dump_session()` 方法和 `print_session_dump()` |
| `cli/src/subcommands.rs` | `handle_session_command` 匹配 `Dump`，传 `json` flag |

### 类型定义

```rust
// cli/src/session.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDump {
    pub session_id: String,
    pub checkpoint_id: String,
    pub step: i64,
    pub source: String,
    pub created_at: Option<DateTime<Utc>>,
    pub state: loom::state::ReActState,
}
```

### dump_session 实现

```rust
impl SessionManager {
    pub fn dump_session(&self, session_id: &str) -> Result<Option<SessionDump>, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let mut stmt = conn.prepare(
            r#"
            SELECT checkpoint_id, metadata_step, metadata_source, metadata_created_at, payload
            FROM checkpoints
            WHERE thread_id = ?1
            ORDER BY metadata_created_at DESC
            LIMIT 1
            "#
        ).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        stmt.query_row([session_id], |row| {
            let checkpoint_id: String = row.get(0)?;
            let step: i64 = row.get(1)?;
            let source: String = row.get(2)?;
            let created_at_ms: Option<i64> = row.get(3)?;
            let payload: Vec<u8> = row.get(4)?;

            Ok((checkpoint_id, step, source, created_at_ms, payload))
        })
        .optional()
        .map_err(|e| format!("Failed to query: {}", e))?
        .map(|(checkpoint_id, step, source, created_at_ms, payload)| {
            let state = serde_json::from_slice::<loom::state::ReActState>(&payload)
                .map_err(|e| format!("Failed to deserialize state: {}", e))?;
            Ok(SessionDump {
                session_id: session_id.to_string(),
                checkpoint_id,
                step,
                source,
                created_at: created_at_ms.and_then(DateTime::from_timestamp_millis),
                state,
            })
        })
        .transpose()
    }
}
```

## JSON Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "SessionDump",
  "type": "object",
  "required": ["session_id", "checkpoint_id", "step", "source", "state"],
  "properties": {
    "session_id": { "type": "string", "description": "thread_id" },
    "checkpoint_id": { "type": "string" },
    "step": { "type": "integer" },
    "source": { "type": "string", "enum": ["Input", "Loop", "Update", "Fork"] },
    "created_at": { "type": "string", "format": "date-time" },
    "state": { "$ref": "#/$defs/ReActState" }
  },
  "$defs": {
    "ReActState": {
      "type": "object",
      "properties": {
        "model_config": { "$ref": "#/$defs/ModelConfig" },
        "messages": { "type": "array", "items": { "$ref": "#/$defs/Message" } },
        "last_reasoning_content": { "type": ["string", "null"] },
        "tool_calls": { "type": "array", "items": { "$ref": "#/$defs/ToolCall" } },
        "tool_results": { "type": "array", "items": { "$ref": "#/$defs/ToolResult" } },
        "turn_count": { "type": "integer" },
        "approval_result": { "type": ["boolean", "null"] },
        "usage": { "$ref": "#/$defs/LlmUsage", "description": "最后一次 LLM 调用的 token 用量" },
        "total_usage": { "$ref": "#/$defs/LlmUsage", "description": "累积 token 用量" },
        "message_count_after_last_think": { "type": ["integer", "null"] },
        "think_count": { "type": "integer" },
        "summary": { "type": ["string", "null"] },
        "should_continue": { "type": "boolean" },
        "force_compact": { "type": "boolean" }
      }
    },
    "ModelConfig": {
      "type": "object",
      "properties": {
        "model_id": { "type": "string", "description": "精确模型 ID，如 openai/gpt-4o" },
        "tier": { "type": "string", "enum": ["None", "Light", "Standard", "Strong"] },
        "temperature": { "type": ["number", "null"] },
        "tool_choice": { "type": ["string", "null"], "enum": ["auto", "none", "required", null] }
      }
    },
    "Message": {
      "oneOf": [
        {
          "type": "string",
          "description": "System 消息（纯文本）"
        },
        {
          "$ref": "#/$defs/UserContent",
          "description": "User 消息"
        },
        {
          "$ref": "#/$defs/AssistantPayload",
          "description": "Assistant 消息（无 tool_calls 时序列化为纯文本 string）"
        },
        {
          "type": "object",
          "description": "Tool 结果消息",
          "required": ["tool_call_id", "content"],
          "properties": {
            "tool_call_id": { "type": "string" },
            "content": { "$ref": "#/$defs/ToolCallContent" }
          }
        }
      ],
      "description": "Message enum 的 JSON 序列化由自定义 serde 实现：System→string, User→UserContent, Assistant→string(无 tool_calls) 或 AssistantPayload(有 tool_calls), Tool→{tool_call_id, content}"
    },
    "UserContent": {
      "oneOf": [
        { "type": "string", "description": "纯文本" },
        { "type": "array", "items": { "$ref": "#/$defs/ContentPart" }, "description": "多模态" }
      ]
    },
    "ContentPart": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": { "type": "string", "enum": ["text", "image_url", "image_base64", "audio_base64", "video_url", "video_base64", "pdf_url", "pdf_base64", "file"] }
      },
      "oneOf": [
        { "properties": { "type": { "const": "text" }, "text": { "type": "string" } } },
        { "properties": { "type": { "const": "image_url" }, "url": { "type": "string" }, "detail": { "type": "string" } } },
        { "properties": { "type": { "const": "image_base64" }, "media_type": { "type": "string" }, "data": { "type": "string" } } },
        { "properties": { "type": { "const": "audio_base64" }, "media_type": { "type": "string" }, "data": { "type": "string" } } },
        { "properties": { "type": { "const": "video_url" }, "url": { "type": "string" } } },
        { "properties": { "type": { "const": "video_base64" }, "media_type": { "type": "string" }, "data": { "type": "string" } } },
        { "properties": { "type": { "const": "pdf_url" }, "url": { "type": "string" } } },
        { "properties": { "type": { "const": "pdf_base64" }, "data": { "type": "string" } } },
        { "properties": { "type": { "const": "file" }, "file_id": { "type": "string" }, "file_data": { "type": "string" }, "filename": { "type": "string" } } }
      ]
    },
    "AssistantPayload": {
      "type": "object",
      "properties": {
        "content": { "type": "string" },
        "tool_calls": { "type": "array", "items": { "$ref": "#/$defs/AssistantToolCall" } },
        "reasoning_content": { "type": ["string", "null"] }
      },
      "description": "当 tool_calls 为空且 reasoning_content 为 null 时，整个 Assistant 消息序列化为纯文本 string 而非 object"
    },
    "AssistantToolCall": {
      "type": "object",
      "required": ["id", "name", "arguments"],
      "properties": {
        "id": { "type": "string", "description": "tool_call_id，如 call_01912abc..." },
        "name": { "type": "string", "description": "工具名，如 read, write_file" },
        "arguments": { "type": "string", "description": "参数 JSON 字符串，可能不合法——这是 dump 要排查的核心字段" }
      }
    },
    "ToolCallContent": {
      "oneOf": [
        { "type": "string", "description": "纯文本结果" },
        {
          "type": "object",
          "required": ["type", "path", "new_text"],
          "properties": {
            "type": { "type": "string", "enum": ["diff"] },
            "path": { "type": "string" },
            "old_text": { "type": "string" },
            "new_text": { "type": "string" }
          }
        },
        {
          "type": "object",
          "required": ["type", "terminal_id"],
          "properties": {
            "type": { "type": "string", "enum": ["terminal"] },
            "terminal_id": { "type": "string" }
          }
        }
      ]
    },
    "ToolCall": {
      "type": "object",
      "required": ["name", "arguments"],
      "properties": {
        "name": { "type": "string" },
        "arguments": { "type": "string" },
        "id": { "type": ["string", "null"] }
      },
      "description": "当前轮待执行的 tool call（Think 写入，Act 读取）"
    },
    "ToolResult": {
      "type": "object",
      "properties": {
        "call_id": { "type": ["string", "null"] },
        "name": { "type": ["string", "null"] },
        "content": { "type": "string" },
        "is_error": { "type": "boolean" },
        "raw_content": { "type": ["string", "null"] },
        "observation_text": { "type": ["string", "null"] },
        "display_text": { "type": ["string", "null"] },
        "storage_ref": { "$ref": "#/$defs/ToolStorageRef" },
        "strategy": { "type": ["string", "null"], "enum": ["Inline", "SummaryOnly", "HeadTail", "FileRef", "FileRefWithExcerpt", null] },
        "raw_chars": { "type": ["integer", "null"] },
        "observation_chars": { "type": ["integer", "null"] },
        "truncated": { "type": "boolean" }
      }
    },
    "ToolStorageRef": {
      "type": "object",
      "required": ["path", "size", "content_type", "encoding", "tool_name"],
      "properties": {
        "path": { "type": "string" },
        "size": { "type": "integer" },
        "content_type": { "type": "string" },
        "encoding": { "type": "string" },
        "tool_name": { "type": "string" }
      }
    },
    "LlmUsage": {
      "type": "object",
      "required": ["prompt_tokens", "completion_tokens", "total_tokens"],
      "properties": {
        "prompt_tokens": { "type": "integer" },
        "completion_tokens": { "type": "integer" },
        "total_tokens": { "type": "integer" },
        "prompt_tokens_details": {
          "type": ["object", "null"],
          "properties": {
            "cached_tokens": { "type": ["integer", "null"] },
            "audio_tokens": { "type": ["integer", "null"] }
          }
        },
        "completion_tokens_details": {
          "type": ["object", "null"],
          "properties": {
            "reasoning_tokens": { "type": ["integer", "null"] },
            "audio_tokens": { "type": ["integer", "null"] },
            "accepted_prediction_tokens": { "type": ["integer", "null"] },
            "rejected_prediction_tokens": { "type": ["integer", "null"] }
          }
        }
      }
    }
  }
}
```

### 核心 jq 查询

```bash
# 列出所有 assistant tool_calls
jq '[.state.messages[] | objects | select(has("tool_calls")) | .tool_calls[]] | length' session.json

# 找 arguments 不是合法 JSON 的 tool call
jq '[.state.messages[] | objects | .tool_calls[]? | {id, name, arguments} | select(.arguments | fromjson? | not)]' session.json 2>/dev/null

# 找某个 tool_call_id 对应的 assistant call + tool result
jq --arg id "call_1f149f69" '
  [.state.messages[] | objects
    | (select(.tool_calls[]?.id == $id) | {role: "assistant", tool_calls})
    // (select(.tool_call_id == $id) | {role: "tool", content})
  ] | map(select(. != null))
' session.json

# 提取每条消息的 role + 摘要
jq '[.state.messages[] | if type == "string" then {role: "system", len: length} elif has("tool_calls") then {role: "assistant", tool_calls: (.tool_calls | length), content_len: (.content | length)} elif has("tool_call_id") then {role: "tool", call_id: .tool_call_id, content_len: (.content | length)} else {role: "user", len: (. | if type == "string" then length else (. | tostring | length) end)} end]' session.json
```

## 不做的事

- 不做 `analyze` 子命令（用户 `jq` 自行分析即可）
- 不 dump 全部 checkpoint（最新已含完整历史，后续按需加 `--step N`）
- 不新增 `--output` 参数（用户自行重定向）
