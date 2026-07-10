# ACP 协议审计：initialize

**协议 ID：** `initialize`
**审计日期：** 2025-08-19
**结论：** 部分实现 — 已确认 3 个差距
**置信度：** 高

---

## 协议规范

`initialize` 协议是 Client → Agent 的握手协议，其作用为：

1. 在 client 和 agent 之间协商 ACP 协议版本
2. 交换能力声明，让双方知道对方支持什么
3. 建立所有 ACP agent 必须遵守的基线能力

根据 `apps/acp/src/protocol.rs:18`，`InitializeResponse` schema 包含以下已记录的能力字段：

- `mcpCapabilities` — MCP 运行时/工具能力
- `promptCapabilities` — 基线字段 `text` 和 `resource_link`（最低要求）
- `sessionCapabilities` — session 生命周期支持
- `loadSession` — 恢复先前 session 的能力

---

## 实现状态

**部分实现**

Loom 在 `agent.rs:349-401` 使用 `agent-client-protocol` v0.15.1 实现协议握手。存在三个真实的合规性差距：

1. **`mcpCapabilities` 缺失** — Loom 拥有完整的 MCP 运行时（`tool-basic/src/mcp/`），但未在 `InitializeResponse` 中声明
2. **`promptCapabilities` 缺少基线字段** — `protocol.rs:18` 中记录的 `text` 和 `resource_link` 字段未实现
3. **`ClientCapabilitiesInfo` 忽略入站能力** — 在文档注释中解析了 `mcpCapabilities` 和 `promptCapabilities`，但实际从未从入站 JSON 中提取

---

## 实现细节

### 协议注册

**`apps/acp/src/protocol.rs`** — 协议 ID 定义

```rust
// protocol.rs:18 — Documented InitializeResponse fields
pub struct InitializeResponse {
    pub protocolVersion: Version,
    pub capabilities: InitializeCapabilities, // mcpCapabilities, promptCapabilities, etc.
    pub reason: String,
}
```

### Initialize 处理器

**`apps/acp/src/agent.rs:349-401`** — initialize 主处理器

```rust
// agent.rs:349
pub async fn handle_initialize(
    &self,
    req: InitializeRequest,
) -> Result<InitializeResponse, AgentError> {
    let version = Version::parse(&req.protocolVersion)
        .context("invalid protocol version")?;

    let caps = ClientCapabilitiesInfo::from_request(&req.clientInfo, &req.capabilities);

    // ... session setup ...

    // InitializeResponse is built here
    Ok(InitializeResponse {
        protocolVersion: PROTOCOL_VERSION.clone(),
        capabilities: AgentCapabilities { /* gaps here */ },
        reason: "ok".into(),
    })
}
```

### 客户端能力解析

**`apps/acp/src/client_capabilities.rs`** — `ClientCapabilitiesInfo` 结构体

```rust
// client_capabilities.rs
/// Parsed client capabilities.
/// Documents mcpCapabilities and promptCapabilities in doc comments...
pub struct ClientCapabilitiesInfo {
    pub mcp: Option<...>,     // documented but NEVER populated from JSON
    pub prompts: Option<...>, // documented but NEVER populated from JSON
    pub session: Option<...>,
}
```

**差距：** `ClientCapabilitiesInfo::from_request()` 实际上从未从入站 JSON 中读取 `mcpCapabilities` 或 `promptCapabilities`，尽管文档注释暗示这些字段已被处理。

### Stdio 循环集成

**`apps/acp/src/stdio_loop.rs:282-301`** — stdio 传输的接入点

```rust
// stdio_loop.rs:282
async fn handle_initialize(req: InitializeRequest) -> Result<InitializeResponse, ...> {
    agent.handle_initialize(req).await
}
```

### 端到端测试

**`apps/acp/tests/e2e_mega.rs:39-51`** — 测试覆盖

```rust
// e2e_mega.rs:39
#[tokio::test]
async fn test_initialize() {
    let res = client.initialize(InitializeRequest { ... }).await;
    assert!(res.protocol_version >= 1);
    assert!(res.capabilities.prompt_capabilities.is_some());
    // Missing assertions for:
    // - mcpCapabilities
    // - loadSession / sessionCapabilities
    // - promptCapabilities.{text, resource_link}
}
```

**测试差距：** 仅校验 `protocolVersion ≥ 1` 和 `promptCapabilities` 的存在性，未验证实际字段内容。

---

## 实现方式

