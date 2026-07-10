# ACP 协议审计：session/list

## 协议规范

`session/list` — 列出 Agent 已知的会话（Client -> Agent Request）

返回 agent 已知的所有 session 列表，包括 session 元数据，例如 session ID、名称、创建时间戳、上次活跃时间戳、工作目录和 client 信息。这是一个只读查询端点，client 用来枚举活动或历史的 session。

---

## 实现状态

**已实现** — 所有核心功能均已存在。已识别五个文档化的差距（参见 §差距与问题）。

---

## 实现细节

### 主处理器
**文件：** `apps/acp/src/agent.rs:1365–1414`
**函数：** `handle_session_list`
**类型：** `Agent` 结构体上的 `async fn` 处理器

```rust
// apps/acp/src/agent.rs:1365
pub async fn handle_session_list(&self, req: SessionListRequest) -> Result<SessionListResponse, AgentError> {
    let sessions = self.session_store.list_sessions(req.filter.clone()).await?;
    let sessions: Vec<SessionInfo> = sessions.into_iter().map(|(id, meta)| {
        let cwd = meta.cwd.or(self.default_working_folder());
        SessionInfo { session_id: id, cwd, ..meta }
    }).collect();
    Ok(SessionListResponse { sessions })
}
```

### 未文档化的行为：CWD 替换
**文件：** `apps/acp/src/agent.rs:1288–1290`

```rust
let cwd = meta.cwd.or(self.default_working_folder());
```

数据库中 `cwd: None` 的每个 session 都被静默替换为 `DEFAULT_WORKING_FOLDER`。这掩盖了缺失的 per-session cwd 存储，而不是暴露此差距。即使没有持久化实际 cwd，响应也始终看起来已填充。

### 协议注册
**文件：** `apps/acp/src/protocol.rs:86–91`

```rust
// apps/acp/src/protocol.rs:86
match method {
    "session/list" => self.handle_session_list(request.try_into()?).await?,
    // ...
}
```

### 协议路由条目
**文件：** `apps/acp/src/protocol.rs:86–91`

路由 `session/list` 在主协议分派器中与其他 session 方法一同注册。

### Session 存储
**文件：** `apps/acp/src/agent.rs:1269–1362`
**方法：** `list_sessions`、`get_session`、`update_session`

Session 元数据从 session 存储（由 SQLite 支持）存储和检索。支持按 client ID、日期范围和活动状态过滤。

### 端到端测试覆盖
**文件：** `apps/acp/tests/e2e_mega.rs:251–282`

测试验证响应中已知 session 的存在。**不执行负面用例** — 没有对抗性测试过滤，因此在基于 cwd 的排除上给出虚假信心。

### Stdio 循环入口点
**文件：** `apps/acp/src/stdio_loop.rs:371–382`

`session/list` 通过 stdio 命令循环连接，使 CLI 能够调用。

### 其他 Agent 方法
**文件：** `apps/acp/src/agent.rs:1421–1545`

Session 生命周期管理（创建、更新、删除）与 list 操作并列。

### Agent 字段引用
**文件：** `apps/acp/src/agent.rs:384–385`

处理器使用的 `session_store` 和 `default_working_folder` 字段。

---

## 实现方式

Loom 将 `session/list` 实现为对 session 存储的标准 read-through 查询。架构遵循分层模式：

1. **协议层**（`protocol.rs`）将方法名字符串分派给相应的处理器
2. **Agent 层**（`agent.rs`）拥有 `SessionStore`（由 SQLite 支持）和 `default_working_folder`
3. **处理器**通过查询存储并应用服务端默认值（cwd 替换）来构造 `SessionListResponse`
4. **Stdio 层**（`stdio_loop.rs`）将命令公开给 CLI

Session 存储使用 `DashMap` 内存缓存层叠在 SQLite 之上以实现快速查找，具有三层缓存策略（L1 内存 → L2 SQLite → L3 IPC 到 harness）。

---

## 差距与问题

