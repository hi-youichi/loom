# 故障排查指南

常见问题诊断和解决方案，帮助您快速定位和解决 Loom 框架使用中的问题。

## 问题概览

| 症状 | 可能原因 | 解决方案 |
|------|----------|----------|
| `ExecutionFailed: API key not found` | API 密钥未配置或无效 | 检查 `.env` 文件和环境变量 |
| `model not found: gpt-4` | 模型名称不正确或无访问权限 | 验证模型名称和账户权限 |
| `tool not found: search_web` | 工具未正确注册或配置 | 检查工具源配置和可用性 |
| `NodeNotFound: unknown_node` | 图结构中引用了不存在的节点 | 修正边连接或添加缺失节点 |
| `InvalidChain` | 图结构不是单一线性链 | 重新设计图结构确保线性流程 |
| `ThreadIdRequired` | 检查点操作需要 thread_id | 在 RunnableConfig 中设置 thread_id |
| `Serialization` | 状态无法序列化/反序列化 | 检查状态类型实现 Serialize/Deserialize |
| `MCP/transport error` | MCP 服务器连接失败 | 验证 MCP 服务器配置和网络连接 |
| `EmptyLlmResponse` | LLM 返回空响应 | 检查模型状态和重试配置 |
| `Cancelled` | 执行被取消 | 检查取消令牌和超时设置 |

## 详细故障排查步骤

### 1. API 密钥/认证错误

**常见错误信息**：
- `ExecutionFailed: API key not found`
- `Access denied, please make sure your account is in good standing. (code: Arrearage)`
- `Invalid API key provided`

**排查步骤**：

1. **检查环境变量**
```bash
# 验证 API 密钥是否设置
echo $OPENAI_API_KEY
echo $OPENAI_BASE_URL
```

2. **检查 .env 文件**
```bash
# 确保 .env 文件存在且格式正确
cat .env
# 应该包含类似内容：
# OPENAI_API_KEY=sk-...
# OPENAI_BASE_URL=https://api.openai.com/v1
```

3. **验证 API 密钥有效性**
```bash
# 使用 curl 测试 API 密钥
curl -X POST https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-3.5-turbo","messages":[{"role":"user","content":"test"}]}'
```

4. **检查配置文件**
```toml
# ~/.loom/config.toml
[env]
OPENAI_API_KEY = "sk-..."
OPENAI_BASE_URL = "https://api.openai.com/v1"
```

**解决方案**：
- 确保 API 密钥格式正确（通常以 `sk-` 开头）
- 检查账户余额和计费状态
- 验证 API 密钥权限和模型访问权限
- 确保环境变量在正确的进程中加载

### 2. 模型未找到错误

**常见错误信息**：
- `model not found: gpt-4`
- `The model `gpt-4` does not exist`
- `Model access denied`

**排查步骤**：

1. **验证模型名称**
```bash
# 检查可用模型列表
curl -X GET https://api.openai.com/v1/models \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

2. **检查模型配置**
```rust
// 确保使用正确的模型名称
let config = ReactBuildConfig {
    model: "gpt-4o".to_string(),  // 而不是 "gpt-4"
    ..Default::default()
};
```

3. **验证账户权限**
```rust
// 某些模型需要特殊权限
// 检查您的 OpenAI 账户是否有访问该模型的权限
```

**解决方案**：
- 使用正确的模型名称（如 `gpt-4o` 而不是 `gpt-4`）
- 升级到支持该模型的付费计划
- 检查地区限制和模型可用性
- 使用替代模型（如 `gpt-4o-mini`）

### 3. 工具执行失败

**常见错误信息**：
- `tool not found: search_web`
- `ToolError: execution timeout`
- `invalid arguments: missing required field 'query'`

**排查步骤**：

1. **检查工具注册**
```rust
// 确保工具已正确注册
let mut tool_source = Box::new(MemoryToolSource::new());
tool_source.add_tool(Arc::new(WeatherTool));

// 验证工具列表
let tools = tool_source.list_tools().await?;
println!("可用工具: {:?}", tools);
```

2. **检查工具定义**
```rust
// 确保工具定义正确
impl CustomTool for WeatherTool {
    fn name(&self) -> &str { "get_weather" }
    
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_weather".to_string(),
            description: "获取天气信息".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "城市名称"}
                },
                "required": ["city"]  // 确保必填字段正确
            }),
        }
    }
}
```

3. **检查工具执行**
```rust
// 测试工具直接调用
let result = tool_source.call_tool("get_weather", json!({"city": "北京"})).await?;
println!("工具结果: {:?}", result);
```

**解决方案**：
- 确保工具名称与定义完全一致
- 验证工具参数格式符合 JSON Schema
- 检查工具实现的错误处理
- 添加超时和重试机制

### 4. 图编译错误

**常见错误信息**：
- `NodeNotFound: unknown_node`
- `graph must have exactly one edge from START`
- `edges must form a single linear chain from START to END`
- `node has both edge and conditional edges: think`

**排查步骤**：

1. **检查节点注册**
```rust
let mut graph = StateGraph::<ReActState>::new();