Loom 使用 `agent-client-protocol` v0.15.1 作为协议基础。`InitializeResponse.capabilities` 字段由 `AgentCapabilities` 结构体构建。处理器在连接时同步运行于 stdio 循环中，阻塞直到完成才进入主事件循环。

MCP 支持位于独立模块（`tool-basic/src/mcp/`），其中包含 `McpToolSource`、`McpSession` 和 `McpHttpSession` — 实现完整但未接入到 `InitializeResponse` 的能力声明中。

---

## 差距与问题

| # | 严重程度 | 差距 | 位置 |
|---|----------|-----|------|
| 1 | **高** | `InitializeResponse` 中未返回 `mcpCapabilities`，尽管 `tool-basic/src/mcp/` 存在完整 MCP 运行时 | `agent.rs:349-401` |
| 2 | **中** | `promptCapabilities` 缺少 `protocol.rs:18` 文档中规定的 `text` 和 `resource_link` 基线字段 | `agent.rs:349-401` |
| 3 | **低** | `ClientCapabilitiesInfo` 文档说明解析 `mcpCapabilities`/`promptCapabilities`，但实际未从入站 JSON 中提取 | `client_capabilities.rs` |
| 4 | **低** | 端到端测试仅验证 `protocolVersion` 和 `promptCapabilities` 的存在性，未验证实际字段内容 | `e2e_mega.rs:39-51` |

**注：** `protocolCapabilities`（在服务器响应中回传客户端能力）**不是** ACP initialize 标准字段，正确地未被实现。

---

## 验证

**过程：** 对已确认文件进行对抗性分析 — `agent.rs:349-401`、`stdio_loop.rs:282-301`、`client_capabilities.rs`、`protocol.rs`、`e2e_mega.rs:39-51`、`tool-basic/src/mcp/`。

**结论：**
- 三个差距通过对实际代码与 `protocol.rs:18` 的交叉引用得到确认
- `tool-basic/src/mcp/` 的存在证明 Loom 具备 MCP 能力但未对外声明
- `ClientCapabilitiesInfo` 的文档注释提及 `mcpCapabilities`/`promptCapabilities`，但提取代码缺失
- 端到端测试断言不足以验证协议合规性

**对初始分析的细微修正：** `protocolCapabilities` 缺失最初被标记但并非真实差距 — 它不是 ACP initialize 标准字段。

---

## 总结

Loom 的 `initialize` 实现对 ACP 协议 v0.15.1 **部分合规**。核心握手正确工作，但三个差距阻碍了完全合规：

1. **优先修复：** 将 `tool-basic/src/mcp/` 的 MCP 能力接入到 `InitializeResponse` — 鉴于 Loom 拥有完整 MCP 运行时，这是最显著的遗漏
2. **次要修复：** 根据 `protocol.rs:18` 基线规范在 `promptCapabilities` 中添加 `text` 和 `resource_link`
3. **第三修复：** 在 `ClientCapabilitiesInfo::from_request()` 中实现 `mcpCapabilities`/`promptCapabilities` 的提取
4. **加强测试：** 在 `e2e_mega.rs` 中添加对所有文档化能力字段的断言

这些差距属于实现差距，不是架构问题 — 协议处理器结构健全，正确遵循了 `agent-client-protocol` v0.15.1 契约。

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:349-401（当前实现）
pub async fn handle_initialize(
    &self,
    req: InitializeRequest,
) -> Result<InitializeResponse, AgentError> {
    // ✓ 协议版本协商（working）
    // ✗ mcpCapabilities 未返回（尽管有完整 MCP 运行时）
    // ✗ promptCapabilities 缺少 text + resource_link 基线字段
    // ✗ ClientCapabilitiesInfo 忽略入站字段
    // ✓ capabilities 基础结构存在
    Ok(InitializeResponse {
        protocolVersion: PROTOCOL_VERSION.clone(),
        capabilities: AgentCapabilities { /* 不完整 */ },
        reason: "ok".into(),
    })
}
```

### 差距 1 修复：声明 mcpCapabilities

**问题位置：** `apps/acp/src/agent.rs:349-401`

Loom 在 `tool-basic/src/mcp/` 拥有完整 MCP 运行时（`McpToolSource`、`McpSession`、`McpHttpSession`），但未在 `InitializeResponse` 中声明 `mcpCapabilities`。

**修复前 vs 修复后：**

```rust
// apps/acp/src/agent.rs:373-395

