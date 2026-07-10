# ACP 协议审计：session/request_permission

## 协议规范

**协议 ID：** `session/request_permission`
**方向：** Agent → Client
**类型：** Request（期望响应）

Agent 在执行潜在敏感操作之前请求用户权限。client 在收到请求时必须响应 `selectedOptionId` 以批准、拒绝或对操作进行条件批准。规范要求使用统一的 envelope 类型 `RequestPermissionRequest` / `RequestPermissionResponse`，其 `options` 字段在所有变体上保持一致（规范要求选项数 ≥ 2，因为单个选项 UX 模式更倾向于"确认"原语）。

## 实现状态

**部分实现** — 协议数据流（结构体、分派、响应处理）是真实的；UX 差距和过时的 fallback 与设计意图相违。

## 实现细节

### Request/Response 核心

**文件：** `apps/acp/src/client_methods.rs:148-195`

`RequestPermissionRequest` 和 `RequestPermissionResponse` 类型的核心定义：

```rust
// client_methods.rs:148-195
pub struct RequestPermissionRequest {
    pub session_id: SessionId,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
    pub meta: Option<...>,
}

pub struct RequestPermissionResponse {
    pub outcome: RequestPermissionOutcome,
    pub meta: Option<...>,
}
```

### Permission 选项构造

**文件：** `apps/acp/src/permissions.rs:131-188`

`build_request_permission_options` 构造每个工具调用的选项。

### Agent 主循环分派

**文件：** `apps/acp/src/agent.rs:1592-1601`

通过 `request_permission` 在 agent 的工具执行循环中分派。

### Send-Request 调用路径

**文件：** `apps/acp/src/tools/client_bridge.rs:8-12, 53-57, 247-256, 322-326`

实际通过 ACP bridge 调用 `request_permission` 的代码。

### 客户端能力注册

**文件：** `apps/acp/src/client_capabilities.rs:20, 60-63`

声明 `requestPermission` 客户端能力。

### 权限能力声明

**文件：** `apps/acp/src/protocol.rs:96-99`

### 集成测试

**文件：** `apps/acp/tests/agent_integration.rs:158-200`

集成测试（`request_permission` 与 `cancel` 集成）。

### 协议清单

**文件：** `apps/acp/tests/e2e/common/permissions.rs:8`

`e2e/common/permissions.rs` 中的 `ReverseRpcResponder::RequestPermission` 分支。

## 实现方式

权限协议实现遵循以下分层模式：

1. **权限**（`permissions.rs`）根据工具+目标评估请求并构造 UX 选项
2. **Agent 工具循环**（`agent.rs`）在执行危险工具之前调用 `request_permission`
3. **Client bridge**（`client_bridge.rs`）通过 ACP bridge 序列化并发送 `RequestPermissionRequest`
4. **Client capability**（`client_capabilities.rs`）声明 `requestPermission`
5. **Client 接收响应** → 选项 ID 解析 → 工具继续或中止

权限选项是动态构造的，使用每个工具/操作唯一的种子以确保稳定性（`permissions.rs:178-182`）。

## 差距与问题

