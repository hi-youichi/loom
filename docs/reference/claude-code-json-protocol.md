---
sidebar_position: 1
title: "Claude Code JSON 协议参考"
description: "Claude Code CLI JSON 输出协议的完整字段参考，涵盖 json 和 stream-json 两种输出格式"
---

# Claude Code CLI JSON 输出协议参考

> **版本**：基于 Claude Code CLI v2.1.x，逆向整理自官方文档和社区源码。
> **状态**：非官方。Anthropic 未发布正式 JSON Schema，字段可能随 CLI 版本变化。
> **参考**：
> - 官方文档：https://code.claude.com/docs/en/headless
> - CLI 参考：https://code.claude.com/docs/en/cli-reference
> - 社区 Rust crate：https://docs.rs/claude-codes/latest
> - 社区 Elixir 库：https://hexdocs.pm/claude_code
> - GitHub Issue：https://github.com/anthropics/claude-code/issues/24596

---

## 1. 概述

Claude Code CLI 在 `-p`（`--print`）非交互模式下支持三种输出格式：

| `--output-format` | 格式 | 说明 |
|---|---|---|
| `text`（默认） | 纯文本 | 直接输出回复字符串 |
| `json` | 单个 JSON 对象 | 完成后输出，含元数据 |
| `stream-json` | NDJSON（每行一个 JSON） | 实时流式事件 |

所有 JSON 输出共享相同的字段命名约定：
- `snake_case` 用于顶层字段
- `camelCase` 用于 `rate_limit_info` 和 `modelUsage` 内部字段（来自 API 响应头）
- 所有事件都有 `type` 字段用于区分

---

## 2. `--output-format json` 完整格式

CLI 完成后向 stdout 输出单个 JSON 对象。

### 2.1 ResultEnvelope

```json
{
  "type": "result",
  "subtype": "success",
  "is_error": false,
  "result": "The response text here...",
  "session_id": "fdf2d90a-fd9e-4736-ae35-806edd13643f",
  "total_cost_usd": 0.010621,
  "duration_ms": 2995,
  "duration_api_ms": 2190,
  "num_turns": 1,
  "stop_reason": "end_turn",
  "usage": { ... },
  "modelUsage": { ... },
  "structured_output": null,
  "permission_denials": [],
  "uuid": "d379c496-f33a-4ea4-b920-3c5483baa6f7"
}
```

### 2.2 字段说明

| 字段 | 类型 | 必定存在 | 说明 |
|---|---|---|---|
| `type` | `string` | 是 | 固定 `"result"` |
| `subtype` | `string` | 是 | 结果子类型（见下表） |
| `is_error` | `boolean` | 是 | 是否为错误 |
| `result` | `string` | 是 | 最终文本回复。使用 `--json-schema` 时可能为空字符串 |
| `session_id` | `string` | 是 | 会话 UUID，可用于 `--resume` |
| `total_cost_usd` | `number` | 是 | 本次调用总费用（USD）。新会话为单次费用；`--resume`/`--continue` 为累计 |
| `duration_ms` | `integer` | 是 | 总耗时（毫秒） |
| `duration_api_ms` | `integer` | 否 | API 调用耗时（毫秒） |
| `num_turns` | `integer` | 否 | Agent 轮次数量 |
| `stop_reason` | `string` \| `null` | 否 | 停止原因（见下表） |
| `usage` | `object` | 是 | token 用量统计（见 §2.4） |
| `modelUsage` | `object` | 否 | 按模型分组的用量（见 §2.5） |
| `structured_output` | `any` | 否 | `--json-schema` 的结构化输出（见 §2.6） |
| `permission_denials` | `array` | 否 | 权限拒绝记录 |
| `uuid` | `string` | 否 | 消息 UUID |

### 2.3 subtype 枚举

| 值 | 说明 |
|---|---|
| `"success"` | 正常完成 |
| `"error"` | 一般错误 |
| `"error_max_budget_usd"` | 超出 `--max-budget-usd` 预算上限 |
| `"error_max_turns"` | 超出 `--max-turns` 轮次上限 |