// 确保所有节点都已注册
graph.add_node("think", Arc::new(think_node));
graph.add_node("act", Arc::new(act_node));
graph.add_node("observe", Arc::new(observe_node));

// 验证节点存在
// graph.add_edge("unknown", "act");  // 这会导致 NodeNotFound 错误
graph.add_edge("think", "act");
```

2. **检查图结构**
```rust
// 确保图是单一线性链
graph.add_edge("START", "think");   // 必须有且只有一个从 START 的边
graph.add_edge("think", "act");
graph.add_edge("act", "observe");
graph.add_edge("observe", "END");   // 必须有且只有一个到 END 的边
```

3. **检查条件边**
```rust
// 节点不能同时有普通边和条件边
// graph.add_edge("think", "act");
// graph.add_conditional_edges("think", HashMap::new());  // 这会导致冲突

// 选择一种连接方式
graph.add_edge("think", "act");
// 或者使用条件边
// graph.add_conditional_edges("think", path_map);
```

**解决方案**：
- 确保所有引用的节点都已通过 `add_node` 注册
- 验证图结构是单一线性链（无分支、循环或断开）
- 为每个节点选择一种连接方式（普通边或条件边）
- 使用 `compile()` 前验证图结构

### 5. 状态序列化错误

**常见错误信息**：
- `Serialization: data did not match any variant of untagged enum Message`
- `serialization: missing field `messages``
- `storage: database locked`

**排查步骤**：

1. **检查状态类型**
```rust
// 确保状态类型实现了 Serialize 和 Deserialize
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyState {
    pub messages: Vec<Message>,
    pub counter: i32,
}
```

2. **检查序列化兼容性**
```rust
// 测试状态序列化
let state = ReActState::default();
let serialized = serde_json::to_string(&state)?;
let deserialized: ReActState = serde_json::from_str(&serialized)?;
```

3. **检查检查点存储**
```rust
// 确保检查点器配置正确
let serializer = Arc::new(JsonSerializer);
let checkpointer = Arc::new(SqliteSaver::new(
    "./checkpoints.db",
    serializer,
)?);
```

**解决方案**：
- 为所有状态类型添加 `#[derive(Serialize, Deserialize)]`
- 确保状态字段类型可序列化
- 检查数据库文件权限和锁定状态
- 使用兼容的序列化格式

### 6. MCP 连接问题

**常见错误信息**：
- `MCP/transport error: connection refused`
- `JSON-RPC error: method not found`
- `MCP/transport error: timeout`

**排查步骤**：

1. **检查 MCP 服务器配置**
```bash
# 验证 MCP 服务器是否运行
curl http://localhost:3000/health

# 检查 MCP 配置文件
cat ~/.loom/mcp_config.toml
```

2. **检查网络连接**
```bash
# 测试 MCP 服务器连接
telnet localhost 3000
# 或
nc -zv localhost 3000
```

3. **检查 MCP 会话配置**
```rust
// 确保 MCP 配置正确
let mcp_config = McpConfig {
    servers: vec![
        McpServerConfig {
            name: "filesystem".to_string(),
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                "/path/to/directory".to_string(),
            ],
            env: None,
        },
    ],
};
```

**解决方案**：
- 确保 MCP 服务器正在运行且可访问
- 验证 MCP 服务器地址和端口配置
- 检查防火墙和网络连接
- 验证 MCP 协议版本兼容性

### 7. 内存/检查点错误

**常见错误信息**：
- `ThreadIdRequired`
- `NotFound: checkpoint not found`
- `Storage: database file corrupted`

**排查步骤**：

1. **检查线程 ID 配置**
```rust
// 确保在 RunnableConfig 中设置了 thread_id
let config = RunnableConfig {
    thread_id: Some("session-123".to_string()),  // 必需
    checkpoint_ns: "my_agent".to_string(),
    ..Default::default()
};
```

2. **检查检查点存在性**
```rust
// 验证检查点是否存在
let (checkpoint, metadata) = checkpointer.get_tuple(&config).await?;
if checkpoint.is_none() {
    println!("未找到检查点，需要开始新会话");
}
```

3. **检查数据库状态**
```bash
# 验证 SQLite 数据库文件
ls -la checkpoints.db
sqlite3 checkpoints.db "SELECT * FROM checkpoints LIMIT 5;"
```