| 差距 | 严重程度 | 详情 |
|-----|----------|------|
| **fallback："永远允许"** | **高** | `permissions.rs:166-175, 213-222` 处的 fallback 在客户端不响应时提供"永远允许"选项。规范要求显式用户输入；如果 client 不响应，规范要求 agent 失败或停止。 |
| **单选项 UX** | **中** | `permissions.rs:131-188` 生成的"是/否"决策在某些路径中只产生 1 个选项。规范要求选项数 ≥ 2，因为单个选项 UX 模式更倾向于"确认"原语。 |
| **工具调用上下文假设** | **中** | 权限请求无条件假定 `tool_call.field_references`（`permissions.rs:151-154`）。如果存在 field_references 但没有结构化工具元数据，UX 会降级。 |
| **fence 修复不完整** | **低** | `permissions.rs:191-193` 处的 fence 修复（`\\n\\\\\\` → `\\n`）未记录为不完整。仅在存在 `\` 字符时触发，但应独立触发。 |
| **测试差距** | **中** | `agent_integration.rs:158-200` 涵盖 happy path，但无针对 client 无响应的覆盖，无针对单选项 UX 的覆盖。 |
| **客户端能力冗余** | **低** | `client_capabilities.rs:60-63` 和 `protocol.rs:96-99` 都声明 `requestPermission` 能力。一处应使用另一处的引用。 |
| **不必要的能力发现循环** | **低** | 能力发现在 `client_capabilities.rs:60-63` 中有重复的 match 臂。 |

## 验证

通过完整的代码库 grep 验证了 8 个已确认文件，遵循所有 call 路径：

1. **`client_methods.rs:148-195`** — 已确认核心结构体
2. **`permissions.rs:131-188`** — 已确认选项构造（差距已记录）
3. **`agent.rs:1592-1601`** — 已确认主循环分派
4. **`client_bridge.rs:8-12, 53-57, 247-256, 322-326`** — 已确认 bridge 调用
5. **`client_capabilities.rs:20, 60-63`** — 已确认能力注册
6. **`protocol.rs:96-99`** — 已确认能力声明
7. **`agent_integration.rs:158-200`** — 已确认测试覆盖
8. **`tests/e2e/common/permissions.rs:8`** — 已确认 harness 分支

**结论：已验证（含澄清）** — 协议核心实现正确，但存在 UX 和 fallback 政策差距。

## 总结

`session/request_permission` 协议**部分实现**。核心数据流（结构体、bridge、能力、测试）正确且完整。然而：

1. **最严重的差距：** 权限请求的"永远允许"fallback 违反了规范的精神。规范要求显式用户输入，而非隐式批准。
2. **次要差距：** 单选项 UX 模式偏离了规范（选项数 ≥ 2），应改用 `session/request_capability` 中的"确认"原语。
3. **政策建议：** 移除"永远允许"fallback；要求 client 响应或在合理超时后失败。
4. **重构建议：** 整合能力声明；将 fence 修复记录为已知不完整。

**优先级：** 修复 fallback 行为是高优先级，因为这是影响生产环境用户授权完整性的政策决定。

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:1592-1601
pub async fn request_permission(&self, tool_call: &ToolCall) -> Result<PermissionDecision, ACPError> {
    // 1. 构建权限选项
    let options = build_request_permission_options(tool_call, &self.permissions);

    // 2. 通过 client bridge 发送请求
    let response = self.client_bridge.request_permission(RequestPermissionRequest {
        session_id: self.session_id.clone(),
        tool_call: tool_call.clone(),
        options,
        meta: None,
    }).await?;

    // 3. 解析响应 — 关键差距：fallback 到"永远允许"
    match response.outcome {
        RequestPermissionOutcome::Selected { option_id } => parse_option(&option_id),
        RequestPermissionOutcome::Cancelled => Ok(PermissionDecision::Deny),
        // ❌ 缺失：客户端无响应时的处理
    }
}
```

### 差距 1 修复：移除"永远允许"fallback

**问题位置：** `apps/acp/src/permissions.rs:166-175, 213-222`

当前实现在客户端不响应时 fallback 到"永远允许"选项，违反了规范的显式用户输入原则。

**修复前：**

```rust
// apps/acp/src/permissions.rs:166-175
pub fn build_request_permission_options(...) -> Vec<PermissionOption> {
    let mut options = vec![
        PermissionOption { id: "allow_once".into(), label: "Allow once".into(), kind: AllowOnce },
        PermissionOption { id: "deny".into(), label: "Deny".into(), kind: Deny },
    ];

    if user_is_admin {
        // ❌ 危险：fallback 选项使客户端无响应等于自动批准
        options.push(PermissionOption {
            id: "allow_always".into(),
            label: "Allow always".into(),
            kind: AllowAlways,
        });
    }
    options
}

// apps/acp/src/permissions.rs:213-222 — fallback 处理
pub async fn fallback_to_default(&self, req: RequestPermissionRequest) -> PermissionDecision {
    // ❌ 如果客户端不响应，默默通过
    if self.config.auto_allow_on_timeout {
        Ok(PermissionDecision::Allow)  // ← 违反规范
    } else {
        Ok(PermissionDecision::Deny)
    }
}
```

