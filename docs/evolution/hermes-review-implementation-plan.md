# Hermes 后台 Review 完整实现计划

> 基于对 Hermes 源码分析文档 + Loom 现有代码的全面审计，分 6 个 Phase 实施闭环审查系统。
> 目标：对话结束后自动 fork 后台线程，回放对话快照，提取记忆更新 + 技能创建/修补。

---

## 现状盘点

### 已实现（可直接复用）

| 模块 | 文件 | 状态 |
|------|------|------|
| `ReviewAgent` 核心 | `cli/src/run/review.rs` | ✅ 完成 |
| `MemoryStore` | `cli/src/run/memory.rs` | ✅ 完成 |
| `SkillRegistry` | `cli/src/run/skill_registry.rs` | ✅ 完成 |
| `Curator` | `cli/src/run/curator.rs` | ✅ 完成 |
| `ReviewHistory` | `cli/src/review_history.rs` | ✅ 完成 |
| 手动 review 命令 | `cli/src/review_cmd.rs` | ✅ 完成 |
| 会话文本提取 | `cli/src/session.rs` | ✅ 完成 |
| `ReviewLlm` trait | `cli/src/run/review.rs:29-31` | ⚠️ 需改造为 async |
| `RealLlm` 同步实现 | `cli/src/review_skill_cmd.rs` | ⚠️ 需替换为 async LlmClient |

### 缺失（需新建/改造）

| 模块 | 说明 | 优先级 |
|------|------|--------|
| 自动触发机制 | 对话结束后 spawn 后台审查线程 | P0 |
| Review Prompt 升级 | 缺少信号检测/反模式/优先级链 | P0 |
| 技能优先级链 | PATCH → PATCH umbrella → support file → CREATE | P0 |
| agent-created 标记 | 后台 review 创建的技能标记来源 | P1 |
| 反模式保护 | 不保存临时故障/一次性任务 | P1 |
| LLM 客户端统一 | 消除 `RealLlm`，复用 `loom::llm::LlmClient` | P1 |
| 配置读取 | `.loom/config.yaml` 中 `review:` 配置段 | P2 |
| Memory 注入 | 会话开始时注入记忆到 system prompt | P2 |
| Skill 自动匹配 | 用户消息 → 匹配 triggers → 注入技能 | P2 |
| 安全扫描 | agent-created 技能的安全检查 | P3 |

---

## Phase 1: LLM 客户端统一 + Review Prompt 升级

### 1.1 ReviewAgent 改 async + 复用 LlmClient

**改造 `cli/src/run/review.rs`**：

- 删除 `ReviewLlm` trait
- `ReviewAgent` 去掉生命周期参数，改为 owned `Box<dyn LlmClient>`
- `review_session` 改为 `async fn`
- `self.llm.complete(prompt)` → `self.llm.invoke(&messages).await`
- 重试逻辑删除，交给 `RetryLlmClient` 包装层

**改造 `cli/src/review_skill_cmd.rs`**：

- 删除 `RealLlm` struct 和 `impl ReviewLlm for RealLlm`
- 删除 `resolve_config()` 函数
- 新增 `pub(crate) fn build_review_client(model_override: Option<&str>) -> Result<Box<dyn LlmClient>>`
  - 读取 `config::load_full_config()` → `ProviderConfig` → `create_llm_client()`
  - 包装 `RetryLlmClient::new(client).with_max_retries(3).with_base_delay(2s)`

**改造 `cli/src/review_cmd.rs`**：

- 删除 `spawn_blocking` 包装
- `do_review_single` / `do_review_batch` 改为 `async fn`
- 用 `build_review_client(args.model.as_deref())` 替换 `RealLlm::new(...)`

**改造测试**：
- `MockLlm` 改为实现 `LlmClient` trait（async）

### 1.2 Review Prompt 升级（对齐 Hermes `_COMBINED_REVIEW_PROMPT`）

当前 prompt 只要求提取"用户偏好/项目事实/事实记录/可复用模式"，需补充：

1. *触发信号说明*：何时值得保存（用户纠正风格、非平凡技巧、过时技能发现）
2. *反模式保护*：不保存的内容（临时故障、工具负面断言、一次性任务）
3. *技能优先级链*：指导 LLM 按顺序决定技能操作
4. *记忆声明式要求*："用户偏好简洁" ✓, "总是简洁回答" ✗
5. *技能命名约束*：类级别命名，禁止 PR 号/错误串

### 1.3 输出结构扩展

新增 `SkillAction` 枚举（Create / Patch）和 `target_skill` 字段。

### Phase 1 验收标准
- [ ] `ReviewLlm` trait 已删除
- [ ] `RealLlm` struct 已删除
- [ ] `ReviewAgent` 使用 `Box<dyn LlmClient>`，`review_session` 是 async fn
- [ ] Review prompt 包含触发信号、反模式、优先级链
- [ ] `cargo build --bin loom` 编译通过
- [ ] `cargo test -p cli` 全部通过
- [ ] `loom review session <id>` 端到端可用