> CLI 可能返回其他未文档化的 error 子类型。解析器应对未知值做兜底处理。

### 2.4 stop_reason 枚举

| 值 | 说明 |
|---|---|
| `"end_turn"` | 模型自然结束 |
| `"max_tokens"` | 达到输出 token 上限，回复被截断 |
| `"tool_use"` | 模型调用了工具（Agent 模式中间态，终态不会出现） |
| `null` | 未提供 |

### 2.5 Usage 对象

```json
{
  "input_tokens": 4,
  "output_tokens": 121,
  "cache_creation_input_tokens": 9369,
  "cache_read_input_tokens": 22296,
  "server_tool_use": null,
  "service_tier": "standard"
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `input_tokens` | `integer` | 输入 token 数 |
| `output_tokens` | `integer` | 输出 token 数 |
| `cache_creation_input_tokens` | `integer` \| `null` | 缓存写入 token 数 |
| `cache_read_input_tokens` | `integer` \| `null` | 缓存命中 token 数 |
| `server_tool_use` | `any` | 服务端工具用量（结构未文档化） |
| `service_tier` | `string` | 服务层级 |

### 2.6 modelUsage 对象

按模型 ID 分组的详细用量。键为模型全名，值为该模型的用量明细。

```json
{
  "claude-sonnet-4-5-20250929": {
    "inputTokens": 3,
    "outputTokens": 24,
    "cacheReadInputTokens": 20012,
    "cacheCreationInputTokens": 0,
    "costUSD": 0.010621,
    "contextWindow": 200000,
    "maxOutputTokens": 64000
  }
}
```

> 注意：`modelUsage` 内部使用 **camelCase** 命名（`inputTokens`、`costUSD`），与顶层 `usage` 的 `snake_case` 不同。

| 字段 | 类型 | 说明 |
|---|---|---|
| `inputTokens` | `integer` | 该模型输入 token |
| `outputTokens` | `integer` | 该模型输出 token |
| `cacheReadInputTokens` | `integer` | 缓存命中 token |
| `cacheCreationInputTokens` | `integer` | 缓存写入 token |
| `costUSD` | `number` | 该模型费用（USD） |
| `contextWindow` | `integer` | 上下文窗口大小 |
| `maxOutputTokens` | `integer` | 最大输出 token |

### 2.7 structured_output 字段

使用 `--json-schema` 时，结构化数据出现在此字段。此时 `result` 通常为空字符串。

```json
{
  "type": "result",
  "subtype": "success",
  "result": "",
  "structured_output": {
    "name": "John Smith",
    "age": 32,
    "city": "New York"
  }
}
```

`structured_output` 的形状完全由传入的 JSON Schema 决定。无 `--json-schema` 时此字段不存在。

---

## 3. `--output-format stream-json` 完整格式

NDJSON 格式：每行一个独立的 JSON 对象，用 `\n` 分隔。

**注意**：必须搭配 `--verbose` 才能看到 `system (init)` 事件和完整的消息结构。

### 3.1 顶层事件类型总览

每行 JSON 都有一个 `type` 字段。已知的顶层类型：

| `type` | 触发时机 | 必需标志 |
|---|---|---|
| `system` | 会话初始化、API 重试 | `--verbose`（init） |
| `assistant` | 模型生成响应后 | — |
| `user` | 工具执行结果返回后 | — |
| `stream_event` | token 级流式更新 | `--include-partial-messages` |
| `rate_limit_event` | 每次 API 调用后 | — |
| `result` | 会话结束（终态） | — |

> 还存在 `compact_boundary`、`status` 等系统子类型，出现在长会话的上下文压缩阶段。完整子类型列表见 §3.8。

---

### 3.2 `system` 事件

#### 3.2.1 `subtype: "init"` — 会话初始化

流的第一个事件。包含会话元数据。

```json
{
  "type": "system",
  "subtype": "init",
  "cwd": "/home/ubuntu/project",
  "session_id": "380bd0cd-2017-414d-b3c3-2101041c4d3b",
  "tools": [
    "Task",
    "Bash",
    "Glob",
    "Grep",
    "Read",
    "Edit",
    "MultiEdit",
    "Write",
    "WebFetch",
    "WebSearch",
    "TodoRead",
    "TodoWrite",
    "mcp__puppeteer__puppeteer_navigate"
  ],
  "mcp_servers": [
    {
      "name": "puppeteer",
      "status": "connected"
    },
    {
      "name": "claude.ai Gmail",
      "status": "needs-auth"
    }
  ],
  "model": "claude-opus-4-20250514",
  "permissionMode": "default",
  "apiKeySource": "none",
  "claude_code_version": "2.1.74",
  "agents": ["dev", "explore"],
  "skills": ["debug", "simplify"],
  "plugins": [],
  "fast_mode_state": "off"
}
```

| 字段 | 类型 | 必定存在 | 说明 |
|---|---|---|---|
| `type` | `string` | 是 | 固定 `"system"` |
| `subtype` | `string` | 是 | `"init"` |
| `session_id` | `string` | 是 | 会话 UUID |
| `cwd` | `string` | 是 | 当前工作目录 |
| `tools` | `string[]` | 是 | 已注册的工具名称列表（包含 MCP 工具）。注意：这是注册清单，不等于运行时权限允许的工具 |
| `mcp_servers` | `object[]` | 是 | MCP 服务器状态列表 |
| `model` | `string` | 是 | 使用的模型全名 |
| `permissionMode` | `string` | 是 | 权限模式（`"default"` / `"auto"` / `"acceptEdits"` / `"bypassPermissions"` 等） |
| `apiKeySource` | `string` | 否 | API key 来源（`"env"` / `"none"` 等） |
| `claude_code_version` | `string` | 否 | CLI 版本号 |
| `agents` | `string[]` | 否 | 已配置的 subagent 名称 |
| `skills` | `string[]` | 否 | 已加载的 skill 名称 |
| `plugins` | `array` | 否 | 已加载的插件 |
| `fast_mode_state` | `string` | 否 | 快速模式状态（`"on"` / `"off"`） |

**mcp_servers 元素**：

```json
{
  "name": "puppeteer",
  "status": "connected"
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | `string` | 服务器名称 |
| `status` | `string` | 状态：`"connected"` / `"needs-auth"` / `"disconnected"` 等 |

#### 3.2.2 `subtype: "api_retry"` — API 重试

当上游 API 请求失败、CLI 准备重试时触发。

```json
{
  "type": "system",
  "subtype": "api_retry",
  "attempt": 1,
  "max_retries": 5,
  "retry_delay_ms": 2000,
  "error_status": 429,
  "error": "rate_limit",
  "uuid": "...",
  "session_id": "..."
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `type` | `string` | 固定 `"system"` |
| `subtype` | `string` | 固定 `"api_retry"` |
| `attempt` | `integer` | 当前重试次数（从 1 开始） |
| `max_retries` | `integer` | 最大重试次数（默认 10，可通过 `CLAUDE_CODE_MAX_RETRIES` 调整） |
| `retry_delay_ms` | `integer` | 下次重试前等待时间（毫秒） |
| `error_status` | `integer` | HTTP 状态码（如 429、500、529） |
| `error` | `string` | 错误类别（见下表） |
| `uuid` | `string` | 消息 UUID |
| `session_id` | `string` | 会话 UUID |

**error 错误类别枚举**：

| 值 | 说明 |
|---|---|
| `"rate_limit"` | 请求频率限制（429） |
| `"server_error"` | 服务端错误（500、529） |
| `"authentication_failed"` | 认证失败（401） |
| `"billing_error"` | 计费问题（402） |
| `"invalid_request"` | 无效请求（400） |
| `"max_output_tokens"` | 输出超长 |
| `"unknown"` | 未知错误 |

---

### 3.3 `assistant` 事件

模型完成一轮响应后触发（非流式，包含完整 message）。

```json
{
  "type": "assistant",
  "session_id": "session_01",
  "uuid": "...",
  "message": {
    "id": "msg_1",
    "type": "message",
    "role": "assistant",
    "model": "claude-sonnet-4-5-20250929",
    "content": [
      { "type": "text", "text": "Planning next steps." }
    ],
    "usage": {
      "input_tokens": 120,
      "output_tokens": 45
    },
    "stop_reason": "end_turn"
  }
}
```

带工具调用的示例：

```json
{
  "type": "assistant",
  "session_id": "session_01",
  "uuid": "...",
  "message": {
    "id": "msg_2",
    "type": "message",
    "role": "assistant",
    "content": [
      { "type": "text", "text": "Let me check the files." },
      {
        "type": "tool_use",
        "id": "toolu_01ABC",
        "name": "Bash",
        "input": { "command": "ls -la" }
      }
    ],
    "stop_reason": "tool_use"
  }
}
```

带错误的示例：

```json
{
  "type": "assistant",
  "session_id": "session_01",
  "uuid": "...",
  "error": "rate_limit",
  "message": {
    "id": "msg_err",
    "type": "message",
    "role": "assistant",
    "content": [
      { "type": "text", "text": "API Error: Request rejected (429) ..." }
    ]
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `type` | `string` | 固定 `"assistant"` |
| `session_id` | `string` | 会话 UUID |
| `uuid` | `string` | 消息 UUID |
| `message` | `object` | 完整的 Anthropic Message 对象（见 §4） |
| `error` | `string` \| `null` | 错误类别（同 §3.2.2 的 error 枚举），无错误时不存在 |
| `parent_tool_use_id` | `string` \| `null` | 父级工具调用 ID（subagent 场景） |

---

### 3.4 `user` 事件

工具执行完成、结果返回给模型时触发。

字符串内容示例：

```json
{
  "type": "user",
  "session_id": "session_01",
  "uuid": "...",
  "parent_tool_use_id": "toolu_01ABC",
  "message": {
    "id": "msg_3",
    "type": "message",
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_01ABC",
        "content": "README.md\nsrc/\nCargo.toml\n"
      }
    ]
  }
}
```

数组内容示例（Task 工具格式）：

```json
{
  "type": "user",
  "session_id": "session_01",
  "uuid": "...",
  "message": {
    "id": "msg_4",
    "type": "message",
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_02DEF",
        "content": [
          { "type": "text", "text": "Task completed successfully." }
        ]
      }
    ]
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `type` | `string` | 固定 `"user"` |
| `session_id` | `string` | 会话 UUID |
| `uuid` | `string` | 消息 UUID |
| `message` | `object` | 完整的 Anthropic Message 对象（见 §4） |
| `parent_tool_use_id` | `string` \| `null` | 关联的工具调用 ID |

---

### 3.5 `stream_event` 事件

token 级别的流式更新。需要 `--include-partial-messages` 标志。

```json
{
  "type": "stream_event",
  "session_id": "...",
  "parent_tool_use_id": null,
  "uuid": "...",
  "event": {
    "type": "content_block_delta",
    "index": 0,
    "delta": {
      "type": "text_delta",
      "text": "Hello"
    }
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `type` | `string` | 固定 `"stream_event"` |
| `session_id` | `string` | 会话 UUID |
| `parent_tool_use_id` | `string` \| `null` | 父级工具调用 ID（subagent 场景） |
| `uuid` | `string` | 事件 UUID |
| `event` | `object` | 内层 API 流式事件（见 §3.5.1） |

#### 3.5.1 内层 event 类型

`event` 字段是一个对象，自身有 `type` 字段：

| `event.type` | 说明 | 关键字段 |
|---|---|---|
| `message_start` | 新消息开始 | `message`（含 `id`、`model`、`usage`） |
| `content_block_start` | 新内容块开始 | `index`、`content_block` |
| `content_block_delta` | 内容增量（最常见） | `index`、`delta` |
| `content_block_stop` | 内容块结束 | `index` |
| `message_delta` | 消息级更新 | `delta`（含 `stop_reason`）、`usage` |
| `message_stop` | 消息结束 | — |

#### 3.5.2 content_block_start

```json
{
  "type": "content_block_start",
  "index": 0,
  "content_block": {
    "type": "text",
    "text": ""
  }
}
```

工具调用开始：

```json
{
  "type": "content_block_start",
  "index": 1,
  "content_block": {
    "type": "tool_use",
    "id": "toolu_01ABC",
    "name": "Bash",
    "input": {}
  }
}
```

#### 3.5.3 content_block_delta

`delta` 字段本身也是一个带 `type` 的对象：

**文本增量**：

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "text_delta",
    "text": "Hello, "
  }
}
```

**工具输入增量**：

```json
{
  "type": "content_block_delta",
  "index": 1,
  "delta": {
    "type": "input_json_delta",
    "partial_json": "{\"command\": \"ls"
  }
}
```

**思考增量**（扩展思考模式）：

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "thinking_delta",
    "thinking": "Let me analyze..."
  }
}
```

**签名增量**（扩展思考的签名块）：

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "signature_delta",
    "signature": "EqoBCkA..."
  }
}
```

**引用增量**：

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "citation_delta",
    "citation": { ... }
  }
}
```

#### 3.5.4 message_delta

```json
{
  "type": "message_delta",
  "delta": {
    "stop_reason": "end_turn",
    "stop_sequence": null
  },
  "usage": {
    "output_tokens": 45
  }
}
```

---

### 3.6 `rate_limit_event` 事件

每次 API 调用后触发（即使未被限流也会触发），提供限流状态信息。

完整示例：

```json
{
  "type": "rate_limit_event",
  "rate_limit_info": {
    "status": "allowed_warning",
    "resetsAt": 1771390800,
    "utilization": 0.85,
    "rateLimitType": "five_hour",
    "overageStatus": "rejected",
    "overageDisabledReason": "org_level_disabled",
    "overageResetsAt": 1771394400,
    "isUsingOverage": false,
    "surpassedThreshold": 0.8
  },
  "uuid": "...",
  "session_id": "..."
}
```

最小示例（纯信息性）：

```json
{
  "type": "rate_limit_event"
}
```

#### rate_limit_info 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `status` | `string` | 当前状态：`"allowed"` / `"allowed_warning"` / `"rejected"` |
| `resetsAt` | `integer` | 限流重置时间（Unix 时间戳，秒或毫秒，取决于版本） |
| `utilization` | `number` | 当前使用率，0.0–1.0 |
| `rateLimitType` | `string` | 限流窗口类型：`"five_hour"` / `"seven_day"` / `"overage"` |
| `overageStatus` | `string` | 超额状态：`"allowed"` / `"allowed_warning"` / `"rejected"` |
| `overageDisabledReason` | `string` | 超额禁用原因（如 `"org_level_disabled"`） |
| `overageResetsAt` | `integer` | 超额重置时间 |
| `isUsingOverage` | `boolean` | 是否正在使用超额额度 |
| `surpassedThreshold` | `number` | 超过的阈值 |

> 注意：`rate_limit_info` 使用 **camelCase** 字段名。

---

### 3.7 `result` 事件

流的最后一个事件。格式与 `--output-format json` 的完整输出完全相同（见 §2）。

```json
{
  "type": "result",
  "subtype": "success",
  "is_error": false,
  "result": "The final answer...",
  "session_id": "...",
  "total_cost_usd": 0.003,
  "duration_ms": 1234,
  "duration_api_ms": 800,
  "num_turns": 1,
  "stop_reason": "end_turn",
  "usage": { ... },
  "uuid": "..."
}
```

---

### 3.8 其他已知系统子类型

除 `init` 和 `api_retry` 外，`system` 事件还可能包含以下子类型：

| `subtype` | 触发时机 | 关键字段 |
|---|---|---|
| `init` | 会话开始 | 见 §3.2.1 |
| `api_retry` | API 重试 | 见 §3.2.2 |
| `compact_boundary` | 上下文压缩完成 | `compacted_tokens`、`remaining_tokens` |
| `status` | 后台操作状态 | `status`、`message` |

---

## 4. Message 对象

`assistant` 和 `user` 事件中的 `message` 字段遵循 Anthropic Messages API 的 Message 格式。

### 4.1 Message 结构

```json
{
  "id": "msg_01XYZ",
  "type": "message",
  "role": "assistant",
  "model": "claude-sonnet-4-5-20250929",
  "content": [ ... ],
  "usage": {
    "input_tokens": 120,
    "output_tokens": 45
  },
  "stop_reason": "end_turn",
  "stop_sequence": null
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 消息 ID（`msg_` 前缀） |
| `type` | `string` | 固定 `"message"` |
| `role` | `string` | `"assistant"` 或 `"user"` |
| `model` | `string` | 模型名（assistant 消息） |
| `content` | `array` | 内容块数组（见 §4.2） |
| `usage` | `object` | token 用量（assistant 消息） |
| `stop_reason` | `string` | 停止原因 |
| `stop_sequence` | `any` | 停止序列 |

### 4.2 ContentBlock 类型

`content` 数组中的每个元素都有一个 `type` 字段：

#### 4.2.1 text — 文本内容

```json
{
  "type": "text",
  "text": "Hello! How can I help you?"
}
```

#### 4.2.2 tool_use — 工具调用

```json
{
  "type": "tool_use",
  "id": "toolu_01ABC",
  "name": "Bash",
  "input": {
    "command": "ls -la"
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 工具调用 ID（`toolu_` 前缀） |
| `name` | `string` | 工具名称 |
| `input` | `object` | 工具输入参数 |

#### 4.2.3 tool_result — 工具结果

字符串内容：

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01ABC",
  "content": "README.md\nsrc/\n"
}
```

数组内容：

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_02DEF",
  "content": [
    { "type": "text", "text": "Task completed successfully." }
  ]
}
```

带错误：

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_03GHI",
  "is_error": true,
  "content": "Command failed with exit code 1"
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `tool_use_id` | `string` | 对应的 `tool_use` 的 `id` |
| `content` | `string` \| `array` | 结果内容，可能是字符串或内容块数组 |
| `is_error` | `boolean` | 是否为错误结果 |

> **重要**：`content` 字段是多态的 — 可能是 `string` 也可能是 `ContentBlock[]`。解析时需要检查类型。

#### 4.2.4 thinking — 扩展思考

```json
{
  "type": "thinking",
  "thinking": "Let me reason through this step by step..."
}
```

#### 4.2.5 image — 图片内容

```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/png",
    "data": "iVBOR..."
  }
}
```

#### 4.2.6 document — 文档内容

```json
{
  "type": "document",
  "source": {
    "type": "base64",
    "media_type": "application/pdf",
    "data": "..."
  }
}
```

---

## 5. 事件流时序

### 5.1 简单文本回复（无工具）

```
1. {"type":"system","subtype":"init",...}            ← --verbose 时出现
2. {"type":"stream_event","event":{"type":"message_start",...}}    ← --include-partial-messages
3. {"type":"stream_event","event":{"type":"content_block_start",...}}
4. {"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}}
   ... (多次 delta)
5. {"type":"stream_event","event":{"type":"content_block_stop",...}}
6. {"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},...}}
7. {"type":"stream_event","event":{"type":"message_stop"}}
8. {"type":"rate_limit_event",...}
9. {"type":"result","subtype":"success",...}          ← 终态
```

### 5.2 带工具调用的回复

```
1. {"type":"system","subtype":"init",...}
2. {"type":"assistant","message":{"content":[
     {"type":"text","text":"Let me check."},
     {"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"ls"}}
   ],"stop_reason":"tool_use"}}
3. {"type":"user","message":{"content":[
     {"type":"tool_result","tool_use_id":"toolu_01","content":"README.md\nsrc/"}
   ]}}
4. {"type":"assistant","message":{"content":[
     {"type":"text","text":"Here's what I found..."}
   ],"stop_reason":"end_turn"}}
5. {"type":"result","subtype":"success",...}
```

### 5.3 API 重试

```
1. {"type":"system","subtype":"init",...}
2. {"type":"system","subtype":"api_retry","attempt":1,"max_retries":5,"error":"rate_limit",...}
   ... (可能多次)
3. {"type":"assistant",...}
4. {"type":"result","subtype":"success",...}
```

### 5.4 多轮 Agent（多次工具调用）

```
1. system (init)
2. assistant → tool_use (Read)
3. user → tool_result
4. assistant → tool_use (Edit)
5. user → tool_result
6. assistant → tool_use (Bash)
7. user → tool_result
8. assistant → text (最终回复)
9. result (success)
```

---

## 6. 输入协议（`--input-format stream-json`）

> **注意**：Loom 不使用此输入协议。Loom 的 Agent 输入通过 Loom 自身的接口处理，仅使用 Claude Code JSON schema 作为**输出**格式。此节仅作文档参考保留。

使用 `--input-format stream-json` 时，CLI 从 stdin 接收 NDJSON 消息。

### 6.1 用户消息

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": "What is 2 + 2?"
  },
  "session_id": "default",
  "parent_tool_use_id": null
}
```

### 6.2 工具审批（approve）

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_pending_01",
        "content": "approved"
      }
    ]
  },
  "session_id": "default",
  "parent_tool_use_id": null
}
```