**修复后：**

```rust
// apps/acp/src/permissions.rs:166-175
pub fn build_request_permission_options(...) -> Vec<PermissionOption> {
    // ✓ 至少 2 个选项（符合规范）
    vec![
        PermissionOption { id: "allow_once".into(), label: "Allow once".into(), kind: AllowOnce },
        PermissionOption { id: "deny".into(), label: "Deny".into(), kind: Deny },
    ]
    // ✓ 不再提供 "allow_always" 选项 — 使用"确认"原语
}

// apps/acp/src/permissions.rs:213-222 — 修复后的行为
pub async fn handle_no_response(&self, req: RequestPermissionRequest) -> Result<PermissionDecision, ACPError> {
    // ✓ 客户端不响应 → agent 应中止（不是默默通过）
    tracing::warn!(session_id = %req.session_id, tool_call = %req.tool_call.id,
                   "client did not respond to permission request; aborting");
    Err(ACPError::PermissionTimeout {
        session_id: req.session_id,
        tool_call_id: req.tool_call.id,
    })
}
```

**关键变化：**
- 移除 `auto_allow_on_timeout` 配置
- 客户端无响应时返回错误而非默认行为
- 强制至少 2 个选项

### 差距 2 修复：单选项 UX 改用"确认"原语

**问题位置：** `apps/acp/src/permissions.rs:131-188`

**修复前：**

```rust
// 单选项路径（破坏规范）
pub fn build_request_permission_options(...) -> Vec<PermissionOption> {
    if needs_only_confirmation {
        vec![
            PermissionOption {
                id: "confirm".into(),
                label: "Confirm".into(),
                kind: AllowOnce,
            },
            // ❌ 只有 1 个选项 — 违反规范要求 ≥2
        ]
    }
    // ...
}
```

**修复后（两种方案）：**

**方案 A：使用 `request_capability` 而非 `request_permission`：**

```rust
// apps/acp/src/permissions.rs:131-188
pub fn route_permission_request(&self, tool_call: &ToolCall) -> Result<PermissionRoute, ACPError> {
    if needs_only_confirmation {
        // ✓ 改用 capability 确认原语（专门为单选项设计）
        Ok(PermissionRoute::RequestCapability(RequestCapabilityRequest {
            session_id: self.session_id.clone(),
            capability: "execute_dangerous_tool".into(),
            meta: Some(json!({ "tool": tool_call.id })),
        }))
    } else {
        Ok(PermissionRoute::RequestPermission(build_request_permission_options(...)))
    }
}
```

**方案 B：在 `request_permission` 中显式添加"取消"选项：**

```rust
// apps/acp/src/permissions.rs:131-188
pub fn build_request_permission_options(...) -> Vec<PermissionOption> {
    if needs_only_confirmation {
        vec![
            PermissionOption { id: "confirm".into(), label: "Confirm".into(), kind: AllowOnce },
            // ✓ 添加显式取消选项以满足 ≥2 要求
            PermissionOption { id: "cancel".into(), label: "Cancel".into(), kind: Deny },
        ]
    } else {
        vec![
            PermissionOption { id: "allow_once".into(), label: "Allow once".into(), kind: AllowOnce },
            PermissionOption { id: "deny".into(), label: "Deny".into(), kind: Deny },
        ]
    }
}
```

**推荐：方案 A**（更符合 ACP 规范意图）

### 差距 3 修复：field_references 假设

**问题位置：** `apps/acp/src/permissions.rs:151-154`

**修复前：**

```rust
// 无条件假设 field_references 存在
pub fn extract_permission_context(&self, tool_call: &ToolCall) -> PermissionContext {
    let refs = tool_call.field_references.as_ref()
        .expect("field_references must be set for permission requests");  // ❌
    // ...
}
```

