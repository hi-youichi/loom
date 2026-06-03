# 方案 A：删除 ToolSource trait，统一到 Tool + ToolRegistry

## 目标

将当前 5 层架构简化为 2 层，删除 `ToolSource` trait 及所有相关中间层，消除重复代码。

## 当前架构（5 层）

```
Tool trait (单工具接口: name/spec/call)
  → ToolRegistry + ToolRegistryLocked (HashMap<String, Box<dyn Tool>>)
    → AggregateToolSource (桥接 Tool → ToolSource + context 存储)
      → 各种 *ToolSource wrapper (8 个文件, 纯样板转发)
        → ActNode 消费 dyn ToolSource
```

**问题**：
- `Tool` 和 `ToolSource` 两个 trait 高度重复
- `AggregateToolSource` 是 `ToolRegistryLocked` 的无意义包装
- 8 个 `*ToolSource` 文件几乎全是样板代码
- `set_call_context` + `call_tool_with_context` 双通道冗余

## 目标架构（2 层）

```
Tool trait (保留不变)
  → ToolRegistry (增强: 添加 list/call/filter/dry-run 能力)
    → ActNode 直接持有 ToolRegistry
```

## 详细步骤

---

### 第一阶段：增强 ToolRegistry，使其能替代 ToolSource

#### 1.1 给 ToolRegistry 添加装饰器能力

在 `loom/src/tools/registry.rs` 中：

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    filter: Option<BuiltinToolFilter>,  // 新增
    dry_run: bool,                       // 新增
    yaml_specs: Option<Vec<ToolSpec>>,   // 新增
}
```

新增方法：
- `list()` — 返回 tools 的 spec（已存在），应用 filter 和 yaml_specs override
- `call(name, args, ctx)` — 已存在，增加 filter 检查和 dry-run 拦截
- `set_filter(filter: Option<BuiltinToolFilter>)` — 设置过滤器
- `set_dry_run(enabled: bool)` — 设置 dry-run 模式
- `set_yaml_specs(specs: Vec<ToolSpec>)` — 设置 YAML spec override
- `register(tool: Box<dyn Tool>)` — 已存在

#### 1.2 简化 ToolRegistryLocked

```rust
pub struct ToolRegistryLocked {
    inner: Arc<RwLock<ToolRegistry>>,
}
```

方法不变（register_async, register_sync, list, call），但 list 和 call 内部会应用 filter/dry_run。

---

### 第二阶段：删除 AggregateToolSource

#### 2.1 将 context 管理内联到 ToolRegistry

`AggregateToolSource` 唯一的"额外逻辑"是 context 存储和 fallback。简化方案：

- 删除 `set_call_context` 机制
- `ToolRegistryLocked::call()` 始终接收 `ctx: Option<&ToolCallContext>`（已经是这样）
- ActNode 在每次调用时直接传 ctx，无需预存储

#### 2.2 删除文件

- `loom/src/tools/aggregate_source.rs` — 整个文件删除

#### 2.3 迁移调用点

`AggregateToolSource::new()` + `register_*` → `ToolRegistryLocked::new()` + `register_*`

---

### 第三阶段：删除所有 *ToolSource wrapper 文件

以下文件的逻辑通过两种方式迁移：

#### 3.1 直接删除（`new()` 返回 AggregateToolSource 的那些）

这些文件的 `new()` 实际已经返回 `AggregateToolSource`，不需要 ToolSource impl：

| 文件 | 迁移方案 |
|------|---------|
| `bash_tools_source.rs` | `BashToolsSource::new()` → 改为返回 `ToolRegistryLocked`，或改为 helper 函数 `fn register_bash_tools(registry: &ToolRegistryLocked)` |
| `web_tools_source.rs` | 同上：`fn register_web_tools(registry: &ToolRegistryLocked)` |
| `store_tool_source.rs` | 同上：`fn register_store_tools(registry: &ToolRegistryLocked, store, namespace)` |
| `memory_tools_source.rs` | 同上：`fn register_memory_tools(registry: &ToolRegistryLocked, store, namespace)` |
| `short_term_memory_tool_source.rs` | 同上：`fn register_short_term_memory(registry: &ToolRegistryLocked)` |
| `telegram_tools_source.rs` | 同上：`fn register_telegram_tools(registry: &ToolRegistryLocked)` |
| `file_tool_source.rs` | `register_file_tools()` 已经存在，改为接收 `&ToolRegistryLocked` 而非 `&AggregateToolSource` |
| `read_only_dir_tool_source.rs` | `register_read_only_dir_tools()` 改为接收 `&ToolRegistryLocked` |

#### 3.2 合并到 ToolRegistry

| 文件 | 迁移方案 |
|------|---------|
| `filtered_tool_source.rs` | filter 逻辑内联到 `ToolRegistry::list()` 和 `ToolRegistry::call()` |
| `dry_run_tool_source.rs` | dry_run 逻辑内联到 `ToolRegistry::call()` |
| `yaml_specs.rs` | YAML spec override 内联到 `ToolRegistry::list()` |

#### 3.3 保留但改造

| 文件 | 迁移方案 |
|------|---------|
| `mcp/mod.rs` | `McpToolSource` → 改为实现 `Tool` trait（通过 `McpToolAdapter` 已有的模式包装每个 MCP tool），或者保留为独立的 Tool 子注册表 |
| `mock.rs` | `MockToolSource` → 改为实现 `Tool` trait，或构造一个 `ToolRegistry` 填充 mock tools |
| `context.rs` | 保留，`ToolCallContext` 移动到 `loom/src/tools/` 下 |

---

### 第四阶段：改造 ActNode

#### 4.1 类型变更

```rust
// 之前
pub struct ActNode { tools: Box<dyn ToolSource>, ... }