### 6.3 工具拒绝（deny）

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_pending_01",
        "content": "denied",
        "is_error": true
      }
    ]
  },
  "session_id": "default",
  "parent_tool_use_id": null
}
```

---

## 7. 解析注意事项

### 7.1 NDJSON 解析

- 每行一个完整 JSON 对象，用 `\n` 分隔
- **不要**用 `JSON.parse` 解析整个输出
- 空行应忽略
- 某些行可能不是标准 JSON（如 stderr 混入），解析失败时应跳过

### 7.2 字段命名不一致

| 位置 | 命名风格 | 示例 |
|---|---|---|
| 顶层字段 | `snake_case` | `session_id`、`total_cost_usd` |
| `usage` 内部 | `snake_case` | `input_tokens`、`cache_read_input_tokens` |
| `modelUsage` 内部 | `camelCase` | `inputTokens`、`cacheReadInputTokens` |
| `rate_limit_info` 内部 | `camelCase` | `resetsAt`、`rateLimitType`、`isUsingOverage` |
| `system (init)` 混合 | 混合 | `permissionMode`（camelCase）、`api_key_source`（可能 snake_case） |

### 7.3 多态字段

- `tool_result.content`：可能是 `string` 或 `ContentBlock[]`
- `message.stop_sequence`：可能不存在或为 `null`
- `rate_limit_info`：可能不存在（bare 模式下 `rate_limit_event` 可能只有 `type` 字段）

### 7.4 兜底建议

1. **不要使用 `deny_unknown_fields`** — CLI 可能随时新增字段
2. **未知 `type` 值应忽略** — 新事件类型可能随时添加
3. **所有可选字段用 `Option<T>`** — 不同 CLI 版本字段可能缺失
4. **`--verbose` 和 `--include-partial-messages` 影响输出** — 没有这些标志时，部分事件不会出现

---

## 8. 版本差异

| 版本 | 变化 |
|---|---|
| v2.1.45+ | 引入 `rate_limit_event` |
| v2.1.49+ | `rate_limit_event` 频繁触发（信息性） |
| v2.1.74+ | init 事件新增 `claude_code_version`、`agents`、`skills`、`plugins` 字段 |
| v2.1.x | API 错误仍以 assistant text block 形式输出（非独立 error 事件） |

---

## 9. 快速参考：jq 过滤器

```bash
# 提取最终文本结果
claude -p "query" --output-format json | jq -r '.result'

# 提取结构化输出
claude -p "query" --output-format json --json-schema '...' | jq '.structured_output'

# 实时流式文本
claude -p "query" --output-format stream-json --verbose --include-partial-messages \
  | jq -rj 'select(.type == "stream_event" and .event.delta.type? == "text_delta") | .event.delta.text'

# 监控重试
claude -p "query" --output-format stream-json \
  | jq 'select(.type == "system" and .subtype == "api_retry") | {attempt, max_retries, error, delay_ms: .retry_delay_ms}'

# 提取所有工具调用
claude -p "query" --output-format stream-json \
  | jq 'select(.type == "assistant") | .message.content[] | select(.type == "tool_use") | {name, input}'

# 监控限流状态
claude -p "query" --output-format stream-json \
  | jq 'select(.type == "rate_limit_event") | .rate_limit_info'
```