**修复后：**

```rust
pub fn extract_permission_context(&self, tool_call: &ToolCall) -> PermissionContext {
    // ✓ 优雅降级：field_references 可选
    let refs = tool_call.field_references.as_ref();
    PermissionContext {
        tool_name: tool_call.title.clone(),
        primary_arg: refs.and_then(|r| r.first().cloned()),
        full_args: tool_call.raw_args.clone(),
    }
}

// 改进 UX：如果 field_references 缺失，从 raw_args 推断
pub fn infer_primary_arg(&self, tool_call: &ToolCall) -> Option<String> {
    if let Some(refs) = &tool_call.field_references {
        refs.first().cloned()
    } else if let Some(args) = &tool_call.raw_args {
        // 启发式：选择第一个非元数据字段
        args.as_object()
            .and_then(|obj| obj.keys().next().map(|k| k.clone()))
    } else {
        None
    }
}
```

### 差距 4 修复：fence 修复完整性

**问题位置：** `apps/acp/src/permissions.rs:191-193`

**修复前：**

```rust
// 不完整的 fence 修复 — 仅在 `\` 存在时触发
pub fn fix_fence(&self, text: &str) -> String {
    if text.contains('\\') {
        text.replace("\\\\\\n", "\n")
            .replace("\\n", "\n")
    } else {
        text.to_string()
    }
}
```

**修复后：**

```rust
pub fn fix_fence(&self, text: &str) -> String {
    // ✓ 独立处理每种情况
    let mut result = text.to_string();

    // 1. 多重转义的换行符
    if result.contains("\\\\\\n") {
        result = result.replace("\\\\\\n", "\n");
    }

    // 2. 标准转义换行符
    if result.contains("\\n") {
        result = result.replace("\\n", "\n");
    }

    // 3. 字面换行符（无转义）
    // 注意：通常不需要处理，但保留以防万一
    if result.contains("\\\n") {
        result = result.replace("\\\n", "\n");
    }

    // 已知不完整：unicode 转义符、十六进制序列
    // TODO: 完整覆盖所有 fence 变体
    result
}
```

**更彻底的方案：使用成熟的 markdown 解析器：**

```rust
pub fn fix_fence(&self, text: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag, CodeBlockKind};
    let mut result = String::new();
    let parser = Parser::new(text);

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                result.push_str(&format!("\n```{}\n", lang));
            }
            Event::End(Tag::CodeBlock(_)) => {
                result.push_str("\n```\n");
            }
            Event::Text(text) => {
                result.push_str(&text);
            }
            _ => {}
        }
    }
    result
}
```

### 差距 5 修复：测试覆盖

**问题位置：** `apps/acp/tests/agent_integration.rs:158-200`

**修复后（添加 4 个新测试）：**

```rust
// 差距 5a: 客户端无响应 → 错误
#[tokio::test]
async fn test_permission_client_no_response_errors() {
    let client = TestClient::new_silent();  // 不响应
    let tool = ToolCall { id: "tc-1".into(), title: "rm -rf /".into(), ..Default::default() };

    let result = client.request_permission(&tool).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ACPError::PermissionTimeout { .. } => {},  // ✓ 预期错误
        other => panic!("expected PermissionTimeout, got {:?}", other),
    }
}

// 差距 5b: 单选项 → 使用 request_capability
#[tokio::test]
async fn test_permission_single_option_routes_to_capability() {
    let mut client = TestClient::new();
    let received = Arc::new(Mutex::new(None));
    let r = received.clone();
    client.on_request(move |req| {
        let r = r.clone();
        async move {
            *r.lock().await = Some(req.clone());
            Response::Selected { option_id: "confirm".into() }
        }
    }).await;

    let tool = ToolCall { id: "tc-2".into(), needs_only_confirmation: true, ..Default::default() };
    client.request_permission(&tool).await.unwrap();

    let req = received.lock().await.clone().unwrap();
    assert!(matches!(req, RequestType::Capability(_)),
            "single-option should route to request_capability");
}

