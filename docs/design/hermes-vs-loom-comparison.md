# Hermes Agent vs Loom Agent 功能完整性对比

**分析日期：** 2025-08-19
**结论：** Loom 已实现 Hermes 约 55-60% 的核心功能。

---

## 1. Agent 核心架构

| 功能 | Hermes | Loom |
|------|--------|------|
| ReAct 循环 | ✅ | ✅ |
| 多种 Agent 模式 | ❌ | ✅ DUP/GoT/ToT |
| 子代理系统 | ✅ | ✅ invoke_agent |
| 模型降级/Failover | ✅ CredentialPool | ❌ |
| 消息消毒 | ✅ | ⚠️ 仅 UTF-8 截断 |
| Prompt Caching | ✅ | ❌ |

**关键差异**
- **Failover**：Hermes 有 CredentialPool + probe_tier；Loom 无 API key 池
- **消息安全**：Hermes 有消毒机制；Loom 仅基础 UTF-8 截断

---

## 2. 工具系统

### 已实现
- 文件操作（ls/read/write/edit/move/delete/glob/grep）
- 网络工具（web_fetcher、web_search）
- 系统工具（bash、powershell、batch）
- 任务工具（task_create/update/show/list/delete）
- Todo 工具
- MCP 适配器

### 缺失

| 工具 | Hermes | Loom |
|------|--------|------|
| 浏览器工具 | ✅ | ❌ |
| 图像/视频生成 | ✅ | ❌ |
| 代码执行沙箱 | ✅ | ❌ |
| 工具审批系统 | ✅ | ❌ |

---

## 3. 记忆系统

| 功能 | Hermes | Loom |
|------|--------|------|
| 文件持久化 | ✅ MEMORY/USER | ✅ USER/PROJECT/FACTS |
| 工具 | ✅ | ✅ |
| 插件架构 | ✅ MemoryProvider | ❌ |
| Char Limit | ✅ 2200 | ❌ |

---

## 4. 任务系统

| 功能 | Hermes | Loom |
|------|--------|------|
| 完整 CRUD | ✅ | ✅ |
| SQLite 持久化 | ❌ | ✅ task-core |
| MCP 服务器 | ❌ | ✅ task-mcp-server |
| 批量并行子任务 | ✅ | ❌ |

---

## 5. 进化系统

| 功能 | Hermes | Loom |
|------|--------|------|
| GEPA 优化器 | ❌ | ✅ |
| 约束检查 | ❌ | ✅ |
| 数据集管理 | ❌ | ✅ JSONL |
| RunStore | ❌ | ✅ |
| evolve run CLI | ❌ | ⚠️ stub |

**Hermes 进化方式**：通过 background_review + skill_manage 让 LLM 修改技能文件
**Loom 进化方式**：GEPA 优化器 + JSONL 数据集 + 后台 review

---

## 6. MCP 集成

| 功能 | Hermes | Loom |
|------|--------|------|
| MCP 协议栈 | ✅ | ✅ mcp-rust |
| 传输层 | stdio/HTTP | stdio/HTTP/WS |
| 动态注册 | ✅ | ❌ |
| ACP stream bridge | ❌ | ✅ |

---

## 7. CLI 命令

| 命令 | Hermes | Loom |
|------|--------|------|
| goal/task/evolve/review | ✅ | ✅ |
| skills/models/tools | ✅ | ✅ |
| agent profiles | ❌ | ✅ |
| evolve run | ❌ | ⚠️ stub |
| Slash Command | ✅ | ❌ |
| TUI Gateway | ✅ | ❌ |

---

## 8. 配置系统

| 功能 | Hermes | Loom |
|------|--------|------|
| Provider 配置 | ✅ | ✅ |
| Tier 系统 | ❌ | ✅ |
| Credential Pool | ✅ | ❌ |
| 多 Provider | Bedrock/Azure/Gemini | ❌ |

---

## 汇总：Loom 缺失的关键功能

### P0 - 生产可用性

| 功能 | 影响 |
|------|------|
| API Failover / Credential Pool | 单点 API 失败即崩溃 |
| evolve run CLI stub | 进化命令不工作 |
| 消息消毒 | CJK 字符有风险 |

### P1 - 用户体验

| 功能 | 影响 |
|------|------|
| 工具审批系统 | 无安全确认 |
| Context Compressor | 长对话撞 token limit |
| Prompt Caching | 浪费 token |
| Slash Command | 无法运行时切换 |

### P2 - 扩展能力

| 功能 |
|------|
| Browser 工具集 |
| 代码执行沙箱 |
| 多 Provider 适配器 |
| TUI Gateway / Web UI |

---

## 总结

### Loom 优势
1. Rust 实现，性能更高
2. 结构化 Agent Profiles
3. GEPA 进化优化器
4. 多种 Agent 模式（DUP/GoT/ToT）
5. 完整 MCP 协议栈

### Hermes 优势
1. 生产级可靠性（Credential Pool）
2. 丰富工具集（浏览器/OAuth/沙箱）
3. TUI Gateway Web UI
4. 多 Provider 支持
5. Slash Command 系统

### 建议
优先补足 P0：evolve run、Credential Pool、CJK 消毒
