# 代码与文档清理方案 **[已完成]**

> **状态**: ✅ 已完成。清理方案中的问题已修复，本文档保留作为历程记录。

> 审计日期: 2026-05-25
> 审计范围: 整个 workspace（16 crates + docs + config）

---

## 一、审计发现总览

| 类别 | 严重度 | 数量 | 状态 |
|------|--------|------|------|
| 🔴 第三方完整仓库嵌入 | P0 | 1 | 待清理 |
| 🟠 重复代码 | P1 | 2 处 | 待清理 |
| 🟡 `#[allow(dead_code)]` 掩盖的死代码 | P2 | 12 处 | 待评估 |
| 🟡 TODO/FIXME 注释 | P2 | 7 处（项目内） | 待处理 |
| 🟡 过时的方案/计划文档 | P2 | 12 个 | 待归档 |
| 🟢 Clippy 警告 | P3 | 3 个 | 待修复 |
| 🟢 `#[allow(unused)]` 掩盖的问题 | P3 | 33 处 | 待评估 |

---

## 二、详细清单

### 2.1 🔴 P0: `penpot-reference/` — 完整第三方仓库嵌入

**问题**: `penpot-reference/` 是 Penpot 设计工具的**完整源码**（Clojure/ClojureScript/WASM），包含：
- 完整的 backend/（Clojure）、frontend/（ClojureScript）、render-wasm/（Rust）
- 47+ 个 FIXME、30+ 个 TODO 注释
- `.github/workflows/`、`.opencode/`、`backend/dev/` 等
- 大量非本项目代码

**影响**:
- 严重污染搜索结果（grep/glob 匹配到大量无关代码）
- 占用磁盘空间
- 干扰代码审计工具
- 增加仓库 clone 时间

**方案**:
```
1. 如果仅为参考用途 → 移至独立仓库，用 git submodule 或纯文档引用
2. 如果完全不需要 → 从工作区删除
3. 最小化方案 → 添加到 .gitignore，不纳入日常工作区
```

**预估工作量**: 0.5h

---

### 2.2 🟠 P1: 重复代码

#### 2.2.1 Spinner 重复实现

| 文件 | 大小 | 用途 |
|------|------|------|
| `cli/src/run/spinner.rs` | 7,457B | CLI 专用的 Spinner（`SpinnerTrait` + `Spinner` + `NoopSpinner`） |
| `loom/src/stream_display/spinner.rs` | 9,046B | loom lib 专用的 Spinner（`Spinner` + `SpinnerTrait` + `QuietSpinner`） |

两者都实现了 `SpinnerTrait` trait、一个实际 spinner 和一个 no-op spinner，但代码完全独立。

**方案**:
- 将 `SpinnerTrait` 和共享实现提取到 `loom` 中作为 pub trait/pub struct
- `cli` 通过 `use loom::...` 引用
- 删除 `cli/src/run/spinner.rs` 中的重复代码

**预估工作量**: 2h

#### 2.2.2 Retry 逻辑重复

| 文件 | 位置 |
|------|------|
| `loom/src/http_retry.rs` | 通用 HTTP 重试逻辑 |
| `telegram-bot/src/streaming/retry.rs` | Telegram 特定重试，包含 4 个 `#[allow(unused_assignments)]` |

**方案**: 统一为 `loom` 中的通用重试层，Telegram bot 使用组合而非复制。

**预估工作量**: 1h

---

### 2.3 🟡 P2: `#[allow(dead_code)]` 掩盖的死代码

以下代码被 `#[allow(dead_code)]` 标记，需要逐个评估是保留还是删除：

| 文件 | 位置 | 说明 |
|------|------|------|
| `loom/src/llm/openai_compat.rs:201` | `max_output_tokens()` | OpenAI API 参数，未来可能使用 |
| `loom/src/llm/openai_compat.rs:263-267` | `ChatMessage` 字段 | OpenAI 消息格式辅助 |
| `cli/src/display_limits.rs:11` | `truncate_message()` | 未使用的工具函数 |
| `cli/src/run/spinner.rs:15` | `SpinnerMsg::Update` | Spinner 协议一部分 |
| `cli/src/run/spinner.rs:22` | `SpinnerTrait::update()` | trait 方法 |
| `cli/src/run/spinner.rs:67` | `Spinner::update()` | 实际方法 |
| `cli/src/run/spinner.rs:188-191` | `NoopSpinner` 及方法 | 静默模式预留 |
| `loom/src/stream_display/spinner.rs` | 类似 spinner 死代码 | 同上 |
| `loom/src/llm/mod.rs` | 部分 LLM 配置 | 预留功能 |

**方案**:
1. 审计每个 `#[allow(dead_code)]`，标注"保留原因"
2. 如果只是预留且无近期计划 → 删除，未来需要时再添加
3. 如果是公共 API → 添加文档注释说明

**预估工作量**: 2h

---

### 2.4 🟡 P2: TODO/FIXME 注释（项目内，不含 penpot）

| 文件 | 行号 | 注释 | 建议 |
|------|------|------|------|
| `loom-acp/src/agent.rs` | 965 | `TODO: Connect MCP servers from request` | 创建 issue 跟踪 |
| `loom-acp/src/agent.rs` | 1221 | `TODO: Store cwd in checkpoints` | 创建 issue 跟踪 |
| `loom-acp/tests/agent_plan_e2e.rs` | 367 | `TODO: Sub-agent events not propagating` | 已知缺陷，创建 issue |
| `loom-acp/tests/agent_plan_e2e.rs` | 411 | `TODO: Sub-agent events not propagating` | 同上 |
| `loom-acp/tests/agent_plan_e2e.rs` | 469 | `TODO: Sub-agent events not propagating` | 同上 |
| `loom/src/lsp/sync.rs` | 67 | `TODO: Implement proper diff algorithm` | LSP 预留功能 |
| `loom/src/lsp/manager.rs` | 116 | `TODO: Use workspace root` | LSP 预留功能 |
| `loom/src/lsp/client.rs` | 487/493 | `TODO: Implement workspace folder support` | LSP 预留功能 |