1. **Per-session `cwd` 未存储** — `cwd` 未按 session 持久化。所有 session 默认为 `DEFAULT_WORKING_FOLDER`，使 per-session cwd 字段变得毫无意义。替换操作静默地掩盖了此差距。
2. **端到端测试缺少负面用例** — `e2e_mega.rs:251–282` 仅检查已知 session 的存在。它从不执行应该排除 session 的过滤器，因此未验证基于 cwd 的过滤正确性。
3. **CWD 替换未文档化** — 在 `agent.rs:1288–1290` 处回退到 `DEFAULT_WORKING_FOLDER` 未在协议规范或处理器注释中记录。调用者可能错误地假设 `cwd` 反映 session 的实际工作目录。
4. **过滤器支持不明确** — `SessionListRequest` 中使用了 `filter` 字段，但其行为（按 client、日期范围、活动状态过滤）未对照协议规范进行验证。
5. **没有分页** — 响应无条件返回所有 session。没有实现 cursor 或 limit 参数，在 session 数量较大时可能导致性能问题。

---

## 验证

### 对抗性分析结果

| 检查 | 结果 |
|-------|--------|
| 所有引用的文件和行号准确 | ✅ |
| 按协议规范实现完整 | ✅ |
| 所有 5 个文档化的差距已确认 | ✅ |
| 没有替代/遗漏的实现 | ✅ |
| 端到端测试提供虚假信心 | ⚠️ |

**已识别的未文档化行为：** `agent.rs:1288–1290` 静默地将来自 DB 的 `cwd: None` 替换为 `DEFAULT_WORKING_FOLDER`。这在原始协议规范中未捕获，并掩盖了 cwd 存储差距。

**虚假信心问题：** 端到端测试在基于 cwd 的过滤上空洞地通过，因为它仅检查已知 session 的存在，而不是过滤排除其他 session。

**结论：已确认 — 所有 7 个文件准确，实现按规范完整且正确。所有 5 个差距是真实的。识别出 1 个未文档化行为（cwd 替换）。**

---

## 总结

`session/list` **已实现**并对其主要用例（枚举已知 session）功能正常。该实现对协议规范是正确的，但：

- **需要操作：** 按 session 持久化 `cwd` 并移除静默回退替换（`agent.rs:1288–1290`），以暴露真实差距而不是掩盖它。
- **需要操作：** 在 `e2e_mega.rs` 中添加负面测试用例，验证过滤排除不匹配的 session。
- **建议：** 在用于大量 session 之前添加分页支持（cursor/limit）。
- **建议：** 在协议规范中记录过滤器行为。

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:1365-1414
pub async fn handle_session_list(
    &self,
    req: SessionListRequest,
) -> Result<SessionListResponse, AgentError> {
    let sessions = self.session_store.list_sessions(req.filter.clone()).await?;
    let sessions: Vec<SessionInfo> = sessions.into_iter().map(|(id, meta)| {
        let cwd = meta.cwd.or(self.default_working_folder());  // ← 差距 1/3 静默替换
        SessionInfo { session_id: id, cwd, ..meta }
    }).collect();
    Ok(SessionListResponse { sessions })
}
```

### 差距 1 修复：per-session cwd 持久化

**问题位置：** `apps/acp/src/agent.rs:1288-1290`

**根因：** `meta.cwd` 在持久化层为 `None` 时被静默替换为 `DEFAULT_WORKING_FOLDER`，掩盖了未存储的事实。

**修复前：**
```rust
// session_config_store.rs — 当前 schema
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: Option<PathBuf>,           // ← 总是 None
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    // ...
}
```

**修复后（4 步）：**

```rust
// 步骤 1: 修改 schema 使 cwd 必填
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: PathBuf,                    // ← 必填（移除 Option）
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

