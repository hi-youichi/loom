# LLM HTTP Headers 增加方案

## 背景

为了增强 LLM 请求的可观测性和追踪能力，需要为所有 LLM HTTP 请求添加自定义 HTTP headers，包括：
- `X-App-Id`: 应用标识符（**固定为 "loom"**）
- `X-Thread-Id`: 线程标识符  
- `X-Trace-Id`: 追踪标识符
- 其他自定义 headers

## 技术方案

### 1. 整体架构

当前 Loom 项目中有两个主要的 LLM 客户端实现：
- `ChatOpenAICompat`: 基于 `reqwest` 的 OpenAI 兼容客户端
- `ChatOpenAI`: 基于 `async_openai` 的 OpenAI 官方客户端

两个客户端需要统一的 HTTP headers 配置接口。

### 2. 核心设计

#### 2.1 Header 配置结构

在 `loom/src/llm/mod.rs` 中定义统一的 header 配置：

```rust
#[derive(Debug, Clone, Default)]
pub struct LlmHeaders {
    // X-App-Id 固定为 "loom"，不在配置结构中
    pub thread_id: Option<String>,
    pub trace_id: Option<String>,
    pub custom_headers: std::collections::HashMap<String, String>,
}
```

**重要变更**：`X-App-Id` header 固定设置为 `"loom"`，用户无需配置。

#### 2.2 客户端适配

**ChatOpenAICompat 适配**：
- 在构造函数中添加 `headers: Option<LlmHeaders>` 字段
- 添加 `with_headers()` 构建方法
- 实现 `add_headers_to_request()` 辅助方法，**自动设置 X-App-Id 为 "loom"**
- 在所有 HTTP 请求构建时注入 headers

**ChatOpenAI 适配**：
- 在结构体中添加 `headers: Option<LlmHeaders>` 字段
- 添加 `with_headers()` 构建方法
- 实现 `get_headers_map()` 方法获取 headers，**自动设置 X-App-Id 为 "loom"**
- 通过 `async_openai` 的配置系统注入 headers

### 3. 实现细节

#### 3.1 LlmHeaders 结构

位置：`loom/src/llm/mod.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct LlmHeaders {
    pub thread_id: Option<String>, 
    pub trace_id: Option<String>,
    pub custom_headers: std::collections::HashMap<String, String>,
}

impl LlmHeaders {
    pub fn with_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }
    
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
    
    pub fn add_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_headers.insert(key.into(), value.into());
        self
    }
}
```

**重要变更**：
- 移除了 `app_id` 字段和 `with_app_id()` 方法
- `X-App-Id` header 将在 HTTP 请求中自动设置为 `"loom"`

#### 3.2 ChatOpenAICompat 实现

位置：`loom/src/llm/openai_compat.rs`

**结构体修改**：

```rust
pub struct ChatOpenAICompat {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    tools: Option<Vec<ToolSpec>>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoiceMode>,
    parse_thinking_tags: bool,
    headers: Option<LlmHeaders>, // 新增字段
}
```

**新增方法**：

```rust
impl ChatOpenAICompat {
    pub fn with_headers(mut self, headers: LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }
    
    fn add_headers_to_request(&self, request_builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = request_builder;
        
        if let Some(headers) = &self.headers {
            // 固定设置 X-App-Id 为 "loom"
            builder = builder.header("X-App-Id", "loom");
            
            if let Some(thread_id) = &headers.thread_id {
                builder = builder.header("X-Thread-Id", thread_id);
            }
            if let Some(trace_id) = &headers.trace_id {
                builder = builder.header("X-Trace-Id", trace_id);
            }
            
            for (key, value) in &headers.custom_headers {
                builder = builder.header(key, value);
            }
        }
        
        builder
    }
}
```

**请求修改示例**：

原代码：
```rust
let res = self.client
    .post(&url)
    .bearer_auth(&self.api_key)
    .json(&body)
    .send()
    .await;
```

修改后：
```rust
let res = self.add_headers_to_request(
    self.client
        .post(&url)
        .bearer_auth(&self.api_key)
        .json(&body)
)
.send()
.await;
```

#### 3.3 ChatOpenAI 实现

位置：`loom/src/llm/openai/mod.rs`

**结构体修改**：

```rust
pub struct ChatOpenAI {
    client: Client<OpenAIConfig>,
    headers: Option<LlmHeaders>, // 新增字段
    // ... 其他字段
}
```

**新增方法**：

```rust
impl ChatOpenAI {
    pub fn with_headers(mut self, headers: LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }
    
    fn get_headers_map(&self) -> std::collections::HashMap<String, String> {
        let mut headers = std::collections::HashMap::new();
        
        if let Some(config) = &self.headers {
            if let Some(app_id) = &config.app_id {
                headers.insert("X-App-Id".to_string(), app_id.clone());
            }
            if let Some(thread_id) = &config.thread_id {
                headers.insert("X-Thread-Id".to_string(), thread_id.clone());
            }
            if let Some(trace_id) = &config.trace_id {
                headers.insert("X-Trace-Id".to_string(), trace_id.clone());
            }
            
            for (key, value) in &config.custom_headers {
                headers.insert(key.clone(), value.clone());
            }
        }
        
        headers
    }
}
```

**请求修改**：

在 `async_openai` 客户端中使用默认 headers 功能。需要在客户端初始化时设置：

```rust
pub fn with_config(config: OpenAIConfig, model: impl Into<String>) -> Self {
    // ... 现有代码 ...
    let mut client = Client::with_config(config);
    
    // 设置默认 headers
    if let Some(headers) = &self.headers {
        let headers_map = self.get_headers_map();
        // async_openai 支持 default_headers
        client = client.with_default_headers(headers_map);
    }
    
    Self { client, model: model.into(), .. }
}
```

