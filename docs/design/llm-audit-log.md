# LLM 审计日志方案

## 目标

将 LLM API 调用（请求 + 响应）持久化到文件，用于调试、成本分析、对话回放。

## 存储设计

| 项目 | 方案 |
|------|------|
| 路径 | `~/.loom/data/llm_logs/` |
| 文件命名 | `{thread_id}.jsonl` |
| 格式 | JSONL，每行一条完整记录 |
| 追加写入 | 同 session 追加到同一文件 |
| 大小限制 | 无 |

无 `thread_id` 时跳过记录。

## 日志结构

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "timestamp": "2025-08-19T10:00:00Z",
  "thread_id": "session-uuid",
  "type": "chat|chat_stream",
  "model": "deepseek-chat",
  "url": "https://api.deepseek.com/v1/chat/completions",
  "duration_ms": 1234,
  "status": 200,
  "request": {
    "messages": [...],
    "tools": [...],
    "parameters": {
      "temperature": 0.7,
      "max_tokens": 4096,
      "stream": true
    }
  },
  "response": {
    "content": "...",
    "usage": {
      "prompt_tokens": 100,
      "completion_tokens": 200,
      "total_tokens": 300
    },
    "tool_calls": [...]
  }
}
```

streaming 场景：等流结束后一次性写入完整记录。

## 脱敏规则

| 字段 | 处理 |
|------|------|
| `api_key` | 移除 |
| `Authorization` header | 移除 |
| 其他数据 | 完整保留 |

## 配置方式

```yaml
# config.yaml
llm:
  audit:
    enabled: false
    path: "~/.loom/data/llm_logs"
```

## 保留策略

永不过期，用户自行清理。

## 记录范围

- 只记录 `ChatOpenAICompat` 客户端
- 其他客户端（ChatOpenAI、FixedProvider）暂不记录

---

## 详细开发方案

### Step 1: 定义审计日志数据结构

**文件**: `loom/src/llm/audit.rs`（新建）

```rust
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一次 LLM 调用的完整审计记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditEntry {
    /// 唯一 ID（UUID v6）
    pub id: String,
    /// ISO 8601 时间戳
    pub timestamp: String,
    /// 会话标识（来自 LlmHeaders.thread_id）
    pub thread_id: String,
    /// 调用类型：非流式 "chat"，流式 "chat_stream"
    #[serde(rename = "type")]
    pub entry_type: String,
    /// 模型名称
    pub model: String,
    /// 请求 URL
    pub url: String,
    /// 请求耗时（毫秒）
    pub duration_ms: u64,
    /// HTTP 状态码（成功时为 200，错误时为实际值）
    pub status: u16,
    /// 请求详情
    pub request: LlmAuditRequest,
    /// 响应详情（错误时为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<LlmAuditResponse>,
    /// 错误信息（成功时为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 请求部分。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRequest {
    /// 发送给 API 的消息列表（已序列化为 JSON Value）
    pub messages: serde_json::Value,
    /// 工具定义（如有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    /// 请求参数
    pub parameters: LlmAuditRequestParams,
}

/// 请求参数摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRequestParams {
    pub temperature: Option<f32>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

/// 响应部分。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditResponse {
    /// 助手回复内容
    pub content: String,
    /// 推理/思考内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Token 使用量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmAuditUsage>,
    /// 工具调用列表
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmAuditToolCall>,
}

/// Token 使用量（与 LlmUsage 对应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 工具调用记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: String,
}
```

### Step 2: 定义 Trait 接口

**文件**: `loom/src/llm/audit.rs`（续）

```rust
/// LLM 审计日志接口。
pub trait LlmAuditLog: Send + Sync {
    /// 记录一条审计日志（异步，不阻塞调用方）。
    fn log(&self, entry: LlmAuditEntry);
}

/// 空实现：不记录。
pub struct NoOpLlmAuditLog;

impl LlmAuditLog for NoOpLlmAuditLog {
    fn log(&self, _entry: LlmAuditEntry) {}
}
```

### Step 3: 实现 FileLlmAuditLog

**文件**: `loom/src/llm/audit.rs`（续）

```rust
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::warn;

/// 后台写入消息。
enum AuditMsg {
    Write(LlmAuditEntry),
    Shutdown,
}

