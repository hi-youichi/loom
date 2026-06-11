# 移除非 Stream 执行路径方案

## 背景

当前 Loom 的 Graph 执行引擎（`loom-graph` + `loom-pregel`）同时支持两种执行模式：

- **Stream 模式**：通过 `compiled.stream()` 创建 channel-backed stream，实时推送 `StreamEvent`
- **非 Stream 模式**（invoke）：通过 `compiled.invoke()` / `invoke_with_context()` 直接返回最终状态，无中间事件

生产代码（CLI、ACP、Telegram Bot）已全部走 Stream 模式。非 Stream 路径仅存在于 Runner 层对 `invoke()` 的调用，以及部分示例和测试中。

graph 和 pregel 层**保留**不动。清理范围仅限于 runner 层及以上的显式非 stream 调用路径。

## 目标

**只保留 Stream 模式，移除 Runner 层及以上的非 Stream 调用路径。** 具体包括：

1. ~~删除所有 Agent Runner 的 `invoke()` 方法~~ ✅
2. ~~删除顶层 `run_agent()` 函数~~ ✅
3. 清理示例中对 `compiled.invoke()` 的调用 — **跳过**（graph 层保留不动，示例直接使用 graph API）

## 实施步骤

### 第 1 步：React Runner 精简 ✅

**文件：`loom-agent/src/agent/react/runner/runner.rs`**

- ✅ 删除 `invoke()` 和 `invoke_with_config()` 方法
- ✅ 删除 `run_agent()` 函数
- ✅ `agent_tool.rs` 中的 `runner.invoke()` 改为 `runner.stream_with_callback()`
- ✅ `runners.rs` 测试更新为 stream 调用
- ✅ `mod.rs` 和 `react/mod.rs` 移除 `run_agent` 的 re-export
- ✅ 更新模块文档注释

### 第 2 步：DUP/ToT/GoT Runner 精简 ✅

- ✅ **`loom-agent/src/agent/dup/runner.rs`** — 删除 `invoke()` 和 `invoke_with_config()` 方法，测试更新为 stream
- ✅ **`loom-agent/src/agent/tot/runner.rs`** — 同上
- ✅ **`loom-agent/src/agent/got/runner.rs`** — 同上

### 第 3 步：顶层 API 精简 ✅

**文件：`loom-agent/src/cli_run_agent.rs`**

- ✅ 确认 `run_agent_with_options()` 已全走 stream 路径——无需改动

### 第 4 步：示例和测试清理 — 跳过

按设计文档原则"graph 和 pregel 层保留不动"，以下文件直接使用 `loom-graph` 层的 `CompiledStateGraph::invoke()` API，属于 graph 层使用范畴，不在本次 Runner 层清理范围内：

| 文件 | 原因 |
|------|------|
| `loom-examples/examples/state_graph_echo.rs:65` | 直接使用 graph API |
| `loom-examples/examples/memory_persistence.rs:71` | 直接使用 graph API |
| `loom-examples/examples/memory_checkpoint.rs:65` | 直接使用 graph API |
| `loom-examples/examples/react_memory.rs:547` | 直接使用 graph API |
| `loom-compress/src/graph.rs:62/72/107` | 内部子图节点，graph 层 |

## 验证结果

| 检查项 | 结果 |
|--------|------|
| `cargo build -p loom-agent` | ✅ 通过 |
| `cargo clippy -p loom-agent -- -D warnings` | ✅ 通过 |
| `cargo clippy --workspace -- -D warnings` | ✅ 通过 |
| `cargo test -p loom-agent` (131 tests) | ✅ 全部通过 |
| `cargo test --workspace` | ✅ 通过（config crate 有预先存在的 16 个失败，与本次变更无关） |

## 变更文件清单

| 文件 | 变更 |
|------|------|
| `loom-agent/src/agent/react/runner/runner.rs` | 删除 `invoke()`/`invoke_with_config()`/`run_agent()`，更新文档 |
| `loom-agent/src/agent/react/runner/mod.rs` | 移除 `run_agent` re-export |
| `loom-agent/src/agent/react/mod.rs` | 移除 `run_agent` re-export |
| `loom-agent/src/agent/react/agent_tool.rs` | `invoke()` → `stream_with_callback()` |
| `loom-agent/src/agent/react/build/runners.rs` | 测试改用 stream |
| `loom-agent/src/agent/dup/runner.rs` | 删除 `invoke()`/`invoke_with_config()`，测试改用 stream |
| `loom-agent/src/agent/tot/runner.rs` | 删除 `invoke()`/`invoke_with_config()`，测试改用 stream |
| `loom-agent/src/agent/got/runner.rs` | 删除 `invoke()`/`invoke_with_config()`，测试改用 stream |
| `loom-agent/src/agent/react/runner/options.rs` | 更新文档注释 |

## 影响范围

| 维度 | 影响 |
|------|------|
| `loom-graph` | 不动 |
| `loom-pregel` | 不动 |
| `StreamEvent` / `StreamMode` | 不动 |
| `RunContext` | 不动 |
| CLI 用户交互 | 无影响 |
| ACP 协议 | 无影响 |
| Telegram Bot | 无影响 |
| 工具执行 | 无影响 |
| LLM client (`llm.invoke()`) | 无关，不动 |
