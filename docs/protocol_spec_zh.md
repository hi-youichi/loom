# 流式输出协议规范（Protocol Spec）

本规范定义 Loom Agent 运行的**流式输出**数据格式与语义。发送方（服务端/CLI）与接收方（客户端/前端）**必须**按本协议生成与解析数据，以便多会话、多轮次及按节点执行的结果能正确合并与渲染。

**传输形式**：本规范定义**每条消息（帧）的 JSON 结构**；不限定底层传输方式（如 stdout NDJSON、WebSocket 文本帧、HTTP 分块）。每行或每帧须为单个完整 JSON 对象。

---

## 1. 范围与术语

### 1.1 范围

- **在范围内**：Agent 运行期间的流式事件及该运行的最终回复。
- **不在范围内**：握手、认证、连接管理及非流式请求/响应格式（由其他文档或实现规定）。

### 1.2 术语

| 术语 | 含义 |
|------|------|
| **会话（Session）** | 一次连续对话，通常由 thread_id 或连接标识。 |
| **节点运行（Node run）** | 从节点进入（node_enter）到节点退出（node_exit）的一次执行；该区间内所有事件共享同一 node_id。 |
| **事件（Event）** | 流中的一条结构化消息，类型包括 node_enter、node_exit、message_chunk、usage、values、updates 等（见 §4）。 |
| **回复（Reply）** | 当前运行的完整助手回复；流中与该运行对应的最后一条消息。 |

---

## 2. 标识符（信封字段）

每条消息（事件行或回复行）的 JSON 对象**可以**在顶层携带下列信封字段；实现**建议**包含这些字段，以便在会话与轮次间可靠合并。

| 字段 | 类型 | 必填 | 含义与约束 |
|------|------|------|------------|
| **session_id** | string | 否 | 会话 ID。在同一会话内保持不变。 |
| **node_id** | string | 否 | 节点运行 ID。从 node_enter 到 node_exit 为一个区间；该区间内所有行共享同一 node_id。**node_id 可能重复**（跨区间或跨运行）；接收方用 event_id 或行序区分。 |
| **event_id** | number | 否 | 每条消息的序号。在流内单调递增；每条消息唯一；用于排序、去重与引用。 |

在事件体内，**id** 表示节点名称（如 "think"、"act"），与信封中的 **node_id** 不同；信封的 node_id 可能重复，而 body 中的 id 是节点类型名。

---

## 3. 传输与编码

- **编码**：UTF-8。
- **帧边界**：每帧一个完整 JSON 对象。对按行传输（如 NDJSON），每行以 `\n` 结尾；行内不得包含未转义换行符。
- **顺序**：帧顺序与事件顺序一致；event_id（若存在）单调递增。

---

## 4. 消息结构：事件

### 4.1 通用规则

- 事件消息**必须**在顶层包含 **type**（string），表示事件种类。
- 除 type 外，其余字段为载荷；与信封字段（session_id、node_id、event_id）同级。
- **事件**包括 run_start、node_enter、node_exit、message_chunk、usage、values、updates、custom、checkpoint、ToT/GoT 相关类型，以及 tool 相关类型（tool_call_chunk、tool_call、tool_start、tool_output、tool_end、tool_approval）（见 §4.2）。

### 4.2 事件类型与载荷

下表列出所有事件类型及其载荷字段（不含 type）。`state` 表示图状态 JSON（结构依 Agent 类型而定）。载荷中的 **id** 字段表示节点名称；与信封的 node_id 不同。

| type | 描述 | 载荷字段（除 type 外） |
|------|------|------------------------|
| **run_start** | Agent 运行开始（在首个 node_enter 之前） | `run_id`: string（可选），`message`: string（可选，用户消息），`agent`: string（可选，如 "react"、"tot"、"got"） |
| **node_enter** | 节点运行开始 | `id`: string（节点名，如 "think"、"act"） |
| **node_exit** | 节点运行结束 | `id`: string（节点名），`result`: "Ok" 或 `{"Err": string}` |
| **message_chunk** | LLM 消息块 | `content`: string，`id`: string（产生该块的节点名） |
| **usage** | Token 用量 | `prompt_tokens`: number，`completion_tokens`: number，`total_tokens`: number |
| **values** | 完整状态快照 | `state`: state |
| **updates** | 节点合并后的状态 | `id`: string（节点名），`state`: state |
| **custom** | 自定义 JSON | `value`: 任意 JSON |
| **checkpoint** | 检查点 | `checkpoint_id`、`timestamp`、`step`、`state`、`thread_id`、`checkpoint_ns` |
| **tot_expand** | ToT 扩展 | `candidates`: [ string, ... ] |
| **tot_evaluate** | ToT 评估 | `chosen`: number，`scores`: [ number, ... ] |
| **tot_backtrack** | ToT 回溯 | `reason`: string，`to_depth`: number |
| **got_plan** | GoT 计划 | `node_count`、`edge_count`、`node_ids` |
| **got_node_start** | GoT 节点开始 | `id`: string |
| **got_node_complete** | GoT 节点完成 | `id`: string，`result_summary`: string |
| **got_node_failed** | GoT 节点失败 | `id`: string，`error`: string |
| **got_expand** | AGoT 扩展 | `node_id`: string，`nodes_added`，`edges_added` |
| **tool_call_chunk** | 工具调用参数流式增量（如流式 JSON） | `call_id`: string（可选），`name`: string（可选），`arguments_delta`: string |
| **tool_call** | 完整工具调用：名称与完整参数 | `call_id`: string（可选），`name`: string，`arguments`: object |
| **tool_start** | 工具执行开始 | `call_id`: string（可选），`name`: string |
| **tool_output** | 工具产生的内容（如 stdout）；每次调用可发送多次 | `call_id`: string（可选），`name`: string，`content`: string |
| **tool_end** | 工具执行结束 | `call_id`: string（可选），`name`: string，`result`: string，`is_error`: boolean |
| **tool_approval** | 待用户确认的工具调用（如危险操作） | `call_id`: string（可选），`name`: string，`arguments`: object |

