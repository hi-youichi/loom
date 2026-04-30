# 检查点与存储

智能体状态检查点和持久化存储系统，支持执行中断恢复和跨会话的长期记忆管理。

## 存储实现对比

| 特性 | MemorySaver | SqliteSaver | LanceDB |
|------|-----------|-------------|---------|
| 用途 | 开发/测试 | 生产环境 | 语义搜索 |
| 持久化 | ❌ 内存 | ✅ SQLite | ✅ 向量数据库 |
| 性能 | ⚡ 最快 | 🚀 快速 | 🐢 较慢 |
| 搜索支持 | ❌ | ❌ 基础 | ✅ 语义搜索 |
| 适用场景 | 单次运行 | 生产部署 | RAG 应用 |
| 配置复杂度 | 🟢 简单 | 🟡 中等 | 🔴 较高 |

## 核心概念

### Checkpointer 生命周期

```rust
// 1. 执行前 - 创建检查点器
let checkpointer = Arc::new(SqliteSaver::new("./checkpoints.db", serializer)?);

// 2. 执行中 - 图编译集成
let compiled = graph.compile_with_checkpointer(checkpointer)?;

// 3. 执行时 - 自动保存检查点
let result = compiled.invoke(input, Some(config)).await?;

// 4. 执行后 - 恢复或继续
let (checkpoint, _) = checkpointer.get_tuple(&config).await?;
```

### Store 模式

```rust
// 键值存储模式
store.put(&namespace, "user_123", &json!({"name": "Alice"})).await?;
let user = store.get(&namespace, "user_123").await?;

// 语义搜索模式 (LanceDB)
let results = store.search(&ns_prefix, SearchOptions::new()
    .with_query("查找用户偏好")).await?;
```

## 核心接口

### Checkpointer Trait

```rust
#[async_trait]
pub trait Checkpointer<S>: Send + Sync
where
    S: Clone + Send + Sync + 'static,
{
    async fn put(
        &self,
        config: &RunnableConfig,
        checkpoint: &Checkpoint<S>,
    ) -> Result<String, CheckpointError>;

    async fn get_tuple(
        &self,
        config: &RunnableConfig,
    ) -> Result<Option<(Checkpoint<S>, CheckpointMetadata)>, CheckpointError>;

    async fn list(
        &self,
        config: &RunnableConfig,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Vec<CheckpointListItem>, CheckpointError>;
}
```

### Store Trait

```rust
#[async_trait]
pub trait Store: Send + Sync {
    async fn put(&self, namespace: &Namespace, key: &str, value: &serde_json::Value) 
        -> Result<(), StoreError>;
    async fn get(&self, namespace: &Namespace, key: &str) 
        -> Result<Option<serde_json::Value>, StoreError>;
    async fn search(&self, namespace_prefix: &Namespace, options: SearchOptions) 
        -> Result<Vec<SearchItem>, StoreError>;
    // ... 更多方法
}
```

## 代码示例

### 基础 MemorySaver 使用

```rust
use loom::memory::{Checkpointer, MemorySaver};
use loom::graph::{StateGraph, RunnableConfig};
use loom::agent::react::ReActState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建内存检查点器
    let checkpointer = Arc::new(MemorySaver::<ReActState>::new());

    // 构建并编译状态图
    let mut graph = StateGraph::<ReActState>::new();
    // ... 添加节点和边

    let compiled = graph.compile_with_checkpointer(checkpointer)?;

    // 配置执行参数
    let config = RunnableConfig {
        thread_id: Some("session-123".to_string()),
        checkpoint_ns: "my_agent".to_string(),
        ..Default::default()
    };

    // 执行并自动保存检查点
    let result = compiled.invoke(initial_state, Some(config)).await?;

    println!("执行完成，检查点已保存");

    Ok(())
}
```

### SqliteSaver 持久化存储