// 【修复前】仅声明 loadSession 和 promptCapabilities
obj.insert(
    "agentCapabilities".to_string(),
    serde_json::json!({
        "loadSession": true,
        "sessionCapabilities": { "list": {}, "fork": {} },
        "promptCapabilities": { "embeddedContext": true, "image": true, "audio": true }
    }),
);

// 【修复后】补全 mcpCapabilities
obj.insert(
    "agentCapabilities".to_string(),
    serde_json::json!({
        "loadSession": true,
        "sessionCapabilities": { "list": {}, "fork": {} },
        "mcpCapabilities": {                        // ← 新增
            "http": true,                            //   通过 McpHttpSession 支持
            "stdio": true,                           //   通过 McpSession 支持
            "sse": false,                            //   尚未实现
        },
        "promptCapabilities": {
            "embeddedContext": true,
            "image": true,
            "audio": true,
            "text": true,                            // ← 新增（基线）
            "resource_link": true,                   // ← 新增（基线）
        }
    }),
);
```

### 差距 2 修复：promptCapabilities 补全基线字段

**问题位置：** `apps/acp/src/agent.rs:373-395`（修复与差距 1 合并）

`protocol.rs:18` 文档规定 `promptCapabilities` 应至少包含 `text` 和 `resource_link` 两个基线字段，但当前实现缺少。

**修复后 JSON 结构：**

```json
{
  "promptCapabilities": {
    "text": true,
    "resource_link": true,
    "image": true,
    "audio": true,
    "embeddedContext": true
  }
}
```

**字段说明：**
- `text: true` — 支持纯文本 prompt（基线，必须）
- `resource_link: true` — 支持资源链接（基线，必须）
- `image: true` — 支持多模态图像（Loom 扩展）
- `audio: true` — 支持多模态音频（Loom 扩展）
- `embeddedContext: true` — 支持嵌入式上下文（Loom 扩展）

### 差距 3 修复：ClientCapabilitiesInfo 实际解析入站字段

**问题位置：** `apps/acp/src/client_capabilities.rs`

`ClientCapabilitiesInfo` 在文档注释中描述了 `mcpCapabilities` 和 `promptCapabilities`，但实际从未从入站 JSON 中提取。

**修复前：**

```rust
// apps/acp/src/client_capabilities.rs（当前）
pub struct ClientCapabilitiesInfo {
    pub mcp: Option<...>,     // 文档说明会从 req.capabilities 解析
    pub prompts: Option<...>, // 但实际从未填充
    pub session: Option<...>,
}

impl ClientCapabilitiesInfo {
    pub fn from_request(_client: &Client, _caps: &ClientCapabilities) -> Self {
        // 仅返回空结构体 — 忽略入站字段
        Self { mcp: None, prompts: None, session: None }
    }
}
```

**修复后：**

```rust
// apps/acp/src/client_capabilities.rs（修复）
use agent_client_protocol::schema::ClientCapabilities;

pub struct ClientCapabilitiesInfo {
    pub mcp: Option<McpClientCapabilities>,
    pub prompts: Option<PromptCapabilities>,
    pub session: Option<SessionCapabilities>,
}

impl ClientCapabilitiesInfo {
    pub fn from_request(
        _client: &Client,
        caps: &ClientCapabilities,
    ) -> Result<Self, AcpError> {
        // 实际从入站 JSON 反序列化
        let mcp = caps.mcp.as_ref().map(|m| McpClientCapabilities {
            http: m.http.unwrap_or(false),
            stdio: m.stdio.unwrap_or(false),
            sse: m.sse.unwrap_or(false),
        });

        let prompts = caps.prompts.as_ref().map(|p| PromptCapabilities {
            text: p.text.unwrap_or(false),
            resource_link: p.resource_link.unwrap_or(false),
            image: p.image.unwrap_or(false),
            audio: p.audio.unwrap_or(false),
            embedded_context: p.embedded_context.unwrap_or(false),
        });

        let session = caps.session.as_ref().map(|s| SessionCapabilities {
            list: s.list.is_some(),
            fork: s.fork.is_some(),
            resume: s.resume.is_some(),
        });

        Ok(Self { mcp, prompts, session })
    }
}
```

### 演示：修复后的 Initialize 完整握手

**Client 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "0.15.1",
    "clientInfo": { "name": "vscode-acp", "version": "1.2.0" },
    "capabilities": {
      "mcp": { "http": true, "stdio": true },
      "prompts": {
        "text": true,
        "resource_link": true,
        "image": true,
        "audio": true,
        "embedded_context": true
      },
      "session": { "list": {}, "fork": {}, "resume": {} }
    }
  }
}
```

