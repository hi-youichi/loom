# Loom Server SSE 集成测试方案

状态：实施中。更新日期：2026-07-24。目标协议：OpenCode `b8142c7aa` 的 `Event.Payload`、`SessionEvent.Durable` 与 `SessionEvent.All`。

## 实施进度（2026-07-24）

已新增 `apps/server/tests/sse_v2_integration.rs`：通过实际 Axum SSE response 校验全部 32 个 `session.next.*` type 的 envelope、durable/live 分流、durable version、无 `null` optional 字段及显式 required/optional 字段表；同时已覆盖 session replay/cursor/隔离、synthetic→compact→revert handler 链路、loopback TCP `/api/event` frame，以及 `MockLlm` 经 `agent_runner → translator → SSE` 的 text turn。

尚未达到本文定义的完成判据：持久化重启/append-failure、broadcast lag/reconnect、legacy keepalive，以及 tool/failure/cancel mock 场景仍需补齐。另：当前 `MockLlm` 的 agent-runner 实测只回放 `step.started`，未回放 terminal `step.ended`；这是需在 production event path 修正的缺口，不能通过测试伪造 terminal event。完成前不得报告为 100% 结构覆盖。

本文定义 **仅针对 Loom server** 的 SSE 自动化测试：通过真实 Axum `Router` 或绑定 loopback TCP 的 server 读取原始 SSE bytes。所有 agent 路径均注入 Loom 自身的 `MockLlm`/`MultiRoundMockLlm`；不启动 OpenCode、OpenChamber、不调用 SDK、不访问网络 Provider，也不要求 API key。它不以 translator 单测、`V2Event` 序列化单测或“连接返回 200”代替端到端协议验证。

## 1. 目标与非目标

目标：在 CI 中验证 legacy `/global/event` 与 v2 `/api/event`、`/api/session/:id/event` 的路由、SSE frame、durability、cursor、重放/订阅竞态、跨 session 隔离和持久化恢复。

### 100% SSE 返回结构覆盖的定义

本方案的 **100%** 是返回结构覆盖率，不是 Rust 行/分支覆盖率。覆盖集合 `U` 固定为：

1. 3 个 SSE endpoint 的成功 response headers 与首帧/业务帧/keepalive comment frame；
2. 4 类 frame 外壳：legacy wrapper、v2 durable、v2 live-only、HTTP 400 invalid cursor；
3. 全部 32 个 `session.next.*` type 的实际 `data:` JSON；
4. 每 type 的 required field、optional field 的 absent/present 两个分支、nested union（`ToolContent`、error、model、tokens、revert）的每个 variant；
5. session stream 的 replay、live、reconnect、跨 session、lag、delete/restart 结果结构。

令 `covered` 是在 CI 中由 TCP SSE reader 实际读到、再经严格 validator 验证成功的结构 ID 数；只有 `covered == U`（100%）才允许通过。fixture 只定义期待值，**不能**单独计入 `covered`；每个 fixture 都必须有一个 Loom handler/MockLlm 场景产生相同 type 的真实 frame。测试生成 `sse-structure-coverage.json`，其中列出 `U`、covered、缺失 ID 与百分比；任何百分比低于 100 或有 `#[ignore]` 的结构用例都使 gate 失败。

非目标：真实模型内容质量、OpenChamber UI 渲染和外部 Provider 的网络稳定性。任何外部系统联调均不属于这套集成测试的通过条件。

## 2. 测试目录与基架

新增 `apps/server/tests/sse_v2_integration.rs`，不要继续向 `protocol.rs` 的旧 session SSE 测试追加断言。旧测试 `session_event_endpoint_is_sse_and_replays_only_that_session` 使用 legacy event-id cursor 并期望 `server.connected`，应替换而不是迁移。

### 2.1 双层执行模式

| 层 | 启动方式 | 覆盖范围 | CI |
| --- | --- | --- | --- |
| Router integration | `build_router(new_state())` + `oneshot` | 状态码、headers、replay payload、非法 query、跨 session | 每次 PR |
| TCP wire integration | `TcpListener::bind("127.0.0.1:0")` + `axum::serve` + `reqwest`/raw TCP | chunk framing、`event: message`、keepalive、live subscription、断线重连 | 每次 PR |
| process persistence | Loom 子进程 `loom-server serve`，专属 `LOOM_HOME` | restart 后 JSONL load、seq 延续、delete 清理 | 每次 PR（Windows/Linux） |
| mock agent E2E | Loom router + `MockLlm`/`MultiRoundMockLlm` | prompt→agent runner→translator→SSE，含 text/reasoning/tool/failure/cancel | 每次 PR |