**解决方案**：
- 为需要检查点的执行设置 `thread_id`
- 使用正确的 `checkpoint_ns` 分隔不同类型的会话
- 定期备份检查点数据库
- 处理检查点不存在的 gracefully

### 8. 流式输出问题

**常见错误信息**：
- `StreamRunError: channel closed`
- `stream ended unexpectedly`
- `chunk parsing failed`

**排查步骤**：

1. **检查流式回调**
```rust
// 确保流式回调正确处理错误
runner.stream_with_callback(
    "测试消息",
    Some(|event| {
        match event {
            StreamEvent::Error(err) => {
                eprintln!("流式错误: {:?}", err);
            },
            StreamEvent::End => {
                println!("流结束");
            },
            _ => {}
        }
        async move { Ok(()) }
    })
).await?;
```

2. **检查通道状态**
```rust
// 确保通道没有意外关闭
let (chunk_tx, mut chunk_rx) = mpsc::channel(100);

// 在另一个任务中处理接收
tokio::spawn(async move {
    while let Some(chunk) = chunk_rx.recv().await {
        // 处理 chunk
    }
});
```

**解决方案**：
- 在流式回调中处理所有事件类型，包括错误
- 确保通道容量足够处理高频率数据
- 实现适当的超时和重连机制
- 验证网络连接稳定性

### 9. Docker/Bot 部署问题

**常见错误信息**：
- `container exited with code 1`
- `network connection failed`
- `permission denied: ./checkpoints.db`

**排查步骤**：

1. **检查 Docker 容器状态**
```bash
# 查看容器日志
docker logs loom-bot

# 检查容器状态
docker ps -a | grep loom
```

2. **检查网络配置**
```bash
# 验证网络连接
docker exec loom-bot ping api.openai.com

# 检查环境变量
docker exec loom-bot env | grep API
```

3. **检查文件权限**
```bash
# 验证挂载目录权限
ls -la ./data/checkpoints.db

# 修复权限
chmod 666 ./data/checkpoints.db
```

**解决方案**：
- 确保 Docker 容器有正确的网络访问权限
- 检查环境变量是否正确传递到容器
- 验证挂载卷的权限和路径
- 使用健康检查和自动重启策略

## 调试日志指南

### 启用详细日志

```bash
# 设置环境变量启用详细日志
export RUST_LOG=debug
export RUST_LOG=loom=trace

# 运行应用程序
loom run "测试消息"
```

### 日志级别说明

- `error`: 仅显示错误信息
- `warn`: 警告和错误信息  
- `info`: 一般信息（默认级别）
- `debug`: 详细调试信息
- `trace`: 最详细的跟踪信息

### 关键日志位置

```rust
// 在代码中添加调试日志
use log::{info, debug, error, warn};

info!("开始处理用户消息: {}", user_message);
debug!("当前状态: {:?}", state);
warn("工具调用超时，重试中...");
error!("LLM API 调用失败: {:?}", err);
```

### 常用调试命令

```bash
# 查看实时日志
loom run "消息" 2>&1 | tee debug.log

# 过滤特定错误
loom run "消息" 2>&1 | grep -i error

# 监控日志文件
tail -f debug.log
```

## 获取帮助

### 社区资源

- **GitHub Issues**: [https://github.com/your-org/loom/issues](https://github.com/your-org/loom/issues)
- **文档网站**: [https://docs.loom.dev](https://docs.loom.dev)
- **Discord 社区**: [https://discord.gg/loom](https://discord.gg/loom)

### 报告问题

当报告问题时，请提供以下信息：

1. **环境信息**
```bash
# 系统信息
uname -a
rustc --version
loom --version

# 环境变量
env | grep -E "(LOOM|OPENAI|API)"
```

2. **错误信息**
```
# 完整的错误堆栈跟踪
Error: ExecutionFailed: API key not found
  at loom/src/agent/react/think_node.rs:123
  at loom/src/graph/compiled.rs:456
```

3. **复现步骤**
```bash
# 最小化复现示例
loom run "简单测试消息"
```

4. **配置文件**
```toml
# 相关配置内容
cat ~/.loom/config.toml
cat .env
```

### 诊断信息收集

```bash
# 收集诊断信息
loom doctor > diagnostics.txt

# 包含系统信息、配置状态、依赖版本等
```

### 常见问题查询

在报告问题前，先搜索现有问题：

```bash
# 搜索关键词
# "API key not found"
# "model not found" 
# "MCP connection failed"
# "checkpoint serialization error"
```

---

**相关资源**: [快速入门](../getting-started/quickstart.md) | [LLM 客户端](../core/llm-client.md) | [CLI 部署](./cli.md)