**Agent 响应（修复后）：**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "0.15.1",
    "capabilities": {
      "loadSession": true,
      "sessionCapabilities": {
        "list": {},
        "fork": {}
      },
      "mcpCapabilities": {
        "http": true,
        "stdio": true,
        "sse": false
      },
      "promptCapabilities": {
        "text": true,
        "resource_link": true,
        "image": true,
        "audio": true,
        "embeddedContext": true
      }
    },
    "reason": "ok"
  }
}
```

**关键变化：**
- 客户端的 `mcp.http: true` 被 Loom 接收并尊重（vs 修复前被忽略）
- 客户端的 `prompts.text: true` 被 Loom 验证并支持
- 客户端的 `session.resume: {}` 被 Loom 接收（修复前忽略）

### 演示：能力协商的运行时影响

```text
【修复前】客户端发送 supports MCP HTTP：

Client: initialize { mcp: { http: true } }
  ↓
Agent: 解析失败（忽略 mcp 字段）→ mcp: None
  ↓
Agent: 后续运行时不使用 MCP HTTP 工具
  ↓
Client: 困惑 — 不知道为什么不工作

【修复后】客户端发送 supports MCP HTTP：

Client: initialize { mcp: { http: true } }
  ↓
Agent: 正确解析 → mcp.http: true
  ↓
Agent: 初始化时连接 MCP HTTP 服务器
  ↓
Client: MCP HTTP 工具按预期工作
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中扩展 `test_initialize`：

```rust
#[tokio::test]
async fn test_initialize_declares_mcp_capabilities() {
    let res = client.initialize(InitializeRequest { ... }).await?;
    let caps = res.capabilities;

    // 差距 1 修复：mcpCapabilities 必须存在
    assert!(caps.mcp_capabilities.is_some(),
            "agent must declare mcpCapabilities");
    let mcp = caps.mcp_capabilities.unwrap();
    assert!(mcp.http, "agent supports MCP HTTP");
    assert!(mcp.stdio, "agent supports MCP stdio");
}

#[tokio::test]
async fn test_initialize_prompt_capabilities_baseline() {
    let res = client.initialize(InitializeRequest { ... }).await?;
    let prompts = res.capabilities.prompt_capabilities.unwrap();

    // 差距 2 修复：基线字段
    assert!(prompts.text, "promptCapabilities.text must be true (baseline)");
    assert!(prompts.resource_link,
            "promptCapabilities.resource_link must be true (baseline)");
}

#[tokio::test]
async fn test_initialize_parses_client_mcp_capabilities() {
    // 差距 3 修复：ClientCapabilitiesInfo 实际解析入站字段
    let req = InitializeRequest {
        capabilities: ClientCapabilities {
            mcp: Some(McpCapabilities { http: Some(true), stdio: Some(false), .. }),
            ..Default::default()
        },
        ..Default::default()
    };

    let info = ClientCapabilitiesInfo::from_request(&client, &req.capabilities)?;
    assert_eq!(info.mcp.as_ref().unwrap().http, true);
    assert_eq!(info.mcp.as_ref().unwrap().stdio, false);
}

#[tokio::test]
async fn test_initialize_parses_client_prompt_capabilities() {
    let req = InitializeRequest {
        capabilities: ClientCapabilities {
            prompts: Some(PromptCapabilities {
                text: Some(true),
                image: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let info = ClientCapabilitiesInfo::from_request(&client, &req.capabilities)?;
    let prompts = info.prompts.as_ref().unwrap();
    assert!(prompts.text);
    assert!(prompts.image);
    assert!(!prompts.audio);  // 客户端未声明
}

#[tokio::test]
async fn test_initialize_mcp_runtime_actually_works() {
    // 集成测试：声明能力 + 真实使用
    let res = client.initialize(InitializeRequest { ... }).await?;
    assert!(res.capabilities.mcp_capabilities.is_some());

    // 启动 MCP HTTP 服务器，验证 agent 能连接
    let mcp_url = start_test_mcp_http_server().await?;
    let mcp_id = client.connect_mcp(mcp_url).await?;
    let tools = client.list_mcp_tools(mcp_id).await?;
    assert!(!tools.is_empty(), "MCP tools should be available after init");
}
```

### 验收清单