```rust
use loom::memory::{Checkpointer, SqliteSaver, JsonSerializer};
use loom::graph::{StateGraph, RunnableConfig};
use loom::agent::react::ReActState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 SQLite 检查点器
    let serializer = Arc::new(JsonSerializer);
    let checkpointer = Arc::new(SqliteSaver::<ReActState>::new(
        "./checkpoints.db",
        serializer,
    )?);

    // 构建状态图
    let mut graph = StateGraph::<ReActState>::new();
    // ... 添加节点和边

    let compiled = graph.compile_with_checkpointer(checkpointer.clone())?;

    // 配置持久化执行
    let config = RunnableConfig {
        thread_id: Some("production-session-456".to_string()),
        checkpoint_ns: "production_agent".to_string(),
        checkpoint_id: None,  // 自动生成
        ..Default::default()
    };

    // 执行并持久化检查点
    let result = compiled.invoke(initial_state, Some(config)).await?;

    // 验证检查点已保存
    let (checkpoint, metadata) = checkpointer.get_tuple(&config).await?
        .expect("检查点应该存在");

    println!("检查点 ID: {}", metadata.checkpoint_id);
    println!("执行步骤: {}", metadata.step);

    Ok(())
}
```

### Store 跨会话状态管理

```rust
use loom::memory::{Store, InMemoryStore, Namespace};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建内存存储
    let store = Arc::new(InMemoryStore::new());

    // 定义命名空间
    let user_namespace = Namespace::new("users")?;
    let session_namespace = Namespace::new("sessions")?;

    // 存储用户偏好设置
    store.put(&user_namespace, "user_001", &json!({
        "name": "张三",
        "preferences": {
            "language": "zh-CN",
            "theme": "dark",
            "notifications": true
        }
    })).await?;

    // 存储会话上下文
    store.put(&session_namespace, "session_123", &json!({
        "user_id": "user_001",
        "start_time": "2025-08-19T10:00:00Z",
        "conversation_history": ["hello", "how are you"]
    })).await?;

    // 检索数据
    let user_data = store.get(&user_namespace, "user_001").await?
        .expect("用户数据应该存在");

    println!("用户偏好: {}", user_data["preferences"]["language"]);

    // 列出所有会话
    let sessions = store.list(&session_namespace).await?;
    println!("活跃会话数: {}", sessions.len());

    Ok(())
}
```

### 从检查点恢复执行

```rust
use loom::memory::{Checkpointer, SqliteSaver, JsonSerializer};
use loom::graph::{StateGraph, RunnableConfig};
use loom::agent::react::ReActState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建检查点器
    let serializer = Arc::new(JsonSerializer);
    let checkpointer = Arc::new(SqliteSaver::<ReActState>::new(
        "./checkpoints.db",
        serializer,
    )?);

    let compiled = StateGraph::<ReActState>::new()
        .compile_with_checkpointer(checkpointer.clone())?;

    // 配置恢复参数
    let config = RunnableConfig {
        thread_id: Some("interrupted-session-789".to_string()),
        checkpoint_ns: "agent".to_string(),
        checkpoint_id: None,  // 获取最新检查点
        ..Default::default()
    };

    // 检查是否有可恢复的检查点
    if let Some((checkpoint, metadata)) = checkpointer.get_tuple(&config).await? {
        println!("发现检查点: {}", metadata.checkpoint_id);
        println!("上次执行步骤: {}", metadata.step);

        // 从检查点状态继续执行
        let result = compiled.invoke(checkpoint.state, Some(config)).await?;
        println!("恢复执行完成");

    } else {
        println("未找到检查点，开始新会话");
        // 开始新的执行
        let result = compiled.invoke(initial_state, Some(config)).await?;
    }

    Ok(())
}
```

### 检查点历史管理

```rust
use loom::memory::{Checkpointer, SqliteSaver, JsonSerializer, CheckpointListItem};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let serializer = Arc::new(JsonSerializer);
    let checkpointer = Arc::new(SqliteSaver::<ReActState>::new(
        "./checkpoints.db",
        serializer,
    )?);

    let config = RunnableConfig {
        thread_id: Some("session-history").to_string(),
        checkpoint_ns: "agent".to_string(),
        ..Default::default()
    };

    // 列出最近的 10 个检查点
    let checkpoints = checkpointer.list(&config, Some(10), None, None).await?;

    println!("检查点历史:");
    for item in checkpoints {
        println!(
            "ID: {}, 时间: {}, 步骤: {}, 来源: {}",
            item.checkpoint_id,
            item.ts,
            item.step,
            item.source
        );
    }

    // 分页查询（使用 before/after）
    let recent_checkpoints = checkpointer.list(&config, Some(5), None, None).await?;
    if let Some(latest_id) = recent_checkpoints.first().map(|c| &c.checkpoint_id) {
        let older_checkpoints = checkpointer.list(
            &config,
            Some(5),
            Some(latest_id),  // 在最新ID之前
            None
        ).await?;
        println!("更早的检查点: {}", older_checkpoints.len());
    }

    Ok(())
}
```

