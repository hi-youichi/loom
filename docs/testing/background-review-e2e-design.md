# Background Review E2E 测试设计方案

## 现状

Background review 系统 (`loom/src/background_review/`) 目前 **没有任何集成/e2e 测试**，仅有 `MemoryStore` 的单元测试 (`memory.rs`)。

核心调用链：
```
ACP agent.rs → spawn_background_review() → run_background_review_workflow()
  → run_background_review_inner() → AgentReviewRunner::run_with_refs(llm, memory, skills, session, config)
    → loop { llm.invoke() → ReviewToolExecutor.execute() }
```

## 测试策略：三层分离

### Layer 1: AgentReviewRunner 集成测试（核心，优先级最高）

测试 `AgentReviewRunner::run_with_refs` + `ReviewToolExecutor` + `MemoryStore` + `SkillRegistry` 的联动。

**位置**: `loom/tests/background_review_integration.rs`

**关键**：用 `MockLlm`（已有）替代真实 LLM，用 `tempfile::TempDir` 隔离存储。

```rust
use loom::background_review::{AgentReviewRunner, AgentReviewConfig, ReviewMode};
use loom::background_review::memory::{MemoryStore, MemoryFile};
use loom::background_review::skill_registry::SkillRegistry;
use loom::{MockLlm, LlmClient};
use loom::state::ToolCall;

#[tokio::test]
async fn review_saves_user_preference_to_memory() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    // MockLlm: 第1轮调 memory_set，第2轮无 tool_calls 退出
    let llm = MultiRoundMockLlm::new(vec![
        ("Reviewing...".into(), vec![ToolCall {
            name: "memory_set".into(),
            arguments: r#"{"file":"user","content":"- prefers dark mode"}"#.into(),
            id: Some("call-1".into()),
        }]),
        ("Nothing more to save.".into(), vec![]),
    ]);

    let config = AgentReviewConfig::default();
    let session = "User: I prefer dark mode.\nAssistant: Got it!";

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn LlmClient, &memory, &skills, session, &config,
    ).await.unwrap();

    assert_eq!(result.actions.len(), 1);
    assert!(memory.load(MemoryFile::User).unwrap().contains("dark mode"));
}
```

### Layer 2: Workflow 集成测试

测试 `run_background_review_workflow` 的 guard 条件（太短、无凭证、disabled）和副作用（history、observability）。

**位置**: `loom/tests/background_review_workflow.rs`

- 需要 `#[cfg(test)]` 导出 `run_background_review_inner` 和 `run_background_review_workflow`
- 或直接通过 `BackgroundReviewConfig` 构造调用

### Layer 3: ACP Process E2E 测试

测试完整 `loom-acp` 进程级链路：prompt → agent 完成 → background review 触发。

**位置**: `loom-acp/tests/background_review_e2e.rs`

**模式**：参照 `agent_plan_e2e.rs` 的 `wiremock` + `AcpChild::spawn`。

```rust
mod common;
mod e2e;

use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn background_review_triggered_after_prompt() {
    let mock_server = MockServer::start().await;
    // 主对话 + review 的 LLM 响应
    mount_responses(&mock_server).await;

    let mut acp = AcpChild::spawn_with_env(vec![
        ("OPENAI_BASE_URL", mock_server.uri()),
        ("OPENAI_API_KEY", "test-key"),
    ]);
    let session_id = acp.handshake(Duration::from_secs(10)).unwrap();
    acp.send_prompt(&session_id, "I prefer Rust for backend").await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    // 验证 review history 文件 或 memory 文件被更新
}
```

## 需要的 MockLlm 增强

当前 `MockLlm` 仅支持 2 轮（`first_tools_then_end`），review 需要任意轮次。建议新增：

```rust
/// 多轮 MockLlm，按预录序列返回响应
pub struct MultiRoundMockLlm {
    rounds: Vec<(String, Vec<ToolCall>)>,
    current: AtomicUsize,
}

impl MultiRoundMockLlm {
    pub fn new(rounds: Vec<(String, Vec<ToolCall>)>) -> Self { ... }
}

// 或闭包式，更灵活
pub struct FnMockLlm {
    f: Box<dyn Fn(usize, &[Message]) -> (String, Vec<ToolCall>) + Send + Sync>,
    current: AtomicUsize,
}
```

**位置**: `loom/src/llm/mock.rs` 中扩展，或 `loom/tests/multi_round_mock.rs` 独立文件。

## 需要暴露的接口

| 函数 | 当前可见性 | 建议 |
|------|-----------|------|
| `AgentReviewRunner::run_with_refs` | `pub` | 已可用 |
| `run_background_review_inner` | `async fn` (模块私有) | `#[cfg(test)] pub` |
| `run_background_review_workflow` | `async fn` (模块私有) | `#[cfg(test)] pub` |
| `ReviewToolExecutor` | `pub` | 已可用 |
| `MemoryStore::new` | `pub` | 已可用 |
| `SkillRegistry::new` | `pub` | 已可用 |

在 `loom/src/background_review/mod.rs` 添加：
```rust
#[cfg(test)]
pub use workflow::{run_background_review_inner, run_background_review_workflow};
```

## 测试用例清单

| # | 测试名 | 层级 | 验证点 |
|---|--------|------|--------|
| 1 | `review_saves_user_preference_to_memory` | L1 | memory_set → MemoryStore 写入 |
| 2 | `review_creates_new_skill` | L1 | skill_create → SkillRegistry 文件 |
| 3 | `review_patches_existing_skill` | L1 | skill_patch → 增量更新 |
| 4 | `review_checks_existing_memory_first` | L1 | 先 get 再 set 的多轮序列 |
| 5 | `review_respects_max_iterations` | L1 | 迭代上限后停止，不 panic |
| 6 | `review_truncates_long_session` | L1 | 超长 session 被截断 |
| 7 | `review_no_updates_when_nothing_to_save` | L1 | 无 tool_calls → summary 为空 |
| 8 | `review_rejects_disallowed_tools` | L1 | 非 review 白名单工具被拒绝 |
| 9 | `workflow_skips_short_session` | L2 | session < min_chars 跳过 |
| 10 | `workflow_skips_no_credentials` | L2 | 空 API key 跳过 |
| 11 | `workflow_records_history` | L2 | ReviewHistory 文件正确写入 |
| 12 | `workflow_observability_metrics` | L2 | ObservabilityStore 有指标 |
| 13 | `e2e_prompt_triggers_review` | L3 | ACP prompt 后 review 异步触发 |
| 14 | `e2e_review_updates_memory_file` | L3 | 文件系统上 memory 被更新 |

## 实施优先级

1. **P0**: `MultiRoundMockLlm` + L1 集成测试（#1-#8）— 核心逻辑覆盖
2. **P1**: L2 workflow guard 测试（#9-#12）— 边界条件
3. **P2**: L3 ACP process e2e（#13-#14）— 进程级验证，依赖 wiremock 搭建