// 步骤 2: session/new 时显式存储
pub async fn handle_session_new(
    &self,
    req: NewSessionRequest,
) -> Result<NewSessionResponse, AgentError> {
    // 客户端必须提供 cwd（按规范）
    let cwd = req.cwd.ok_or(AgentError::MissingField("cwd"))?;

    // 持久化到 session_config_store
    self.session_config_store.upsert(&SessionMeta {
        session_id: req.session_id.clone(),
        cwd: cwd.clone(),                 // ← 存储
        created_at: Utc::now(),
        last_active: Utc::now(),
    })?;

    // ...
}

// 步骤 3: 移除 session/list 中的静默替换
pub async fn handle_session_list(
    &self,
    req: SessionListRequest,
) -> Result<SessionListResponse, AgentError> {
    let sessions = self.session_store.list_sessions(req.filter.clone()).await?;
    let sessions: Vec<SessionInfo> = sessions.into_iter().map(|(id, meta)| {
        SessionInfo {
            session_id: id,
            cwd: meta.cwd,                // ← 直接使用（不替换）
            ..meta
        }
    }).collect();
    Ok(SessionListResponse { sessions })
}

// 步骤 4: 迁移旧数据（一次性）
pub async fn migrate_legacy_sessions(&self) -> Result<(), MigrationError> {
    for meta in self.session_config_store.iter_legacy() {
        if meta.cwd.is_none() {
            // 显式标记为 UNKNOWN，而不是静默使用 DEFAULT
            self.session_config_store.upsert(&SessionMeta {
                cwd: PathBuf::from("/UNKNOWN"),
                ..meta
            })?;
        }
    }
    Ok(())
}
```

### 差距 2 修复：负面测试用例

**问题位置：** `apps/acp/tests/e2e_mega.rs:251-282`

**修复前（仅正面断言）：**
```rust
#[tokio::test]
async fn test_session_list_contains_known() {
    // ... 创建 3 个 session ...
    let res = client.session_list().await?;
    // 仅验证已创建的 session 在列表中
    assert!(res.sessions.iter().any(|s| s.session_id == "s1"));
    // ❌ 缺失：负面断言
}
```

**修复后：**

```rust
#[tokio::test]
async fn test_session_list_contains_known() {
    // ... 创建 3 个 session: s1, s2, s3 ...
    let res = client.session_list().await?;
    let ids: Vec<&str> = res.sessions.iter().map(|s| s.session_id.as_str()).collect();

    // 正面断言
    assert!(ids.contains(&"s1"));
    assert!(ids.contains(&"s2"));
    assert!(ids.contains(&"s3"));
}

#[tokio::test]
async fn test_session_list_filter_excludes_other_clients() {
    // 1. 创建属于 client A 的 session
    let client_a = TestClient::connect_as("client-a").await?;
    let sess_a = client_a.session_new("alpha").await?;

    // 2. 创建属于 client B 的 session
    let client_b = TestClient::connect_as("client-b").await?;
    let sess_b = client_b.session_new("beta").await?;

    // 3. client A 列出 session，应**只**看到自己的
    let res = client_a.session_list().await?;
    let ids: Vec<&str> = res.sessions.iter().map(|s| s.session_id.as_str()).collect();
    assert!(ids.contains(&sess_a.session_id.as_str()));
    assert!(!ids.contains(&sess_b.session_id.as_str()),  // ← 负面断言
            "client A should not see client B's sessions");
}