### LanceDB 语义搜索（可选特性）

```rust
#[cfg(feature = "lance")]
use loom::memory::{Store, LanceStore, Namespace, SearchOptions};
use loom::memory::embedder::OpenAIEmbedder;
use serde_json::json;

#[cfg(feature = "lance")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建嵌入器
    let embedder = Arc::new(OpenAIEmbedder::new("text-embedding-3-small")?);

    // 创建 LanceDB 存储
    let store = LanceStore::new("./lancedb", embedder).await?;

    let doc_namespace = Namespace::new("documents")?;

    // 存储文档（自动生成嵌入向量）
    store.put(&doc_namespace, "doc1", &json!({
        "title": "Rust 编程指南",
        "content": "Rust 是一种系统编程语言，注重安全性和性能"
    })).await?;

    store.put(&doc_namespace, "doc2", &json!({
        "title": "Python 数据分析",
        "content": "Python 在数据科学领域广泛使用"
    })).await?;

    // 语义搜索
    let results = store.search(
        &doc_namespace,
        SearchOptions::new()
            .with_query("系统编程语言的特点")
            .with_limit(3)
    ).await?;

    println!("搜索结果:");
    for result in results {
        println!("文档: {}, 相似度: {:.2}", 
            result.item.key, result.score);
    }

    Ok(())
}
```

### 批量操作与命名空间管理

```rust
use loom::memory::{Store, InMemoryStore, Namespace, StoreOp};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(InMemoryStore::new());
    let config_namespace = Namespace::new("config")?;

    // 批量操作
    let batch_ops = vec![
        StoreOp::Put(config_namespace.clone(), "theme", json!("dark")),
        StoreOp::Put(config_namespace.clone(), "language", json!("zh-CN")),
        StoreOp::Put(config_namespace.clone(), "timezone", json!("Asia/Shanghai")),
        StoreOp::Delete(config_namespace.clone(), "old_setting"),
    ];

    let results = store.batch(batch_ops).await?;
    println!("批量操作完成: {} 个操作", results.len());

    // 列出所有命名空间
    let namespaces = store.list_namespaces(Default::default()).await?;
    println!("可用命名空间: {:?}", namespaces);

    // 命名空间前缀搜索
    let prefix = Namespace::new("app")?;
    let app_namespaces = store.list_namespaces(
        ListNamespacesOptions::new().with_prefix(&prefix)
    ).await?;

    println!("应用相关命名空间: {:?}", app_namespaces);

    Ok(())
}
```

## 配置与集成

### 在 ReactRunner 中使用

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::memory::{SqliteSaver, JsonSerializer};
use loom::graph::RunnableConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        thread_id: Some("react-session".to_string()),
        checkpoint_ns: "react_agent".to_string(),
        db_path: "./checkpoints.db".to_string(),
        ..Default::default()
    };

    let runner = build_react_runner(&config, None, true).await?;

    // 执行会自动使用配置的检查点器
    let result = runner.invoke("复杂任务").await?;

    Ok(())
}
```

## 最佳实践

### 开发环境
- 使用 `MemorySaver` 进行快速迭代和测试
- 避免磁盘 I/O 开销，提高开发效率
- 适合单次运行和调试场景

### 生产环境
- 使用 `SqliteSaver` 确持状态持久化
- 配置合理的数据库备份策略
- 监控检查点大小和存储空间使用
- 定期清理过期的检查点历史

### 检查点管理
- 为不同类型任务使用不同的 `checkpoint_ns`
- 设置合理的检查点保留策略
- 利用 `list()` API 进行检查点审计
- 实现检查点压缩和归档机制

### 存储优化
- 使用命名空间组织不同类型的数据
- 对频繁访问的数据使用 `InMemoryStore`
- 对需要语义搜索的场景使用 `LanceDB`
- 合理设置批量操作大小提高性能

### 错误处理
- 实现检查点加载失败的降级策略
- 处理存储不可用的情况
- 记录检查点保存失败的日志
- 提供手动触发检查点保存的机制

---

## 相关概念

- **状态图编译**: 图结构与检查点集成
- **ReAct 运行模式**: 智能体执行与状态管理
- **配置管理**: RunnableConfig 和参数设置
- **错误处理**: CheckpointError 和 StoreError

---

**下一页**: [状态图编译](../core/state-graph.md) | [ReAct 运行模式](../core/react.md) | [配置管理](../core/configuration.md)