Router 层不能安全验证无限 stream 的 frame 边界；TCP 层不能用固定端口，也不能依赖 `sleep`。所有等待使用 `tokio::time::timeout`。

### 2.2 公共 helper

```rust
struct TestServer { base: Url, shutdown: oneshot::Sender<()> }
async fn spawn_server(state: SharedState) -> TestServer;
async fn open_sse(base: &Url, path: &str) -> SseReader;
async fn next_frame(reader: &mut SseReader) -> ParsedSseFrame;
fn assert_v2_durable(frame: &ParsedSseFrame, ty: &str, session: &str, seq: u64, version: u32);
```

`SseReader` 必须按空行分割事件、收集多行 `data:`、忽略 comment line，不能假设一次 network chunk 等于一个 SSE event。`ParsedSseFrame` 至少包含 `event_name: Option<String>`、`data: String`、`comments: Vec<String>`。

测试环境中的 `LOOM_HOME` 必须是唯一临时目录。因为环境变量为进程全局，涉及 file-log 的测试需要同一全局 mutex，或改为 `V2FileLog::open(tempdir)` 的直接测试；并行测试不得共享该目录。

## 3. 端点契约用例

### 3.1 Legacy global stream

| ID | 请求/前置 | 断言 |
| --- | --- | --- |
| L1 | `GET /global/event` | 200、`text/event-stream`、首帧 legacy `{directory,payload}`，含 `server.connected`。 |
| L2 | 连接后 `state::emit("message.updated", ...)` | payload type/data 使用 legacy wrapper；不出现 v2 durable。 |
| L3 | 等待略大于 keepalive interval | 收到 `server.heartbeat` business event 与 SSE comment keepalive；两者分别断言。 |

### 3.2 V2 global stream

| ID | 请求/前置 | 断言 |
| --- | --- | --- |
| G1 | `GET /api/event`，发布 durable event | 200、`text/event-stream`、`event: message`、data 为 flat `V2Event`。 |
| G2 | 发布 `text.delta` | event 没有 `durable`，但 data 有 session/message/block ID 和 delta。 |
| G3 | 发布 durable event | `id` 以 `evt_` 开头，`durable.aggregateID == data.sessionID`。 |

### 3.3 V2 session stream

| ID | 前置 | 断言 |
| --- | --- | --- |
| S1 | `after` 缺失 | 仅该 session 的 durable history，严格 seq 递增，`event: message`。 |
| S2 | `after=1`，已有 seq 1..3 | 只收到 2、3；不出现 seq 1。 |
| S3 | `after=0`、`after=-1`、`after=abc`、`after=u64+1` | 缺失=0；其余均 400。 |
| S4 | A/B 各写 durable event | A stream 不含 B 的 data/aggregate ID；global stream 可见二者。 |
| S5 | session stream 已订阅，随后 publish | 新 event 立即收到一次。 |
| S6 | subscribe/replay 边界发布 event | 每个 seq 恰好一次、无漏号；覆盖 watermark 算法。 |
| S7 | 对 session stream 发 live delta | 永不出现；同一 delta 在 `/api/event` 出现。 |
| S8 | 空闲 11 秒 | 只允许 comment keepalive；禁止 JSON `server.connected`/`server.heartbeat`。 |
| S9 | broadcast receiver lag | 断线后以最后 seq reconnect，store replay 连续、不重复。 |

## 4. Durable log 与进程恢复

| ID | 操作 | 断言 |
| --- | --- | --- |
| P1 | 发布 1..3 后新建 state/load | history/SSE 为 1..3，下一 publish 为 4。 |
| P2 | A/B 并发各 publish N 条 | 每个 aggregate 独立连续 `1..N`。 |
| P3 | JSONL 尾部写入非法行 | 启动记录明确错误并拒绝/隔离该 log；不能静默错序 replay。 |
| P4 | 删除 session 后重启 | JSONL 不存在，history 与 SSE 不返回旧 event。 |
| P5 | 模拟 append 失败 | event 不进入 memory log、不 broadcast、不推进 sequence。 |

P5 需要为 file writer 注入 trait 或 test-only failing writer；不要用目录权限等平台相关技巧。

## 5. SSE 返回结构的全字段 schema gate

新增 `apps/server/tests/fixtures/v2-session-events/`：每个 `session.next.*` 一个 JSON fixture，共 32 个。fixture 必须是 wire envelope，不是内部 input。

每个 fixture 与每个实际 SSE `data:` JSON 均执行以下全字段断言；禁止只断言 `type` 或少数字段：