/// 基于文件的审计日志实现。
///
/// 使用 mpsc::unbounded_channel 异步写入，
/// 后台 task 消费队列并将每条记录序列化为 JSONL 追加到文件。
pub struct FileLlmAuditLog {
    tx: msc::UnboundedSender<AuditMsg>,
}

impl FileLlmAuditLog {
    /// 创建新的文件审计日志。
    ///
    /// - `base_path`: 日志目录，如 `~/.loom/data/llm_logs/`
    ///
    /// 启动一个后台 tokio task 负责写入文件。
    /// 当 `tx` 被 drop 时（FileLlmAuditLog 被 drop），后台 task 自动退出。
    pub fn new(base_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(Self::writer_task(rx, base_path));
        Self { tx }
    }

    async fn writer_task(
        mut rx: mpsc::UnboundedReceiver<AuditMsg>,
        base_path: PathBuf,
    ) {
        while let Some(msg) = rx.recv().await {
            match msg {
                AuditMsg::Write(entry) => {
                    let thread_id = entry.thread_id.clone();
                    let file_path = base_path.join(format!("{}.jsonl", thread_id));
                    if let Err(e) = Self::append_entry(&file_path, &entry) {
                        warn!(
                            path = %file_path.display(),
                            error = %e,
                            "Failed to write LLM audit log"
                        );
                    }
                }
                AuditMsg::Shutdown => break,
            }
        }
    }

    /// 将一条记录追加到 JSONL 文件。
    fn append_entry(path: &PathBuf, entry: &LlmAuditEntry) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let mut line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        line.push('\n');
        file.write_all(line.as_bytes())
    }
}

impl LlmAuditLog for FileLlmAuditLog {
    fn log(&self, entry: LlmAuditEntry) {
        if self.tx.send(AuditMsg::Write(entry)).is_err() {
            // 后台 task 已退出，静默丢弃
        }
    }
}
```

### Step 4: 导出和注册模块

**文件**: `loom/src/llm/mod.rs`

改动：
1. 添加 `pub mod audit;`
2. 导出核心类型

```rust
// 在现有 mod 声明区域添加：
pub mod audit;

// 在现有 pub use 区域添加：
pub use audit::{
    LlmAuditEntry, LlmAuditLog, LlmAuditRequest, LlmAuditRequestParams,
    LlmAuditResponse, LlmAuditToolCall, LlmAuditUsage,
    FileLlmAuditLog, NoOpLlmAuditLog,
};
```

### Step 5: 注入 audit_log 到 ChatOpenAICompat

**文件**: `loom/src/llm/openai_compat.rs`

#### 5.1 添加字段

在 `ChatOpenAICompat` 结构体中添加：

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
    headers: Option<crate::llm::LlmHeaders>,
    // 新增
    audit_log: Option<Arc<dyn LlmAuditLog>>,
}
```

#### 5.2 添加 builder 方法

```rust
/// 设置审计日志记录器。
pub fn with_audit_log(mut self, audit: Arc<dyn LlmAuditLog>) -> Self {
    self.audit_log = Some(audit);
    self
}
```

#### 5.3 所有构造函数中初始化

在 `with_config()`、`new()`、`with_test_client()` 的返回值中添加 `audit_log: None`。

#### 5.4 实现审计记录辅助方法

```rust
/// 提取 thread_id（从 headers 中获取）。
fn thread_id(&self) -> Option<String> {
    self.headers.as_ref().and_then(|h| h.thread_id.clone())
}

/// 构建审计日志的请求部分。
fn build_audit_request(&self, body: &ChatCompletionRequest) -> LlmAuditRequest {
    LlmAuditRequest {
        messages: serde_json::to_value(&body.messages).unwrap_or(serde_json::Value::Null),
        tools: body.tools.as_ref().map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null)),
        parameters: LlmAuditRequestParams {
            temperature: body.temperature,
            stream: body.stream,
            tool_choice: body.tool_choice.clone(),
        },
    }
}

/// 构建审计日志的响应部分。
fn build_audit_response(response: &LlmResponse) -> LlmAuditResponse {
    LlmAuditResponse {
        content: response.content.clone(),
        reasoning_content: response.reasoning_content.clone(),
        usage: response.usage.as_ref().map(|u| LlmAuditUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }),
        tool_calls: response.tool_calls.iter().map(|tc| LlmAuditToolCall {
            id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        }).collect(),
    }
}

/// 记录审计日志（内部方法，自动跳过无 thread_id 的情况）。
fn record_audit(
    &self,
    entry_type: &str,
    url: &str,
    duration_ms: u64,
    status: u16,
    request: LlmAuditRequest,
    response: Option<LlmAuditResponse>,
    error: Option<String>,
) {
    let Some(ref audit) = self.audit_log else { return };
    let Some(thread_id) = self.thread_id() else { return };
    audit.log(LlmAuditEntry {
        id: uuid6().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        thread_id,
        entry_type: entry_type.to_string(),
        model: self.model.clone(),
        url: url.to_string(),
        duration_ms,
        status,
        request,
        response,
        error,
    });
}
```

