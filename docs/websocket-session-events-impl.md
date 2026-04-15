# WebSocket 会话实时事件 - 服务端实现指南

## 概述
本文档描述了服务端需要实现的 WebSocket 协议，以便支持前端会话列表的实时更新。

## WebSocket 消息协议

### 消息格式
所有消息使用 JSON 格式，通过 WebSocket 文本帧发送：

```json
{
  "type": "session_created",
  "workspace_id": "workspace-uuid",
  "session_id": "session-uuid",
  "session_name": "可选会话名称",
  "created_at": "2024-01-15T10:30:00.000Z"
}
```

### 事件类型

#### 1. 会话创建事件 (session_created)
当新会话被创建时，服务端需要向所有订阅该工作区的客户端广播此消息。

**触发时机：**
- 用户创建新会话
- 系统/API 创建新会话
- 从另一个工作区导入会话时

**消息格式：**
```json
{
  "type": "session_created",
  "workspace_id": "ws_abc123",
  "session_id": "sess_xyz789",
  "session_name": "新的对话",
  "created_at": "2024-01-15T10:30:00.000Z"
}
```

**字段说明：**
- `type`: "session_created" (固定值)
- `workspace_id`: 工作区 ID，客户端会筛选属于自己的工作区
- `session_id`: 新创建的会话唯一 ID
- `session_name`: [可选] 会话显示名称
- `created_at`: ISO 8601 格式的时间戳

#### 2. 会话更新事件 (session_updated)
当会话名称被修改时触发。

**触发时机：**
- 用户重命名会话
- 系统自动更新会话标题（如基于首条消息）

**消息格式：**
```json
{
  "type": "session_updated",
  "workspace_id": "ws_abc123",
  "session_id": "sess_xyz789",
  "session_name": "更新后的名称",
  "updated_at": "2024-01-15T10:35:00.000Z"
}
```

#### 3. 会话删除事件 (session_deleted)
当会话被删除时触发。

**触发时机：**
- 用户手动删除会话
- 系统自动清理过期会话
- 会话被归档（取决于后端策略）

**消息格式：**
```json
{
  "type": "session_deleted",
  "workspace_id": "ws_abc123",
  "session_id": "sess_xyz789"
}
```

## 后端实现示例

### Rust 实现思路

```rust
// 1. 定义事件结构
#[derive(Debug, Clone, Serialize)]
pub struct SessionEvent {
    pub r#type: String,
    pub workspace_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// 2. 广播事件的方法
pub async fn broadcast_session_created(
    &self,
    workspace_id: &str,
    session_id: &str,
    session_name: Option<&str>,
) {
    let event = SessionEvent {
        r#type: "session_created".to_string(),
        workspace_id: workspace_id.to_string(),
        session_id: session_id.to_string(),
        session_name: session_name.map(|s| s.to_string()),
        created_at: Some(Utc::now().to_rfc3339()),
        updated_at: None,
    };
    
    // 广播给所有订阅该工作区的连接
    self.broadcast_to_workspace(workspace_id, event).await;
}

// 3. 创建会话时的调用
pub async fn create_session(
    &self,
    workspace_id: &str,
    session_name: Option<&str>,
) -> Result<Session, Error> {
    // ... 创建会话逻辑 ...
    let session = self.db.create_session(workspace_id, session_name).await?;
    
    // 广播事件
    self.broadcast_session_created(
        workspace_id,
        &session.id,
        session_name,
    ).await;
    
    Ok(session)
}
```

### 广播策略

1. **范围控制**: 只广播给与事件工作区关联的连接
2. **并发处理**: 使用异步广播，不阻塞创建操作
3. **错误处理**: 广播失败不应影响会话创建成功
4. **连接过滤**: 客户端只处理自己当前工作区的事件

```rust
// 广播到特定工作区的所有连接
async fn broadcast_to_workspace<T: Serialize>(
    &self,
    workspace_id: &str,
    message: T,
) {
    let json = match serde_json::to_string(&message) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize message: {}", e);
            return;
        }
    };
    
    // 获取订阅该工作区的所有连接
    let connections = self.get_workspace_connections(workspace_id).await;
    
    // 并发发送给所有连接
    let futures = connections.iter().map(|conn| {
        let msg = json.clone();
        async move {
            if let Err(e) = conn.send(Message::Text(msg)).await {
                eprintln!("Failed to send to connection: {}", e);
            }
        }
    });
    
    // 等待所有发送完成（但忽略单个失败）
    let _ = futures::future::join_all(futures).await;
}
```

## 连接管理建议

### 1. 连接订阅注册
当客户端连接并指定工作区时，服务器应记录该连接属于哪个工作区：

```rust
struct ClientConnection {
    ws_stream: WebSocketStream<TcpStream>,
    workspace_id: Option<String>, // 客户端订阅的工作区
    user_id: String,
}
```

### 2. 多工作区支持
如果用户属于多个工作区，连接应有能力切换或订阅多个工作区：

```rust
struct ClientConnection {
    ws_stream: WebSocketStream<TcpStream>,
    subscribed_workspaces: Vec<String>, // 订阅的多个工作区
    user_id: String,
}
```

### 3. 断线重连处理
- 保持连接状态，短暂断开后重连时恢复订阅
- 客户端重连后可能需要请求完整会话列表同步

## 测试验证

### 使用 wscat 测试

```bash
# 安装 wscat
npm install -g wscat

# 连接到服务器
wscat -c ws://localhost:8080

# 订阅特定工作区（通过 message）
> {"type": "subscribe_workspace", "workspace_id": "ws_abc123"}

# 当服务器发送 session_created 事件时，应看到：
< {"type":"session_created","workspace_id":"ws_abc123","session_id":"sess_new","...":"..."}
```

### 集成测试要点

1. **基本功能测试**
   - 创建会话 → 验证所有连接的客户端收到事件
   - 更新会话 → 验证事件正确传播
   - 删除会话 → 验证客户端从列表移除

2. **边界情况**
   - 断网后重连应能恢复同步
   - 跨工作区不应收到其他工作区的事件
   - 高频创建应能正确处理（不丢消息）

3. **并发测试**
   - 多个客户端同时创建会话
   - 大量会话创建的性能测试

## 安全注意事项

1. **权限验证**: 确保只向有权限的用户广播事件
2. **工作区隔离**: 用户不应收到其他工作区的会话事件
3. **敏感信息**: 不要通过 WebSocket 发送敏感会话内容

## 性能优化建议

1. **批量处理**: 如果短时间内有大量事件，考虑批量发送
2. **压缩**: 对于大量数据传输，启用 WebSocket per-message deflate
3. **心跳机制**: 保持连接活跃，检测僵尸连接

```rust
// 示例：启用压缩
let ws_config = WebSocketConfig {
    max_send_queue: None,
    max_message_size: Some(64 << 20), // 64MB
    max_frame_size: Some(16 << 20),   // 16MB
    accept_unmasked_frames: false,
};
```

## 前端兼容性

前端已经实现了：
- ✅ WebSocket 连接管理（connection.ts）
- ✅ 事件解析和分发
- ✅ 会话列表状态自动更新（useRealtimeSessions hook）
- ✅ ChatPage 集成

后端只需要按照本文档实现事件广播即可。

## 相关文件

- `web/src/types/protocol/loom.ts` - 协议类型定义
- `web/src/services/connection.ts` - WebSocket 连接和事件处理
- `web/src/hooks/useRealtimeSessions.ts` - 实时会话 Hook
- `web/src/pages/ChatPage.tsx` - 页面集成