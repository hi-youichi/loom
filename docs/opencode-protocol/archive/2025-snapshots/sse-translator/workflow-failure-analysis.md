# Workflow Engine 失败分析

> 日期：2025-08-19
> 场景：stream-event-refactor workflow 多次运行均以 `status: failed` 终止

## 现象

Workflow `stream-event-refactor`（`.loom/workflows/stream-event-refactor.lua`）共运行 5 次，全部失败。

| # | 模型 | 耗时 | progress 事件 | agent_done status | output |
|---|---|---|---|---|---|
| 1 | `minimax-cn-coding-plan/MiniMax-M3` | 9 min | 271 | Error | null |
| 2 | 同上 | 18 s | 4 | — (未到 done) | — |
| 3 | 同上 | 17 s | 4 | — (未到 done) | — |
| 4 | 默认（无 override） | 21 s | 4 | Error | null |
| 5 | 默认（无 override） | 2.2 min | 6 | Error | null |

## 根因

### 直接原因：`structured_output` 解析失败

以第 5 次运行（`luft-workflow_1784772683`）为例，事件序列完整：

```
1. run_started
2. plan_preview
3. phase_started (A)
4. agent_started (model: minimax-cn-coding-plan/MiniMax-M3)
5. agent_progress: read(stream_event.rs)          ← agent 读了文件
6. agent_progress: read done
7. agent_progress: structured_output({changed:true, summary:"A1-A7 已完成..."})  ← agent 提交了结果
8. agent_progress: structured_output done
9. agent_done: status=Error, output=null           ← 但 output 被丢弃
10. run_done: status=failed
```

Agent 实际完成了工作并调用了 `structured_output`，但 `agent_done` 事件中：
- `status` = `"Error"`（非 `"Ok"`）
- `output` = `null`（`structured_output` 的 payload 未被捕获）
- `tokens` = `{input: 0, output: 0}`（LLM 调用的 token 用量未被记录）

### 推断的失败链路

```
agent 调用 structured_output(tool, payload)
  ↓
workflow engine 尝试验证 payload 是否匹配 schema
  ↓
验证失败 或 structured_output 的返回格式不符合引擎期望
  ↓
agent_done.status = Error, output = null
  ↓
Lua 脚本: if not a.ok → report(error) + return
  ↓
run_done: status = failed
```

### 可能的技术原因

1. **模型 API 超时**：`minimax-cn-coding-plan/MiniMax-M3` 通过非标准 provider 路由，response 格式或 streaming 超时阈值与 workflow engine 的默认 agent runner 不兼容。第一次运行 9 分钟说明 agent 确实在工作，但最终被 engine 判定为超时 error。

2. **`structured_output` tool 调用格式**：MiniMax-M3 返回的 tool call arguments 可能包含额外字段或编码差异，导致 workflow engine 的 JSON schema 验证器拒绝解析。

3. **Token 记录缺失**：所有运行 `tokens` 均为 0，说明 workflow engine 未能从 MiniMax provider 的 response 中提取 usage 数据。这可能是一个更底层的 provider 集成问题——engine 在 `structured_output` 提交时需要 token 数据来标记 checkpoint，缺失时进入 error 路径。

4. **默认模型更不稳定**：未指定 model 时（运行 #4、#5），agent 仅存活 20s-2min 即断连，说明默认模型的 API 端点同样存在稳定性问题。

## 证据

### 代码改动已落地

第 1 次运行后，`cargo check -p stream-event` 通过，输出中包含：

```
warning: use of deprecated variant `types::stream_event::StreamEvent::Messages`: use TextDelta or ReasoningDelta
```

证明 Phase A（枚举改造 + `Messages` 标记 `#[deprecated]`）的代码改动已成功写入磁盘。

### 第 5 次运行的 structured_output payload

```json
{
  "changed": true,
  "files": ["foundation/stream-event/src/types/stream_event.rs"],
  "summary": "A1-A7 已完成：Messages 已替换为 TextDelta/ReasoningDelta，新增文本与推理 block 生命周期、T..."
}
```

Payload 结构完全符合 workflow 中定义的 `SCHEMA`（`changed: boolean`, `files: string[]`, `summary: string`），说明问题不在 schema 验证。

## 影响范围

- **代码**：Phase A 改动已落地且编译通过；Phase B-G 尚未执行
- **Workflow**：所有后续 phase 无法启动（因为 Lua 脚本在 `if not a.ok` 时 `report(error) + return`）
- **Resume**：`workflow_start(resume_from_id)` 不可用（status=Failed 的 instance 不支持 resume）

## 建议的修复方向

### 短期（绕过 workflow engine）

- 改用 `agent` 工具逐 phase 手动执行，跳过 workflow engine 的 `structured_output` 解析层
- 每个 phase 完成后手动检查 `cargo check`，确认改动落地后再进入下一 phase

### 中期（修复 workflow engine）

1. **调查 token 记录**：检查 workflow engine 是否因 `tokens = 0` 而将 agent 标记为 error。相关代码在 `agent/tool/tool-workflow/src/` 下的 agent runner 模块。
2. **增加 structured_output 容错**：即使 token 解析失败，也应保留 `structured_output` 的 payload 作为 `output`，而不是设为 `null`。
3. **provider 适配**：确认 MiniMax provider 的 response 中 usage 字段的路径和格式，与 workflow engine 的解析逻辑匹配。

### 长期

- 在 workflow engine 中增加 `structured_output` 的日志记录（payload + 验证结果），便于后续诊断
- 增加 agent timeout 配置项（当前默认 timeout 可能过短或不适用于 MiniMax provider）
- 对非标准 provider 增加 integration test（验证 token 解析 + structured_output round-trip）