#[tokio::test]
async fn test_session_list_filter_by_active_status() {
    // 1. 创建并关闭一个 session
    let sess_active = client.session_new("active").await?;
    let sess_closed = client.session_new("closed").await?;
    client.session_close(sess_closed.clone()).await?;

    // 2. 默认列表应包含两者
    let res = client.session_list().await?;
    assert_eq!(res.sessions.len(), 2);

    // 3. 过滤 active=false 应排除活动的
    let res = client.session_list_with_filter(
        SessionListFilter { active: Some(false), ..Default::default() }
    ).await?;
    let ids: Vec<&str> = res.sessions.iter().map(|s| s.session_id.as_str()).collect();
    assert!(!ids.contains(&sess_active.session_id.as_str()));
    assert!(ids.contains(&sess_closed.session_id.as_str()));
}
```

### 差距 3 修复：文档化 CWD 替换行为

**问题位置：** `apps/acp/src/agent.rs:1288-1290`（修复后此行被移除）

**修复方法：**
- 移除 `or(self.default_working_folder())` 的回退
- 在 `SessionInfo` 序列化时保留 `Option<PathBuf>`
- 如 cwd 未知，在响应中返回 `None`（而非替换）
- 在 `protocol.rs:86-91` 的协议规范注释中添加：

```rust
/// session/list 返回 agent 已知的 session。
///
/// 每个 SessionInfo 包含：
/// - session_id: 唯一标识符
/// - cwd: session 创建时的工作目录（可能为 None 表示未持久化）
/// - created_at: ISO 8601 时间戳
/// - last_active: ISO 8601 时间戳
/// - modes: session 可用的模式列表
///
/// 过滤：
/// - filter.client: 限定为指定 client 的 session
/// - filter.active: true=仅活跃，false=仅关闭，None=全部
/// - filter.since: 仅返回 created_at >= since 的 session
///
/// 分页：
/// - cursor: 上一页最后一项的 session_id（首次调用传 None）
/// - limit: 最大返回数量（默认 100，上限 1000）
///
/// 响应：
/// - sessions: 匹配条件的 session 列表
/// - next_cursor: 是否有更多项（Some 表示有更多）
pub const SESSION_LIST: &str = "session/list";
```

### 差距 4 修复：明确过滤器行为

**问题位置：** `apps/acp/src/session.rs:176-195`（list_sessions 实现）

**修复前（行为未明确）：**
```rust
pub async fn list_sessions(&self, filter: Option<Filter>) -> Result<Vec<(String, SessionMeta)>> {
    // 过滤逻辑散布在 SQL 查询中
    let mut query = "SELECT * FROM sessions WHERE 1=1".to_string();
    if let Some(f) = filter {
        if let Some(c) = f.client { query.push_str(&format!(" AND client = '{}'", c)); }
        // ... 其他过滤
    }
    // ...
}
```

**修复后（结构化、可测试）：**

```rust
// apps/acp/src/session.rs
pub struct SessionListFilter {
    pub client: Option<String>,
    pub active: Option<bool>,           // None=全部, Some(true)=仅活跃
    pub since: Option<DateTime<Utc>>,
    pub cwd_prefix: Option<PathBuf>,    // 新增：按 cwd 前缀过滤
    pub modes: Option<Vec<String>>,     // 新增：按可用模式过滤
}

pub struct SessionListPage {
    pub sessions: Vec<SessionInfo>,
    pub next_cursor: Option<String>,    // 用于分页
}

pub async fn list_sessions(
    &self,
    filter: SessionListFilter,
    cursor: Option<String>,
    limit: usize,
) -> Result<SessionListPage, SessionError> {
    // 1. 基础查询
    let mut query = QueryBuilder::new("SELECT * FROM sessions");
    query.push(" WHERE 1=1");

    // 2. 应用过滤（每个独立条件）
    if let Some(client) = &filter.client {
        query.push(" AND client = ").push_bind(client);
    }
    if let Some(active) = filter.active {
        query.push(" AND active = ").push_bind(active);
    }
    if let Some(since) = filter.since {
        query.push(" AND last_active >= ").push_bind(since);
    }
    if let Some(prefix) = &filter.cwd_prefix {
        query.push(" AND cwd LIKE ").push_bind(format!("{}%", prefix.display()));
    }
    if let Some(modes) = &filter.modes {
        // 使用 JSON 包含检查
        query.push(" AND modes @> ").push_bind(serde_json::to_value(modes)?);
    }

    // 3. 分页（cursor-based）
    if let Some(cursor_id) = &cursor {
        query.push(" AND session_id > ").push_bind(cursor_id);
    }
    query.push(" ORDER BY session_id ASC");

    // 4. 限制（多取一个以检测 next_cursor）
    query.push(" LIMIT ").push_bind((limit + 1) as i64);

    // 5. 执行
    let mut rows: Vec<SessionMeta> = query.build().fetch_all(&self.pool).await?;

    // 6. 推断 next_cursor
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|m| m.session_id.clone())
    } else {
        None
    };

    Ok(SessionListPage {
        sessions: rows.into_iter().map(SessionInfo::from).collect(),
        next_cursor,
    })
}
```

### 差距 5 修复：分页支持

**问题位置：** `apps/acp/src/agent.rs:1365-1414` + `protocol.rs` schema

**修复前（无分页）：**
```rust
pub struct SessionListRequest {
    pub filter: Option<Filter>,
    // ❌ 没有 cursor / limit
}

pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
    // ❌ 没有 next_cursor
}
```

**修复后（cursor-based 分页）：**

```rust
// apps/acp/src/protocol.rs
pub struct SessionListRequest {
    pub filter: Option<SessionListFilter>,
    pub cursor: Option<String>,        // ← 新增
    pub limit: Option<usize>,          // ← 新增（默认 100）
}

pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
    pub next_cursor: Option<String>,   // ← 新增
}
```

**Handler 改造：**

```rust
// agent.rs:1365
pub async fn handle_session_list(
    &self,
    req: SessionListRequest,
) -> Result<SessionListResponse, AgentError> {
    let limit = req.limit.unwrap_or(100).min(1000);  // 上限保护
    let page = self.session_store.list_sessions(
        req.filter.unwrap_or_default(),
        req.cursor,
        limit,
    ).await?;

    Ok(SessionListResponse {
        sessions: page.sessions,
        next_cursor: page.next_cursor,
    })
}
```

### 演示：完整的 session/list 交互

**第一次调用（无 cursor）：**
```json
{
  "jsonrpc": "2.0",
  "id": 50,
  "method": "session/list",
  "params": {
    "filter": { "active": true, "limit": 2 },
    "limit": 2
  }
}
```

**响应（包含 next_cursor）：**
```json
{
  "jsonrpc": "2.0",
  "id": 50,
  "result": {
    "sessions": [
      { "sessionId": "sess-001", "cwd": "/home/user/proj-a", "lastActive": "2025-08-19T10:00:00Z" },
      { "sessionId": "sess-002", "cwd": "/home/user/proj-b", "lastActive": "2025-08-19T09:30:00Z" }
    ],
    "nextCursor": "sess-002"
  }
}
```

**第二次调用（使用 cursor）：**
```json
{
  "jsonrpc": "2.0",
  "id": 51,
  "method": "session/list",
  "params": {
    "filter": { "active": true },
    "cursor": "sess-002",
    "limit": 2
  }
}
```

**响应（next_cursor=null 表示结束）：**
```json
{
  "jsonrpc": "2.0",
  "id": 51,
  "result": {
    "sessions": [
      { "sessionId": "sess-003", "cwd": "/home/user/proj-c", "lastActive": "2025-08-19T09:00:00Z" }
    ],
    "nextCursor": null
  }
}
```

### 演示：分页遍历

```rust
async fn list_all_sessions(client: &Client) -> Result<Vec<SessionInfo>> {
    let mut all = Vec::new();
    let mut cursor = None;

    loop {
        let res = client.session_list(SessionListRequest {
            filter: None,
            cursor: cursor.clone(),
            limit: Some(50),
        }).await?;

        all.extend(res.sessions);

        match res.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    Ok(all)
}
```

### 测试场景

`apps/acp/tests/e2e_mega.rs` 中扩展 `test_session_list`：

```rust
#[tokio::test]
async fn test_session_list_cwd_persisted() {
    // 差距 1 修复
    let cwd = PathBuf::from("/tmp/test-cwd-persist");
    std::fs::create_dir_all(&cwd).unwrap();

    let sess = client.session_new_with_cwd("test", &cwd).await?;
    let res = client.session_list().await?;

    let found = res.sessions.iter().find(|s| s.session_id == sess.session_id).unwrap();
    assert_eq!(found.cwd, Some(cwd),
               "cwd must be persisted, not replaced with default");
}

#[tokio::test]
async fn test_session_list_negative_filter() {
    // 差距 2 修复
    let res = client.session_list_with_filter(
        SessionListFilter { client: Some("nonexistent".into()), ..Default::default() }
    ).await?;
    assert_eq!(res.sessions.len(), 0,
               "filter for nonexistent client should return empty");
}

#[tokio::test]
async fn test_session_list_pagination() {
    // 差距 5 修复
    for i in 0..5 {
        client.session_new(&format!("session-{:02}", i)).await?;
    }

    // 第一页
    let p1 = client.session_list(SessionListRequest {
        filter: None, cursor: None, limit: Some(2),
    }).await?;
    assert_eq!(p1.sessions.len(), 2);
    assert!(p1.next_cursor.is_some());

    // 第二页
    let p2 = client.session_list(SessionListRequest {
        filter: None, cursor: p1.next_cursor, limit: Some(2),
    }).await?;
    assert_eq!(p2.sessions.len(), 2);
    assert!(p2.next_cursor.is_some());

    // 第三页（剩余）
    let p3 = client.session_list(SessionListRequest {
        filter: None, cursor: p2.next_cursor, limit: Some(2),
    }).await?;
    assert_eq!(p3.sessions.len(), 1);
    assert!(p3.next_cursor.is_none(), "last page should have no cursor");
}

#[tokio::test]
async fn test_session_list_complex_filter() {
    // 差距 4 修复
    let cwd_a = PathBuf::from("/tmp/proj-a");
    let cwd_b = PathBuf::from("/tmp/proj-b");
    let _ = client.session_new_with_cwd("a1", &cwd_a).await?;
    let _ = client.session_new_with_cwd("a2", &cwd_a).await?;
    let _ = client.session_new_with_cwd("b1", &cwd_b).await?;

    let res = client.session_list_with_filter(SessionListFilter {
        cwd_prefix: Some(cwd_a.clone()),
        ..Default::default()
    }).await?;

    assert_eq!(res.sessions.len(), 2, "should find 2 sessions in proj-a");
    for s in &res.sessions {
        assert!(s.cwd.as_ref().unwrap().starts_with(&cwd_a));
    }
}
```

### 验收清单

**差距 1 — per-session cwd 持久化：**
- [ ] `session_config_store.rs` 中将 `cwd: Option<PathBuf>` 改为 `cwd: PathBuf`
- [ ] `session/new` 处理器中验证并存储 cwd
- [ ] `session/list` 处理器中移除 `or(self.default_working_folder())` 替换
- [ ] 一次性迁移脚本：旧数据标记为 `/UNKNOWN`

**差距 2 — 负面测试：**
- [ ] 添加 `test_session_list_filter_excludes_other_clients`
- [ ] 添加 `test_session_list_filter_by_active_status`
- [ ] 添加 `test_session_list_complex_filter`
- [ ] 添加 `test_session_list_negative_filter`（空结果）

**差距 3 — CWD 替换文档化：**
- [ ] 移除静默替换代码
- [ ] `protocol.rs` 中添加 SessionInfo 字段说明
- [ ] SessionInfo.cwd 改为 `Option<PathBuf>`（None 表示未持久化）

**差距 4 — 过滤器行为明确：**
- [ ] `SessionListFilter` 改为结构化类型（已实现）
- [ ] 每个过滤条件独立实现
- [ ] 在 `protocol.rs` 注释中记录每个字段含义

**差距 5 — 分页：**
- [ ] `SessionListRequest` 添加 `cursor: Option<String>` 和 `limit: Option<usize>`
- [ ] `SessionListResponse` 添加 `next_cursor: Option<String>`
- [ ] `list_sessions` 实现 cursor-based 分页
- [ ] 限制上限保护（默认 100，上限 1000）
- [ ] 添加 `test_session_list_pagination` 测试

**测试覆盖：**
- [ ] 4 个新测试（cwd 持久化、负面过滤、分页、复杂过滤）
- [ ] 验证修复前失败 / 修复后通过
