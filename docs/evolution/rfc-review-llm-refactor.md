# RFC: ReviewAgent 复用 loom::llm 客户端（方案 B）

> 将 ReviewAgent 从独立的同步 `RealLlm` 改为复用项目主体的 `loom::llm::LlmClient`

## 一、现状问题

当前 Review 功能有两套独立的 LLM 实现：

| | 主 Agent | Review Agent |
|---|---|---|
| **trait** | `LlmClient::invoke(&[Message]) → LlmResponse` | `ReviewLlm::complete(&str) → String` |
| **实现** | `ChatOpenAI` / `ChatOpenAICompat`（async, streaming） | `RealLlm`（sync, `reqwest::blocking`） |
| **配置** | `config.toml` → `ProviderConfig` → `ModelEntry` → `create_llm_client()` | 环境变量 `OPENAI_API_KEY` / `OPENAI_BASE_URL` / `MODEL` |
| **重试** | `RetryLlmClient`（指数退避） | 手动 3 次循环 |
| **模型路由** | `ModelRegistry` tier 解析 | 无 |

**问题清单**：

1. **配置不共享**：Review 读环境变量，主 Agent 读 config.toml，两套配置容易不一致
2. **provider 不复用**：config.toml 中配置的 provider 类型、api_key、base_url 对 Review 无效
3. **重试策略独立**：Review 手写 3 次重试，无退避；主 Agent 有 `RetryLlmClient`
4. **tokio 兼容 hack**：`RealLlm` 用 `reqwest::blocking`，在 async 上下文需要 `spawn_blocking` 包装
5. **维护成本**：两套 LLM 调用代码，修改一处容易忘记另一处

## 二、方案设计

### 2.1 核心变更

```
Before:
  ReviewAgent ──→ dyn ReviewLlm ──→ RealLlm (reqwest::blocking)
  
After:
  ReviewAgent ──→ Box<dyn LlmClient> ──→ ChatOpenAI / ChatOpenAICompat (async)
                       ↑
                  create_llm_client(ModelEntry)
                       ↑
                  config::load_full_config() → ProviderConfig
```

### 2.2 文件变更清单

| 文件 | 操作 | 变更内容 |
|------|------|----------|
| `cli/src/run/review.rs` | 重写 | 删除 `ReviewLlm` trait；`ReviewAgent` 改 async，持有 `Box<dyn LlmClient>` |
| `cli/src/review_cmd.rs` | 修改 | 用 `config::load_full_config` + `create_llm_client` 构造客户端；删除 `spawn_blocking` |
| `cli/src/review_skill_cmd.rs` | 修改 | 删除 `RealLlm`、`resolve_config`；改用 `LlmClient` |
| `cli/src/review_history.rs` | 不变 | — |

### 2.3 `ReviewAgent` 改造（`cli/src/run/review.rs`）

#### 删除

```rust
// 删除整个 ReviewLlm trait
pub trait ReviewLlm: Send + Sync {
    fn complete(&self, prompt: &str) -> Result<String, String>;
}
```

#### 新结构

```rust
use loom::llm::LlmClient;
use loom::message::{Message, UserContent};

pub struct ReviewAgent {
    llm: Box<dyn LlmClient>,
    memory: MemoryStore,
    skills: SkillRegistry,
    config: ReviewConfig,
}

impl ReviewAgent {
    pub fn new(
        llm: Box<dyn LlmClient>,
        memory: MemoryStore,
        skills: SkillRegistry,
    ) -> Self { ... }

    pub fn with_config(
        llm: Box<dyn LlmClient>,
        memory: MemoryStore,
        skills: SkillRegistry,
        config: ReviewConfig,
    ) -> Self { ... }

    pub async fn review_session(&self, session_content: &str) -> Result<ReviewOutput, String> {
        let truncated = /* 截断逻辑不变 */;
        let prompt = build_review_prompt(truncated);
        
        // 构造 Message（单条 user message）
        let messages = vec![Message::User {
            content: UserContent::Text(prompt),
        }];

        // 调用 LlmClient（已内置 RetryLlmClient 重试）
        let response = self.llm.invoke(&messages).await
            .map_err(|e| format!("Review LLM call failed: {}", e))?;

        // 解析响应
        let output = parse_review_response(&response.content)?;
        self.apply_memory_updates(&output.memory_updates)?;
        self.apply_skill_suggestions(&output.skill_suggestions)?;
        Ok(output)
    }
}
```