### Step 6: 在 invoke() 中注入审计

**文件**: `loom/src/llm/openai_compat.rs`

改动位置：`invoke()` 方法（约 L593-805）

```rust
async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
    let trace_id = uuid6().to_string();
    let request_id = uuid6().to_string();
    let url = self.chat_completions_url();
    let body = self.build_request(messages, false);
    let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);

    // --- 审计：记录开始时间 ---
    let audit_start = std::time::Instant::now();
    let audit_request = self.build_audit_request(&body);

    // --- 原有 debug 改为 info ---
    tracing::info!(
        trace_id = %trace_id,
        request_id = %request_id,
        url = %url,
        model = %self.model,
        message_count = messages.len(),
        tools_count = tools_count,
        "OpenAI-compat chat create"
    );

    // ... 原有请求逻辑不变 ...

    // --- 在成功返回前，记录审计 ---
    // 将以下代码放在 Ok(LlmResponse { ... }) 之前
    let audit_duration = audit_start.elapsed().as_millis() as u64;
    let audit_response = Self::build_audit_response(&result);
    self.record_audit(
        "chat",
        &url,
        audit_duration,
        200,
        audit_request,
        Some(audit_response),
        None,
    );

    Ok(result)
}
```

对于错误路径（所有 `return Err(...)` 之前），添加：

```rust
let audit_duration = audit_start.elapsed().as_millis() as u64;
self.record_audit(
    "chat",
    &url,
    audit_duration,
    status.as_u16(), // 或 0（如果是连接错误）
    audit_request.clone(),
    None,
    Some(error_message.to_string()),
);
```

**注意**: `build_audit_request` 返回的 `LlmAuditRequest` 需要 `Clone`（添加 `#[derive(Clone)]`），因为 invoke 中有重试循环，每次重试的错误路径都需要用到它。

### Step 7: 在 invoke_stream_with_tool_delta() 中注入审计

**文件**: `loom/src/llm/openai_compat.rs`

改动位置：`invoke_stream_with_tool_delta()` 方法（约 L816-1199）

```rust
async fn invoke_stream_with_tool_delta(
    &self,
    messages: &[Message],
    chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
) -> Result<LlmResponse, AgentError> {
    if chunk_tx.is_none() {
        return self.invoke(messages).await;
    }

    // --- 审计：记录开始时间 ---
    let audit_start = std::time::Instant::now();
    let audit_request: Option<LlmAuditRequest>; // 延迟初始化

    // ... 原有逻辑 ...

    let body = self.build_request(messages, true);

    // --- 审计：构建请求记录 ---
    audit_request = Some(self.build_audit_request(&body));

    // --- 原有 debug 改为 info ---
    tracing::info!(
        trace_id = %trace_id,
        request_id = %request_id,
        url = %url,
        model = %self.model,
        message_count = messages.len(),
        stream = true,
        tools_count = tools_count,
        "OpenAI-compat chat create_stream"
    );

    // ... 原有流式处理逻辑不变 ...

    // --- 在最终 Ok(LlmResponse { ... }) 之前，记录审计 ---
    let audit_duration = audit_start.elapsed().as_millis() as u64;
    let audit_response = Self::build_audit_response(&final_response);
    self.record_audit(
        "chat_stream",
        &url,
        audit_duration,
        200,
        audit_request.unwrap(),
        Some(audit_response),
        None,
    );

    Ok(final_response)
}
```

错误路径同 Step 6，在 `return Err(...)` 前记录。

### Step 8: 配置解析

**文件**: `config/src/` 目录（具体文件取决于现有配置结构）

添加配置结构体：