// 之后
pub struct ActNode { tools: ToolRegistryLocked, ... }
```

#### 4.2 调用方式变更

```rust
// 之前
self.tools.set_call_context(Some(ctx.clone()));
let result = self.tools.call_tool_with_context(&name, args, Some(&ctx)).await;
self.tools.set_call_context(None);

// 之后
let result = self.tools.call(&name, args, Some(&ctx)).await;
```

删除所有 `set_call_context` 调用。

#### 4.3 ThinkNode 变更

```rust
// 之前
let tools = self.tools.list_tools().await?;

// 之后
let tools = self.tools.list().await;
```

---

### 第五阶段：清理

#### 5.1 删除 ToolSource trait

从 `loom/src/tool_source/mod.rs` 中删除：
- `ToolSource` trait 定义
- `set_call_context` 方法
- `call_tool` / `call_tool_with_context` 方法

#### 5.2 移动/合并模块

- `tool_source/context.rs` → `tools/context.rs`
- `tool_source/yaml_specs.rs` 中的 `load_tool_specs()` → `tools/` 下
- `tool_source/mcp/` → `tools/mcp/`（MCP 保留为独立模块）

#### 5.3 删除空目录

`loom/src/tool_source/` 目录可以大幅精简或删除。

---

### 第六阶段：更新测试

受影响的测试文件：
- `loom/tests/short_term_memory_tool_source.rs`
- `loom/tests/memory_tools_source.rs`
- `loom/tests/tool_streaming.rs`
- 各 `*ToolSource` 内的 `#[cfg(test)]` 模块

全部改为使用 `ToolRegistryLocked` 接口。

---

## 文件变更总结

### 删除的文件（~10 个）
- `loom/src/tools/aggregate_source.rs`
- `loom/src/tool_source/bash_tools_source.rs`
- `loom/src/tool_source/web_tools_source.rs`
- `loom/src/tool_source/store_tool_source.rs`
- `loom/src/tool_source/memory_tools_source.rs`
- `loom/src/tool_source/short_term_memory_tool_source.rs`
- `loom/src/tool_source/telegram_tools_source.rs`
- `loom/src/tool_source/filtered_tool_source.rs`
- `loom/src/tool_source/dry_run_tool_source.rs`
- `loom/src/tool_source/file_tool_source.rs`（只保留 `register_file_tools` 函数）

### 修改的文件（~8 个）
- `loom/src/tools/registry.rs` — 增加 filter/dry_run/yaml_specs
- `loom/src/tools/mod.rs` — 模块结构调整
- `loom/src/tool_source/mod.rs` — 删除 ToolSource trait
- `loom/src/agent/react/act_node.rs` — 改用 ToolRegistryLocked
- `loom/src/agent/react/build/tool_source.rs` — 简化构建逻辑
- `loom/src/agent/react/think_node.rs` — 改用 registry.list()
- `loom/src/tool_source/mcp/mod.rs` — 移动或改造
- `loom/src/tool_source/mock.rs` — 改造

### 新增的文件（~2 个）
- `loom/src/tools/context.rs`（从 tool_source/ 移动）
- `loom/src/tools/registry_helpers.rs`（集中注册 helper 函数）

---

## 风险和注意事项

1. **MCP 适配**：`McpToolSource` 直接实现 `ToolSource`，不走 `Tool` trait。需要用 `McpToolAdapter` 模式将其工具注册到 registry。
2. **公共 API**：`ToolSource` 可能被外部 crate 使用（通过 pub），需要确认是否有外部依赖。
3. **渐进执行**：建议按阶段分 PR 提交，每个阶段可独立编译通过。
4. **YamlSpecToolSource**：当前在 `build_tool_source` 末尾包裹所有工具。合并后，yaml override 成为 registry 的 list() 逻辑的一部分。
5. **`register_file_tools` / `register_read_only_dir_tools`**：这些函数目前接收 `&AggregateToolSource`，需要改为接收 `&ToolRegistryLocked`。

---

## 预估收益

- **减少 ~1000 行**样板代码
- **删除 10 个文件**
- **API 简化**：`Tool` + `ToolRegistry` 两个概念替代原来的 `Tool` + `ToolRegistry` + `ToolSource` + `AggregateToolSource` + 8 个 wrapper
- **消除 context 双通道**：只有 `call(name, args, ctx)` 一种调用方式