**关键变化**：

1. `&'a dyn ReviewLlm` → `Box<dyn LlmClient>`（owned，不再需要生命周期参数）
2. `&'a MemoryStore` → `MemoryStore`（owned）
3. `&'a SkillRegistry` → `SkillRegistry`（owned）
4. `review_session(&self)` → `async fn review_session(&self)`
5. `self.llm.complete(prompt)` → `self.llm.invoke(&messages).await`
6. 重试交给 `RetryLlmClient` 包装层，不再手动循环

#### 测试改造

```rust
// Before: impl ReviewLlm for MockLlm
// After:  impl LlmClient for MockLlm（用 loom::llm::mock::MockLlm 或自定义）

#[cfg(test)]
mod tests {
    use loom::llm::{LlmClient, LlmResponse};
    use loom::message::Message;

    struct ReviewMockLlm { response: String }

    #[async_trait::async_trait]
    impl LlmClient for ReviewMockLlm {
        async fn invoke(&self, _messages: &[Message]) -> Result<LlmResponse, loom::AgentError> {
            Ok(LlmResponse {
                content: self.response.clone(),
                tool_calls: vec![],
                reasoning_content: None,
                usage: None,
            })
        }
    }
}
```

### 2.4 `review_cmd.rs` 改造

#### 客户端构造

```rust
// Before:
//   resolve_config() → (api_key, base_url, model)
//   RealLlm::new(api_key, base_url, model)

// After:
use config::{load_full_config, ProviderDef};
use loom::llm::{create_llm_client, ModelEntry, RetryLlmClient};

fn build_review_client(
    model_override: Option<&str>,
) -> Result<Box<dyn LlmClient>, Box<dyn std::error::Error>> {
    let config = load_full_config("loom")?;
    
    // 获取第一个 provider（或根据 model spec 解析）
    let provider = config.providers.first()
        .ok_or("No provider configured in config.toml")?;
    
    let model = model_override
        .map(|m| m.to_string())
        .or_else(|| std::env::var("LOOM_MODEL").ok())
        .or_else(|| std::env::var("MODEL").ok())
        .unwrap_or_else(|| provider.default_model.clone().unwrap_or_default());
    
    let entry = ModelEntry::from_provider_config(
        &ProviderConfig {
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
        },
        &model,
    );

    let client = create_llm_client(&entry, None)?;
    
    // 包装重试层
    let retry_client = RetryLlmClient::new(std::sync::Arc::from(client))
        .with_max_retries(3)
        .with_base_delay(std::time::Duration::from_secs(2));
    
    Ok(Box::new(retry_client))
}
```

#### 调用方式

```rust
// Before:
//   tokio::task::spawn_blocking(move || do_review_single(...))

// After: 直接 await（ReviewAgent 本身是 async）
async fn do_review_single(
    session_id: &str,
    args: &ReviewArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let llm = build_review_client(args.model.as_deref())?;
    let loom_home = config::home::loom_home();
    let memory = MemoryStore::new(&loom_home);
    let skills = SkillRegistry::new(&loom_home.join("skills"));
    
    let agent = ReviewAgent::with_config(llm, memory, skills, ReviewConfig {
        auto_create_threshold: 1,
        max_session_chars: 24000,
    });
    
    let text = SessionManager::with_default_path()
        .extract_session_text(session_id)?;
    
    let output = agent.review_session(&text).await?;
    // ... 输出逻辑不变
}
```

#### `handle_review_command` 简化

```rust
pub(crate) async fn handle_review_command(
    args: &ReviewArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match &args.command {
        ReviewCommand::Session { session_id } => {
            do_review_single(session_id, args, json).await
        }
        ReviewCommand::Sessions { recent, all_unreviewed, query } => {
            do_review_batch(recent, all_unreviewed, query, args, json).await
        }
        // history/show/pending 不变（纯同步 IO）
        _ => { /* 同之前 */ }
    }
}
```