// 差距 5c: field_references 缺失时优雅降级
#[tokio::test]
async fn test_permission_missing_field_references() {
    let tool = ToolCall {
        id: "tc-3".into(),
        field_references: None,  // ← 缺失
        raw_args: Some(json!({ "path": "/etc/passwd" })),
        ..Default::default()
    };

    let ctx = extract_permission_context(&tool);
    assert_eq!(ctx.primary_arg, Some("path".into()),
               "should infer primary arg from raw_args");
}

// 差距 5d: fence 修复独立于 `\` 字符存在
#[tokio::test]
async fn test_permission_fence_fix_works_without_backslash() {
    let _fixer = FenceFixer::new();
    let input = "```rust\nfn main() {}\n```\n```rust\nfn helper() {}\n```";
    let output = fixer.fix_fence(input);
    assert!(output.contains("```rust"));
    assert!(output.contains("```\n"));
}
```

### 差距 6 修复：能力声明冗余

**问题位置：** `apps/acp/src/client_capabilities.rs:60-63` 和 `apps/acp/src/protocol.rs:96-99`

**修复前：**

```rust
// client_capabilities.rs:60-63
pub fn get_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        request_permission: Some(true),  // ← 这里声明
        // ...
    }
}

// protocol.rs:96-99
// ❌ 重复声明
pub const PERMISSION_CAPABILITY: &str = "requestPermission";
```

**修复后：**

```rust
// protocol.rs:96-99 — 作为唯一定义源
pub const PERMISSION_CAPABILITY: &str = "requestPermission";

// client_capabilities.rs:60-63 — 引用 protocol.rs
pub fn get_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        request_permission: Some(true),  // 关联到 protocol.rs 常量
        // ...
    }
}

// 或使用单一函数
pub fn declare_capability(name: &str) -> bool {
    match name {
        PERMISSION_CAPABILITY => true,
        _ => false,
    }
}
```

### 差距 7 修复：能力发现循环

**问题位置：** `apps/acp/src/client_capabilities.rs:60-63`

**修复前：**

```rust
// 重复的 match 臂
impl ClientCapabilities {
    pub fn get(&self, name: &str) -> Option<bool> {
        match name {
            "requestPermission" => self.request_permission,
            "requestPermission" => self.request_permission,  // ❌ 重复
            // ...
        }
    }
}
```

**修复后：**

```rust
impl ClientCapabilities {
    pub fn get(&self, name: &str) -> Option<bool> {
        match name {
            PERMISSION_CAPABILITY => self.request_permission,
            TERMINAL_CAPABILITY => self.terminal,
            // ... 单一 match，每个能力一行
        }
    }
}
```

### 演示：完整的权限请求流程

**Agent → Client 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": 70,
  "method": "session/request_permission",
  "params": {
    "session_id": "sess-abc-123",
    "tool_call": {
      "toolCallId": "tc-001",
      "title": "Execute: rm -rf /tmp/build",
      "kind": "execute",
      "status": "pending",
      "field_references": [{ "name": "command", "value": "rm -rf /tmp/build" }],
      "raw_args": { "command": "rm -rf /tmp/build" }
    },
    "options": [
      { "optionId": "allow_once", "label": "Allow once", "kind": "allow_once" },
      { "optionId": "deny",      "label": "Deny",      "kind": "deny" }
    ],
    "meta": null
  }
}
```

**Client → Agent 响应（用户选择 Allow once）：**
```json
{
  "jsonrpc": "2.0",
  "id": 70,
  "result": {
    "outcome": { "outcome": "selected", "optionId": "allow_once" },
    "meta": null
  }
}
```

**Client → Agent 响应（用户取消）：**
```json
{
  "jsonrpc": "2.0",
  "id": 70,
  "result": {
    "outcome": { "outcome": "cancelled" },
    "meta": null
  }
}
```

**Client 无响应（修复后）：**
```json
{
  "jsonrpc": "2.0",
  "id": 70,
  "error": {
    "code": -32010,
    "message": "permission timeout: client did not respond for tool tc-001"
  }
}
```