### 4. 环境变量支持

在 `loom/src/llm/mod.rs` 中添加：

```rust
pub fn get_headers_from_env() -> LlmHeaders {
    LlmHeaders {
        app_id: std::env::var("LLM_APP_ID").ok(),
        thread_id: std::env::var("LLM_THREAD_ID").ok(), 
        trace_id: std::env::var("LLM_TRACE_ID").ok(),
        custom_headers: std::collections::HashMap::new(),
    }
}
```

## 使用示例

### 示例 1：直接配置 Headers

```rust
use loom::llm::{LlmHeaders, ChatOpenAICompat};

let headers = LlmHeaders::default()
    .with_thread_id("thread-12345")
    .with_trace_id("trace-67890")
    .add_header("X-Custom-Header", "custom-value");

let client = ChatOpenAICompat::new(
    "https://api.openai.com/v1",
    "sk-xxx",
    "gpt-4"
).with_headers(headers);

// 注意：X-App-Id 会自动设置为 "loom"
```

### 示例 2：使用环境变量

```rust
use loom::llm::{ChatOpenAI, get_headers_from_env};

// 设置环境变量
// LLM_THREAD_ID=thread-12345  
// LLM_TRACE_ID=trace-67890
// 注意：不再需要设置 LLM_APP_ID，X-App-Id 会自动设置为 "loom"

let headers = get_headers_from_env();
let client = ChatOpenAI::with_config(config, "gpt-4")
    .with_headers(headers);
```

### 示例 3：链式调用

```rust
let client = ChatOpenAICompat::new(
    "https://api.openai.com/v1",
    "sk-xxx",
    "gpt-4"
)
.with_headers(
    LlmHeaders::default()
        .with_thread_id("thread-123")
        .with_trace_id("trace-456")
)
.with_tools(tools)
.with_temperature(0.7);

// 注意：X-App-Id 会自动设置为 "loom"
```

## 实施步骤

### Phase 1: 基础结构
1. 在 `loom/src/llm/mod.rs` 中实现 `LlmHeaders` 结构
2. 添加 `get_headers_from_env()` 函数
3. 编写单元测试

### Phase 2: ChatOpenAICompat 适配
1. 修改 `ChatOpenAICompat` 结构体，添加 `headers` 字段
2. 实现 `with_headers()` 方法
3. 实现 `add_headers_to_request()` 辅助方法
4. 修改所有 HTTP 请求构建点
5. 编写集成测试

### Phase 3: ChatOpenAI 适配
1. 修改 `ChatOpenAI` 结构体，添加 `headers` 字段
2. 实现 `with_headers()` 方法
3. 实现 `get_headers_map()` 方法
4. 集成到 `async_openai` 客户端配置
5. 编写集成测试

### Phase 4: 文档和示例
1. 更新 API 文档
2. 添加使用示例
3. 更新相关集成指南

## 测试策略

### 单元测试
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_headers_builder() {
        let headers = LlmHeaders::default()
            .with_app_id("test-app")
            .with_thread_id("test-thread")
            .with_trace_id("test-trace");
        
        assert_eq!(headers.app_id, Some("test-app".to_string()));
        assert_eq!(headers.thread_id, Some("test-thread".to_string()));
        assert_eq!(headers.trace_id, Some("test-trace".to_string()));
    }

    #[test]
    fn test_custom_headers() {
        let headers = LlmHeaders::default()
            .add_header("X-Custom", "value")
            .add_header("X-Another", "another-value");
        
        assert_eq!(headers.custom_headers.get("X-Custom"), Some(&"value".to_string()));
        assert_eq!(headers.custom_headers.get("X-Another"), Some(&"another-value".to_string()));
    }
}
```

### 集成测试
- 验证 HTTP 请求中包含正确的 headers
- 测试重试场景下的 headers 保持
- 验证流式请求中的 headers

## 兼容性

### 向后兼容
- `headers` 字段为 `Option<LlmHeaders>`，默认为 `None`
- 现有代码无需修改即可继续工作
- 新功能通过 `with_headers()` 方法启用

### 破坏性变更
无破坏性变更，所有现有代码保持兼容。

## 性能影响

- 内存影响：每个客户端实例增加少量内存用于存储 headers
- 性能影响：每个请求增加 3-5 个 HTTP headers，影响可忽略不计
- 网络影响：每个请求增加约 100-200 字节数据

## 安全考虑

1. **敏感信息保护**：
   - 不在 headers 中发送 API 密钥等敏感信息
   - 建议通过环境变量配置 headers

2. **Header 验证**：
   - 验证 header 格式和大小
   - 限制自定义 headers 数量

3. **日志记录**：
   - 注意不要在日志中记录敏感的 header 值

## 维护性

1. **统一接口**：两种客户端使用相同的 header 配置结构
2. **易于扩展**：如需添加新的标准 headers，只需修改 `LlmHeaders` 结构
3. **清晰的职责**：header 逻辑集中在专门的类型中

## 后续优化

1. **配置文件支持**：从配置文件中读取 headers
2. **动态生成**：支持动态生成 trace_id 等
3. **请求上下文集成**：与 Loom 的请求上下文系统深度集成
4. **监控集成**：利用 headers 进行请求追踪和监控分析

## 相关文件

- `loom/src/llm/mod.rs`: 主要接口定义
- `loom/src/llm/openai_compat.rs`: OpenAI 兼容客户端实现
- `loom/src/llm/openai/mod.rs`: OpenAI 官方客户端实现
- `loom/src/agent/react/build/llm.rs`: LLM 客户端创建逻辑

## 参考资料

- HTTP/1.1 Header Field Specifications (RFC 7231)
- OpenAI API Documentation
- `reqwest` 客户端文档
- `async_openai` 客户端文档