```rust
/// LLM 审计日志配置。
#[derive(Debug, Clone, Deserialize)]
pub struct LlmAuditConfig {
    /// 是否启用审计日志。
    #[serde(default)]
    pub enabled: bool,
    /// 日志文件目录。
    #[serde(default = "default_audit_path")]
    pub path: PathBuf,
}

fn default_audit_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".loom")
        .join("data")
        .join("llm_logs")
}
```

### Step 9: 审计日志初始化

在应用启动时（如 `cli/src/run/agent.rs` 或 `serve/src/lib.rs`），根据配置创建 audit log 实例：

```rust
fn create_audit_log(config: &LlmAuditConfig) -> Option<Arc<dyn LlmAuditLog>> {
    if !config.enabled {
        return None;
    }
    match FileLlmAuditLog::new(config.path.clone()) {
        Ok(log) => Some(Arc::new(log)),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to create LLM audit log, disabling");
            None
        }
    }
}
```

然后通过 `with_audit_log()` 传入 `ChatOpenAICompat`。

### Step 10: debug 日志升级

当 `audit_log` 存在时（`self.audit_log.is_some()`），将以下位置的 `debug!` 改为 `info!`：

| 位置 | 原有日志 | 改为 |
|------|---------|------|
| `invoke()` L599 | `debug!("OpenAI-compat chat create")` | `info!(...)` |
| `invoke_stream_with_tool_delta()` L832 | `debug!("OpenAI-compat chat create_stream")` | `info!(...)` |

实现方式：根据 `self.audit_log.is_some()` 动态选择级别，或直接改为 `info!`（审计开启时自然需要更高级别日志）。

---

## 改动文件清单

| 文件 | 类型 | 改动说明 |
|------|------|---------|
| `loom/src/llm/audit.rs` | 新建 | 数据结构 + Trait + FileLlmAuditLog + NoOpLlmAuditLog |
| `loom/src/llm/mod.rs` | 修改 | 添加 `pub mod audit`，导出类型 |
| `loom/src/llm/openai_compat.rs` | 修改 | 添加 audit_log 字段、builder、invoke/stream 注入 |
| `config/src/` | 修改 | 添加 `LlmAuditConfig` 配置结构 |
| `cli/src/run/agent.rs` | 修改 | 初始化 audit_log 并传入 client |
| `serve/src/lib.rs` | 修改 | 初始化 audit_log 并传入 client |

## 测试方案

### 单元测试

**文件**: `loom/src/llm/audit.rs`（内联 `#[cfg(test)]`）

1. `test_noop_audit_log_does_nothing` — 验证 NoOp 不报错
2. `test_file_audit_log_writes_jsonl` — 写入一条记录，验证文件内容和 JSON 格式
3. `test_file_audit_log_appends` — 写入多条记录，验证追加行为
4. `test_file_audit_log_creates_directory` — 目标目录不存在时自动创建
5. `test_audit_entry_serialization` — 验证序列化/反序列化 round-trip
6. `test_audit_entry_skip_none_fields` — 验证 `skip_serializing_if` 正确工作

### 集成测试

1. 使用 `with_audit_log` + mock HTTP server，验证 invoke() 完成后 JSONL 文件生成
2. 验证 streaming 场景下流结束后记录写入
3. 验证错误场景（5xx）下记录 status + error 字段
4. 验证无 thread_id 时不写入

## 实现顺序

1. ✅ 定义 `LlmAuditEntry` 等数据结构（Step 1）
2. ✅ 实现 Trait + NoOp（Step 2）
3. ✅ 实现 FileLlmAuditLog（Step 3）
4. ✅ 模块导出（Step 4）
5. ✅ 注入 ChatOpenAICompat 字段和 builder（Step 5.1-5.3）
6. ✅ 实现 invoke() 审计注入（Step 6）
7. ✅ 实现 invoke_stream_with_tool_delta() 审计注入（Step 7）
8. ✅ 配置解析（Step 8）— env vars: `LLM_AUDIT_ENABLED`, `LLM_AUDIT_PATH`
9. ⬜ 应用层初始化（Step 9）— 需要在 main/agent init 中调用 `LlmAuditConfig::from_env().build()`
10. ⬜ debug 日志升级（Step 10）
11. ✅ 单元测试（7 tests）
12. ⬜ 集成测试