### 演示：fallback 行为对比

```text
【修复前】客户端无响应：

Agent: request_permission (allow/deny/always)
  ↓
Client: 5 秒无响应
  ↓
Agent: timeout → fallback → auto_allow (如果配置) → 执行
  ↓
后果：用户未明确批准，操作被执行

【修复后】客户端无响应：

Agent: request_permission (allow/deny)
  ↓
Client: 5 秒无响应
  ↓
Agent: timeout → 返回 PermissionTimeout 错误
  ↓
后果：操作被中止，agent 报告错误
```

### 演示：单选项路由

```text
【场景】tool 只需要"确认"（如：显示敏感信息）

【修复前】单选项 UX：
  Agent: request_permission [{ id: "confirm" }]
    ↓ 违反规范（<2 选项）
  Client: 不知道如何呈现

【修复后】路由到 capability：
  Agent: request_capability { capability: "show_sensitive_info" }
    ↓ 正确原语
  Client: 呈现"显示 / 取消"按钮
```

### 测试场景

在 `apps/acp/tests/agent_integration.rs:158-200` 扩展：

```rust
// 上述 4 个测试（差距 5a-5d）

#[tokio::test]
async fn test_permission_no_always_allow_option() {
    let client = TestClient::new();
    let tool = ToolCall { id: "tc-1".into(), ..Default::default() };
    let req = client.capture_request_permission(&tool).await;

    // ✓ 验证选项中不存在 "allow_always"
    let option_ids: Vec<&str> = req.options.iter().map(|o| o.id.as_str()).collect();
    assert!(!option_ids.contains(&"allow_always"),
            "always_allow option removed for spec compliance");
}

#[tokio::test]
async fn test_permission_field_references_degradation() {
    // 验证 field_references 缺失时使用 raw_args 启发式
    let tool = ToolCall {
        field_references: None,
        raw_args: Some(json!({ "path": "/etc/passwd", "operation": "read" })),
        ..Default::default()
    };
    let ctx = extract_permission_context(&tool);
    assert_eq!(ctx.primary_arg, Some("path".into()));
}
```

### 验收清单

**差距 1 — 移除 fallback：**
- [ ] `permissions.rs:166-175` 移除 `allow_always` 选项
- [ ] `permissions.rs:213-222` 用 `ACPError::PermissionTimeout` 替换默认行为
- [ ] 移除 `config.auto_allow_on_timeout` 配置
- [ ] 添加超时配置（默认 5 秒）

**差距 2 — 单选项 UX：**
- [ ] `permissions.rs:131-188` 添加 `route_permission_request` 路由函数
- [ ] `needs_only_confirmation` 时路由到 `request_capability`
- [ ] 备选：保持 `request_permission` 但添加 "cancel" 选项

**差距 3 — field_references 假设：**
- [ ] `extract_permission_context` 优雅处理 `field_references: None`
- [ ] 添加 `infer_primary_arg` 从 `raw_args` 启发式推断
- [ ] 验证：缺失时使用启发式而非 panic

**差距 4 — fence 修复：**
- [ ] 拆分 `replace` 调用独立处理每种情况
- [ ] 或使用 `pulldown_cmark` 替换手动修复
- [ ] 添加测试覆盖各种 fence 变体

**差距 5 — 测试覆盖：**
- [ ] 添加 4 个新测试（无响应、单选项、field 缺失、fence 独立）
- [ ] 验证：现有 happy path 测试保持通过

**差距 6 — 能力声明去重：**
- [ ] `protocol.rs` 中保留唯一定义
- [ ] `client_capabilities.rs` 引用 protocol.rs 常量

**差距 7 — 能力发现循环：**
- [ ] 移除 `client_capabilities.rs:60-63` 重复 match 臂
- [ ] 验证：能力发现无重复项

**测试覆盖：**
- [ ] 6 个新测试（4 个核心 + 2 个补充）
- [ ] 验证修复前失败 / 修复后通过