单次工具调用的典型顺序：**tool_call**（或 **tool_call_chunk** 流）→ **tool_start** → **tool_output**（零次或多次）→ **tool_end**；或需要客户端先确认时为 **tool_approval**。

### 4.3 示例事件消息（含信封）

```json
{"session_id":"sess-001","event_id":0,"type":"run_start","run_id":"run-1","message":"Hello","agent":"react"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":1,"type":"node_enter","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":2,"type":"message_chunk","content":"I","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":3,"type":"message_chunk","content":" don't","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":4,"type":"usage","prompt_tokens":100,"completion_tokens":62,"total_tokens":162}
{"session_id":"sess-001","node_id":"run-think-1","event_id":5,"type":"node_exit","id":"think","result":"Ok"}
```

不带信封时，事件消息**可以**仅包含 type 与载荷：

```json
{"type":"run_start","run_id":"run-1","agent":"react"}
{"type":"node_enter","id":"think"}
{"type":"message_chunk","content":"Hello","id":"think"}
{"type":"node_exit","id":"think","result":"Ok"}
```

---

## 5. 消息结构：回复

当前运行的完整助手回复由一条**回复消息**表示，通常为该运行在流中的最后一条消息。

- **必须**包含顶层字段 **reply**（string），即完整回复文本。
- **建议**：同时包含 session_id、node_id、event_id（与该运行的事件一致；event_id 延续同一序列）。

示例：

```json
{"session_id":"sess-001","node_id":"run-think-1","event_id":8,"reply":"I don't have access to your device's clock ..."}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| reply | string | 是 | 本运行的完整助手回复。 |
| session_id | string | 否 | 与本运行事件相同。 |
| node_id | string | 否 | 产生本回复的节点运行的 node_id。 |
| event_id | number | 否 | 与事件消息同一单调序列。 |

---

## 6. 与 EXPORT_SPEC 的对应关系

本协议采用统一的 **type + payload** 结构。与 [EXPORT_SPEC.md](./EXPORT_SPEC.md) 中「单键事件」形式的对应如下。语义一致，仅序列化形式不同。

| 本规范 type | EXPORT_SPEC 键 |
|-------------|----------------|
| run_start | RunStart |
| node_enter | TaskStart |
| node_exit | TaskEnd |
| message_chunk | Messages |
| usage | Usage |
| values | Values |
| updates | Updates |
| custom | Custom |
| checkpoint | Checkpoint |
| tot_expand | TotExpand |
| tot_evaluate | TotEvaluate |
| tot_backtrack | TotBacktrack |
| got_plan | GotPlan |
| got_node_start | GotNodeStart |
| got_node_complete | GotNodeComplete |
| got_node_failed | GotNodeFailed |
| got_expand | GotExpand |
| tool_call_chunk | ToolCallChunk |
| tool_call | ToolCall |
| tool_start | ToolStart |
| tool_output | ToolOutput |
| tool_end | ToolEnd |
| tool_approval | ToolApproval |

---

## 7. 实现要求

### 7.1 发送方

- **必须**：对事件消息使用 §4.2 定义的 type 与 payload；回复消息必须包含 `reply` 字段。
- **建议**：在每帧上包含 session_id、node_id、event_id；event_id 在流内从 1 起单调递增。
- **node_id**：在每次 node_enter 时设置，并在对应 node_exit 之前的区间内以及该运行的回复中共享；node_id 可能重复；用 event_id 或顺序区分区间。

### 7.2 接收方

- **必须**：解析 type 与 payload，并支持仅含 type + payload（无信封）的消息。
- **建议**：支持无 event_id 的流；在多会话/多轮次场景下，按 session_id、node_id 和 event_id（或行序）合并。当 node_id 重复时，用 event_id 或连续的 node_enter…node_exit 区间区分。

### 7.3 兼容性

- 接收方**应当**接受仅含 type + payload 的简化输出，以及无 event_id 的旧版流。
- 发送方**建议**发出完整信封，以便更好地进行多轮与多会话渲染。

---

## 8. 版本与扩展

- 信封使用 **node_id** 表示节点运行 ID（可能重复）；事件体使用 **id** 表示节点名称（got_expand 中为 node_id）。
- 新增事件类型时，增加新的 type 与 payload 定义，并保持「每帧一个 type」的约定。
- 本规范与 EXPORT_SPEC 语义一致，作为流式输出的规范格式。