**方案**:
1. 创建 GitHub issues 跟踪所有 TODO
2. 对无法短期实现的，标注 `// TODO(长期):` 前缀
3. 对已知缺陷，标注 `// FIXME(BUG-xxx):` 关联 issue

**预估工作量**: 1h

---

### 2.5 🟡 P2: 过时的方案/计划文档

以下文档看起来是一次性方案/计划文档，已完成或过时：

| 文件 | 说明 | 建议 |
|------|------|------|
| `docs/DOC-PLAN.md` | 文档重组计划 | 归档到 `docs/archive/` |
| `docs/DOC-REORGANIZE-PLAN.md` | 文档重组方案 | 归档到 `docs/archive/` |
| `docs/dev/impl/cli-ux-improvement.md` | CLI UX 改进计划 | 已移至 docs/dev/impl/ |
| `docs/dev/impl/cli-ux-improvement.zh.md` | CLI UX 改进方案（中文） | 已移至 docs/dev/impl/ |
| `docs/hermes-review-and-plan.md` | Hermes 审查方案 | 归档到 `docs/archive/` |
| `docs/llm-tool-dev-plan.md` | LLM 工具开发方案 | 归档到 `docs/archive/` |
| `docs/plan-browser-extension.md` | 浏览器扩展方案 | 归档或删除 |
| `docs/design/goal-external-loop-dev-plan.md` | Goal 外部循环方案 | 归档 |
| `docs/dev/acp/acp-session-load-e2e-plan.md` | ACP 会话加载方案 | 归档 |
| `docs/evolution/hermes-review-implementation-plan.md` | Hermes 审查实现方案 | 归档 |
| `docs/evolution/implementation-plan.md` | 进化实现方案 | 归档 |
| `docs/evolution/rfc-review-command.md` | RFC 审查命令方案 | 归档 |

**方案**:
1. 创建 `docs/archive/` 目录
2. 将已完成的方案文档移入 `docs/archive/`
3. 在归档文档头部添加 `<sup>⚠️ 已归档 — {日期}</sup>` 标记
4. 保留 `docs/evolution/` 中的参考文档（commands.md, config.md, decisions.md）

**预估工作量**: 1h

---

### 2.6 🟢 P3: Clippy 警告

| 文件 | 警告 | 说明 |
|------|------|------|
| `loom/src/stream_display/format_subagent.rs:3` | unused import: `Duration` | 删除未使用的 import |
| `loom/src/stream_display/format_subagent.rs:46` | unused variable: `node_id` | 改为 `node_id: _` |
| `loom/src/stream_display/format_subagent.rs:49` | unused variable: `node_id` | 改为 `node_id: _` |
| `loom/src/stream_display/format_subagent.rs:93` | unreachable pattern | 移除 `_ => None` 分支 |

**预估工作量**: 0.5h

---

### 2.7 🟢 P3: `#[allow(unused)]` 压制的警告

共 33 处 `#[allow(unused_*')]`，分布在：

| Crate | 数量 | 说明 |
|-------|------|------|
| `loom-acp/tests/` | 5 | 测试 mock 代码 |
| `telegram-bot/src/streaming/retry.rs` | 4 | 未使用赋值（代码异味） |
| `loom/src/lib.rs` (tests) | 1 | `#[cfg(unix)]` 条件导入 |
| 其他测试文件 | 23 | 测试辅助代码 |

**方案**:
1. 测试中的 `#[allow(unused)]` → 可接受，保留
2. `telegram-bot/src/streaming/retry.rs` 的 4 个 `#[allow(unused_assignments)]` → 代码异味，重构消除
3. 生产代码中的 → 逐个审计

**预估工作量**: 1h

---

## 三、执行计划

### Phase 1: 快速清理（2h）

| 步骤 | 操作 | 影响 |
|------|------|------|
| 1.1 | 处理 `penpot-reference/`（删除或移除引用） | 消除最大的垃圾源 |
| 1.2 | 修复 clippy 警告（format_subagent.rs） | 0 warning |
| 1.3 | 创建 `docs/archive/` 并归档 12 个过时方案 | 文档清晰 |

### Phase 2: 死代码清理（3h）

| 步骤 | 操作 | 影响 |
|------|------|------|
| 2.1 | 审计 12 处 `#[allow(dead_code)]` | 明确保留或删除 |
| 2.2 | 删除确认无用的死代码 | 减少维护负担 |
| 2.3 | 将 TODO/FIXME 转为 GitHub Issues | 可追踪 |

### Phase 3: 去重（3h）

| 步骤 | 操作 | 影响 |
|------|------|------|
| 3.1 | 提取共享 Spinner 到 loom | 消除 7KB 重复 |
| 3.2 | 重构 Telegram retry 逻辑 | 消除代码异味 |
| 3.3 | 清理 `#[allow(unused_assignments)]` | 代码更健壮 |

### 总预估工作量: 8h

---

## 四、验收标准

- [ ] `cargo clippy` → 0 warnings, 0 errors
- [ ] `cargo build` → 0 warnings, 0 errors
- [ ] `penpot-reference/` 不再在工作区中（或已 .gitignore）
- [ ] `docs/archive/` 包含所有已归档方案
- [ ] 所有 TODO/FIXME 都有对应的 GitHub issue
- [ ] 无重复 Spinner 实现
- [ ] 所有 `#[allow(dead_code)]` 都有明确保留原因的注释