不再需要 `spawn_blocking`——所有 LLM 调用都是 async。

### 2.5 `review_skill_cmd.rs` 改造

删除：

```rust
// 删除 struct RealLlm { ... }
// 删除 impl ReviewLlm for RealLlm { ... }
// 删除 fn resolve_config() { ... }
```

改为：

```rust
pub(crate) async fn handle_review_skill_command(
    args: &ReviewSkillArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let llm = build_review_client(args.model.as_deref())?;
    // ... 其余逻辑复用 ReviewAgent
}
```

`build_review_client()` 提取为公共函数（`pub(crate)`），供 `review_cmd.rs` 和 `review_skill_cmd.rs` 共用。

## 三、依赖分析

### 当前依赖

```
review_cmd.rs ──→ review_skill_cmd.rs (RealLlm, resolve_config)
review_skill_cmd.rs ──→ reqwest::blocking (HTTP)
                      ──→ env vars (OPENAI_API_KEY, OPENAI_BASE_URL, MODEL)
```

### 改造后依赖

```
review_cmd.rs ──→ build_review_client() (新公共函数)
review_skill_cmd.rs ──→ build_review_client()
build_review_client() ──→ config::load_full_config()
                       ──→ loom::llm::create_llm_client()
                       ──→ loom::llm::RetryLlmClient
```

### 新增 crate 依赖

无。`cli` 已依赖 `loom`（含 `loom::llm`）和 `config`。

## 四、实施顺序

```
Step 1 — review.rs: 删 ReviewLlm trait，ReviewAgent 改 async
    ↓
Step 2 — review.rs: 改造测试（MockLlm → impl LlmClient）
    ↓
Step 3 — 新增 build_review_client() 公共函数
    ↓
Step 4 — review_skill_cmd.rs: 删 RealLlm/resolve_config，改用 build_review_client
    ↓
Step 5 — review_cmd.rs: 删 spawn_blocking，改用 async ReviewAgent
    ↓
Step 6 — 编译 + 测试
    ↓
Step 7 — 集成验证（loom review session <id>）
```

## 五、风险评估

| 风险 | 缓解 |
|------|------|
| `MemoryStore`/`SkillRegistry` 是同步 IO，在 async 中调用 | 文件操作很快（几百字节），可接受；如需可后续用 `spawn_blocking` 包装 |
| `ReviewAgent` 持有 owned `MemoryStore`/`SkillRegistry`，不能再借用 | 每次创建 ReviewAgent 时新建实例，开销极小 |
| `load_full_config` 可能读不到配置 | 保留环境变量 fallback（`OPENAI_API_KEY` 等作为兜底） |
| `LlmClient::invoke` 需要 `Message` 类型构造 | 封装 helper `fn single_turn_prompt(text: &str) -> Vec<Message>` |
| 现有测试用 `impl ReviewLlm for MockLlm` | 改为 `impl LlmClient for MockLlm`，改动量小 |

## 六、工作量预估

| 任务 | 预估 |
|------|------|
| review.rs 改 async + 删 ReviewLlm | 2h |
| 测试改造 | 1h |
| build_review_client 公共函数 | 1h |
| review_skill_cmd.rs 改造 | 1h |
| review_cmd.rs 改造 | 1h |
| 编译 + 集成测试 | 1h |
| **合计** | **~7h (1 天)** |

## 七、收益

1. **配置统一**：Review 和主 Agent 共用 config.toml provider 配置
2. **Provider 路由**：支持所有已配置的 provider（openai、bigmodel、modelgate 等）
3. **重试策略**：复用 `RetryLlmClient` 的指数退避
4. **代码精简**：删除 `RealLlm`（~40 行）+ `resolve_config`（~15 行），新增 `build_review_client`（~30 行），净减约 25 行
5. **消除 tokio hack**：不再需要 `spawn_blocking` 包装
6. **未来扩展**：Review 可自然支持 streaming、thinking model 等 `LlmClient` 已有的能力