1. `id/type/data` 存在；`timestamp` 为非负整数；`data.sessionID` 存在。
2. durable fixture 有 `{aggregateID,seq,version}`，aggregate 等于 sessionID；4 个 delta 不得有 durable。
3. `step.ended`、`step.failed` version=2，其余 durable version=1。
4. optional 字段缺失而不是 `null`。
5. `serde_json` parse 后执行 Loom test crate 内的 **严格 structural validator**：拒绝未知的 required type、错误 JSON kind、缺 required field、错误 field casing、optional field 为 `null`、live/durable 混用及错误 durable version。
6. validator 对每个 type 使用显式字段表，不使用“任意 object”或 `Value::is_object()` 作为替代。例如 `step.ended` 必须逐个验证 `assistantMessageID/finish/cost/tokens.input/tokens.output/tokens.reasoning/tokens.cache.read/tokens.cache.write`；`tool.success` 必须验证 `structured/content/provider.executed`；`revert.staged` 必须验证 `revert.messageID`。
7. 对所有 event 检查 outer envelope 的键集合：仅允许 `id`、`metadata?`、`type`、`durable?`、`location?`、`data`；验证 `event: message` SSE name，禁止在 session stream 出现 JSON heartbeat 或 legacy `payload` wrapper。

按来源覆盖的参数化测试分组：

| 组 | fixture 数 | 触发路径 |
| --- | ---: | --- |
| prompt/agent/model/moved/context/synthetic/shell/retry | 10 | session handler 请求序列 |
| step/text/reasoning | 9 | deterministic stream-event fixture runner |
| tool | 7 | deterministic ToolCall/Output/End/Error stream fixture |
| compaction/revert | 6 | compact、stage/clear/commit 路由 |
| **总计** | **32** | — |

### 5.1 每 type 的 data validator 合同

下表是测试代码的字段表；每一行对应一个 validator match arm 和至少一个 fixture。`Base` 总是指 `timestamp:number`、`sessionID:string`；`A` 指 `assistantMessageID:string`；`C` 指 `callID:string`。`D1`/`D2` 是 durable version，`L` 是 live-only（必须没有 durable）。

| type | 必须验证的 data 字段 | 模式 |
| --- | --- | --- |
| agent.switched | Base, `messageID`, `agent` | D1 |
| model.switched | Base, `messageID`, `model.id`, `model.providerID` | D1 |
| moved | Base, `location.directory` | D1 |
| prompted / prompt.admitted | Base, `messageID`, `prompt.text`, `delivery`=queue/steer | D1 |
| context.updated / synthetic | Base, `messageID`, `text` | D1 |
| shell.started | Base, `messageID`, C, `command` | D1 |
| shell.ended | Base, C, `output` | D1 |
| step.started | Base, A, `agent`, `model.id`, `model.providerID` | D1 |
| step.ended | Base, A, `finish`, finite `cost`, full `tokens` object | D2 |
| step.failed | Base, A, `error.type`=`unknown`, `error.message` | D2 |
| text.started / text.ended | Base, A, `textID`; ended 另有 `text` | D1 |
| text.delta | Base, A, `textID`, `delta` | L |
| reasoning.started / reasoning.ended | Base, A, `reasoningID`; ended 另有 `text` | D1 |
| reasoning.delta | Base, A, `reasoningID`, `delta` | L |
| tool.input.started / tool.input.ended | Base, A, C；started 有 `name`，ended 有 `text` | D1 |
| tool.input.delta | Base, A, C, `delta` | L |
| tool.called | Base, A, C, `tool`, object `input`, `provider.executed:boolean` | D1 |
| tool.progress | Base, A, C, object `structured`, `content:ToolContent[]` | D1 |
| tool.success | Base, A, C, object `structured`, `content:ToolContent[]`, `provider.executed:boolean` | D1 |
| tool.failed | Base, A, C, `error:UnknownError`, `provider.executed:boolean` | D1 |
| retried | Base, finite `attempt`, `error.message`, `error.isRetryable:boolean` | D1 |
| compaction.started | Base, `messageID`, `reason`=auto/manual | D1 |
| compaction.delta | Base, `messageID`, `text` | L |
| compaction.ended | Base, `messageID`, `reason`, `text`, `recent` | D1 |
| revert.staged | Base, `revert.messageID` | D1 |
| revert.cleared | Base | D1 |
| revert.committed | Base, `messageID` | D1 |

`ToolContent[]` 逐项验证 tagged union：`{type:"text",text:string}` 或 `{type:"file",uri:string,mime:string,name?:string}`。tokens 的五个叶子均必须为有限非负 number；metadata/location/revert 的 optional 子字段若不存在可省略，但存在时必须匹配其结构，不能为 `null`。

### 5.2 可执行的 coverage registry