---

## Phase 2: 自动触发机制 + 后台审查线程

### 2.1 对话结束钩子

新增 `cli/src/run/background_review.rs`：

```rust
pub fn spawn_background_review(
    session_id: String,
    llm: Box<dyn LlmClient>,
    memory: MemoryStore,
    skills: SkillRegistry,
    config: ReviewConfig,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap();
        rt.block_on(async {
            // 1. 提取会话文本
            // 2. 创建 ReviewAgent
            // 3. 执行审查
            // 4. 记录到 ReviewHistory (trigger: "auto")
        });
    })
}
```

### 2.2 错误处理

- LLM 调用失败 → `RetryLlmClient` 内部处理
- 会话文本提取失败 → 不重试，warn 日志
- JSON 解析失败 → 重试 1 次
- Memory/Skill 写入失败 → 不重试，warn 日志

### 2.3 触发集成

在 `cli/src/run_flow.rs`（或 `repl.rs`）对话结束后调用 `spawn_background_review`。

### Phase 2 验收标准
- [ ] 对话结束后自动触发后台审查（daemon 线程，不阻塞）
- [ ] 审查结果写入 `ReviewHistory`（trigger: "auto"）
- [ ] Memory 和 Skill 文件被正确更新
- [ ] `cargo build` 通过

---

## Phase 3: 技能优先级链 + agent-created 标记

### 3.1 SkillAction 枚举

```rust
pub enum SkillAction { Create, Patch, AddSupportFile }
```

### 3.2 优先级链实现

在 `ReviewAgent::apply_skill_suggestions` 中：
1. `Patch` + target_skill 存在 → 加载并修改 body
2. `AddSupportFile` + target_skill 存在 → 创建 references/templates 文件
3. `Create` → 保存新技能

`SkillRegistry` 扩展 `add_support_file()` 方法。

### 3.3 agent-created 标记

`SkillContent` 新增 `created_by: Option<String>`，auto 创建时标记为 `"auto"`。

### Phase 3 验收标准
- [ ] SkillAction 三种路径单元测试通过
- [ ] `created_by` 字段正确标记
- [ ] Review prompt 包含优先级链指令

---

## Phase 4: 配置集成 + Memory/Skill 自动注入

### 4.1 配置读取

`.loom/config.yaml` 扩展 `review`/`memory`/`skills` 段。

### 4.2 Memory 注入

会话开始时，`MemoryStore::load_all_for_prompt()` 注入 system prompt。

### 4.3 Skill 自动匹配

用户消息 → `SkillRegistry::find_matching(query, 0.6)` → 注入匹配技能到 context。

### Phase 4 验收标准
- [ ] 配置正确解析
- [ ] 记忆自动注入 system prompt
- [ ] 技能自动匹配注入
- [ ] `review.enabled: false` 时后台审查不触发

---

## Phase 5: 反模式保护 + 安全增强

### 5.1 反模式过滤器

过滤临时故障、工具负面断言、一次性任务的记忆更新。

### 5.2 安全扫描

`guard_agent_created: true` 时检查危险操作模式和 15KB 大小限制。

### 5.3 重复检测

相似度 > 0.8 的技能自动合并而非创建新技能。

### Phase 5 验收标准
- [ ] 错误描述不写入记忆
- [ ] 危险操作检测
- [ ] 高重叠技能自动合并

---

## Phase 6: 端到端集成 + 测试 + 文档

### 6.1 集成测试场景

| 场景 | 预期 |
|------|------|
| 用户纠正风格 | USER.md 追加偏好 |
| 非平凡调试 | 创建技能 |
| 简单问答 | 无更新 |
| 环境故障 | 不保存（反模式） |
| 已有技能过时 | Patch 技能 |
| review.enabled: false | 不触发 |

### 6.2 文档更新

`docs/evolution/` 下 5 个文件更新 + 新增 `background-review.md`。

### 6.3 性能控制

辅助模型、24000 字符限制、60s 超时、并发 1。

---

## 文件变更总览

**新增**：`background_review.rs`, `evolution_config.rs`

**修改**：`review.rs`, `review_skill_cmd.rs`, `review_cmd.rs`, `agent.rs`, `skill_registry.rs`, `main.rs`, `run_flow.rs`, `.loom/config.yaml`

## 预估

| Phase | 时间 |
|-------|------|
| Phase 1 | 2-3 天 |
| Phase 2 | 2-3 天 |
| Phase 3 | 2-3 天 |
| Phase 4 | 2-3 天 |
| Phase 5 | 1-2 天 |
| Phase 6 | 2-3 天 |
| **合计** | **11-17 天** |

最小可行路径：Phase 1 + Phase 2（约 3-4 天跑通自动审查）。