**差距 1 — mcpCapabilities 声明：**
- [ ] `agent.rs:373-395` 添加 `mcpCapabilities` 字段（http/stdio/sse）
- [ ] 从 `tool-basic/src/mcp/` 反映真实支持情况
- [ ] 验证：`mcpCapabilities.http = true`（因为有 `McpHttpSession`）
- [ ] 验证：`mcpCapabilities.stdio = true`（因为有 `McpSession`）
- [ ] 验证：`mcpCapabilities.sse = false`（如未实现）

**差距 2 — promptCapabilities 基线：**
- [ ] `agent.rs:373-395` 添加 `text: true`
- [ ] `agent.rs:373-395` 添加 `resource_link: true`
- [ ] 验证：响应包含两个基线字段

**差距 3 — ClientCapabilitiesInfo 解析：**
- [ ] `client_capabilities.rs` 实现 `from_request` 反序列化逻辑
- [ ] 提取 `mcp.http/stdio/sse` 字段（默认 false）
- [ ] 提取 `prompts.text/resource_link/image/audio/embedded_context` 字段（默认 false）
- [ ] 提取 `session.list/fork/resume` 字段（默认 false）
- [ ] 修复后能根据客户端能力跳过不支持的功能

**测试覆盖：**
- [ ] 4 个新测试（mcp 声明、prompt 基线、客户端解析 ×2、运行时验证）
- [ ] 验证修复前 e2e 失败 / 修复后通过

---

## 扩展测试套件（`initialize_matrix.rs`）

除验收清单中的 5 个核心测试外，可选维度已在独立测试文件中覆盖：

**文件：** `apps/acp/tests/initialize_matrix.rs`

### 覆盖范围

| 类别 | 测试数 | 目的 |
|------|--------|------|
| **8 组合真值表** | 8 | 验证 mcp / prompts / session 任意组合下解析正确性 |
| **性能基准** | 2 | `from_request` < 10ms；e2e initialize < 50ms |
| **模糊测试** | 2 | 1000 次随机 JSON + 8 种畸形 JSON 不能 panic |
| **并发隔离** | 2 | 8 客户端并发 + 能力不泄露 |
| **优雅降级** | 4 | 缺字段、部分字段、v0.14.0 旧客户端、下游路由正确性 |
| **合计** | **18** | 矩阵 + 性能 + 健壮性 + 隔离 + 降级 |

### 8 组合真值表（关键覆盖）

| # | mcp | prompts | session | 验证 |
|---|-----|---------|---------|------|
| 000 | ❌ | ❌ | ❌ | 全部 None，无 panic |
| 001 | ✅ | ❌ | ❌ | mcp.http=true 被解析 |
| 010 | ❌ | ✅ | ❌ | image / resource_link 被解析 |
| 011 | ✅ | ✅ | ❌ | mcp + prompts 独立正确 |
| 100 | ❌ | ❌ | ✅ | list / fork / resume 被解析 |
| 101 | ✅ | ❌ | ✅ | mcp + session 独立正确 |
| 110 | ❌ | ✅ | ✅ | prompts + session 独立正确 |
| 111 | ✅ | ✅ | ✅ | 全部子字段正确填充 |

### 性能基准

```text
from_request 平均延迟: < 10ms   （每次调用）
e2e initialize 往返:    < 50ms  （client → agent → response）
```

### 模糊测试覆盖

- 1000 次随机能力组合（seed = `0xC0FFEE`）
- 8 种畸形 JSON（string / null / array / 类型错误 / 未知字段）

### 并发隔离

- 8 客户端不同能力组合并发
- 验证 `ClientCapabilitiesInfo` 按客户端隔离，无泄露

### 优雅降级

| 场景 | 行为 |
|------|------|
| `capabilities` 字段缺失 | 三个子能力全 None，无 panic |
| 部分字段为 `None` | 缺失字段默认 `false` |
| v0.14.0 旧客户端 | 缺失字段 → None（兼容） |
| 下游路由 | mcp-http 客户端走 AcpBridge executor |

### 运行方式

```bash
# 单独运行
cargo test -p acp --test initialize_matrix -- --nocapture

# 与其他测试一起
cargo test -p acp

# 性能基准确认
cargo test -p acp --test initialize_matrix perf_ -- --nocapture
```

### 验收

```text
预期：18 passed; 0 failed

若 8 组合真值表失败 → 修复前后 from_request 逻辑有 bug
若性能测试失败       → from_request 有意外开销（DB 调用？锁？）
若模糊测试 panic     → 必须修复（健壮性问题）
若并发隔离失败       → 状态污染（严重 bug）
若优雅降级失败       → 兼容性问题
```