`sse_v2_integration.rs` 定义单一 `STRUCTURE_REGISTRY`，每项为 `{id, source_scenario, validator, required}`。registry 必须包含 32 个 type ID 加上 `legacy.connected`、`legacy.heartbeat`、`legacy.keepalive`、`v2-global-durable`、`v2-global-live`、`session-invalid-cursor`、`session-replay`、`session-live`、`session-reconnect`、`session-isolation`、`session-restart`、`session-delete`。

测试结束时执行：

```rust
assert_eq!(covered.required_count(), STRUCTURE_REGISTRY.required_count());
assert_eq!(coverage.percent(), 100.0);
```

coverage artifact 对每项记录已验证的 outer keys、data keys、durable mode、SSE event name 和产生它的 Mock scenario。这样新增 `session.next.*` 或修改 schema 字段时，编译/CI 会因 registry 无对应测试或 validator 未消费该字段而失败。

## 6. Mock LLM 驱动的完整 agent SSE 路径

不能为测试调用真实 Provider。Loom 已有 `loom_llm::client::{MockLlm, MultiRoundMockLlm}`；为 server 测试增加 feature `test-support` 与显式注入点：

```rust
pub async fn run_agent_with_test_client(
    state: SharedState,
    request: TestPromptRequest,
    client: Arc<dyn LlmClient>,
) -> Result<RunCompletion, String>;
```

该 API 只在 `cfg(any(test, feature = "test-support"))` 导出，内部仍走生产 `agent_runner → translate_and_emit → v2_event → sse`，不得直接调用 `publish_durable()` 伪造结果。每个测试用 `MockLlm` 的确定性 response 或 `MultiRoundMockLlm` 的 response sequence 驱动：

- text → reasoning → text；多 block 与空 block；
- ToolCall → ToolOutput* → ToolEnd success；ToolError；`ToolEnd(is_error)`；
- `ProviderError` 和取消，检查 open block 收束以及 `step.failed`；
- 同一次 run 对比 legacy `message.part.updated` 与 v2 event 的双发，二者 event order 各自稳定。

该 runner 的断言从 TCP SSE reader 与 `/api/session/:id/history` 读取，不从 private state 猜测“应该已发布”。对取消/ProviderError，新增 Loom-only failing mock `LlmClient`；它在可预测的 stream position 返回错误。

### 6.1 Mock 场景矩阵

| 场景 | Mock 配置 | 必验 SSE type/结构 |
| --- | --- | --- |
| 单轮 text | `MockLlm::with_no_tool_calls("answer")` | prompted/admitted、step、text start/delta/end、step ended。 |
| reasoning + text | 可配置 chunk mock | reasoning start/delta/end 与 text block ID、顺序、完整 ended text。 |
| tool success | `MultiRoundMockLlm`：tool-call 后 final | 7 个 tool type、single input delta、provider.executed=false、terminal 唯一。 |
| tool failed | mock tool error / error end | tool.failed `UnknownError`，不能同时 success。 |
| provider failed | failing mock | step.failed V2、打开 block 终结、无 step.ended。 |
| cancel | delayed mock + interrupt | idle 之后没有新 delta，reconnect 无重复 durable seq。 |
| multi-turn | `MultiRoundMockLlm` | 每 session aggregate 连续，assistant/message/block ID 不串。 |

不得在 artifact、assertion failure 或 CI log 输出 prompt、tool output 或 mock error 原文；test fixture 使用固定短字符串即可。

## 8. 替换步骤与 CI gate

1. 删除/改写 `protocol.rs::session_event_endpoint_is_sse_and_replays_only_that_session`，用 S1–S4 替代。
2. 新增 TCP helper 与 L1–L3、G1–G3、S1–S8；先不依赖 Provider。
3. 新增 JSONL P1–P5 和 32 fixture schema gate。
4. 新增 `test-support` MockLlm 注入与 6.1 的全部 deterministic agent 场景。
5. 删除所有“nightly Provider/SDK E2E”为完成条件的描述；外部联调另立任务。

PR gate：format、`cargo test -p loom-server --features test-support --test sse_v2_integration`、fixture gate、MockLlm agent SSE 场景、100% `sse-structure-coverage.json` gate 和现有 legacy tests。任何降低为“只检查 HTTP 200”、绕过 agent runner/translator、缺少 registry 项、coverage 小于 100%，或把 session stream 改回 legacy heartbeat 的修改都必须失败。

## 9. 完成判据

只有下列全部成立，SSE 集成测试才算完成：`sse-structure-coverage.json` 显示 **100%** 且无缺失 structure ID；32 fixture 及实际返回 frame 均通过严格 structural validator；S1–S9/P1–P5 都在 CI green；6.1 的全部 MockLlm 场景通过；legacy/global stream 不回归。代码 line/branch 覆盖率若启用，应另行报告，不能取代或稀释本方案的 100% 返回结构覆盖 gate。
