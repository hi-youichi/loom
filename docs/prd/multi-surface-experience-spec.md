# Loom 多入口体验规范

> 状态：讨论稿<br>
> 范围：CLI、ACP/IDE、Telegram Bot；浏览器扩展作为 MCP 能力而非独立会话入口

## 1. 目标

用户可以从终端、IDE 或消息入口使用 Loom，但不应因入口不同而得到不同的项目、会话、模型、权限或终止语义。本规范定义共享契约，以及每个入口必要的呈现差异。

## 2. 共享对象

| 对象 | 统一语义 |
| --- | --- |
| Project | 有效 working directory 与其 `.loom/` 配置/产物。 |
| Session | 连续对话和会话级配置的身份；ACP `session_id` 与 Loom `thread_id` 一对一映射。 |
| Run | 一次 Agent 请求，具有开始、进度、完成/失败/取消。 |
| Context | 本次加载的记忆、技能、MCP、Agent profile、模型配置。 |
| Permission | 一次可审计的工具授权决策，遵循安全 PRD。 |
| Workflow instance | 后台工作流执行的独立可追溯对象。 |

## 3. 入口职责

| 入口 | 主要用户任务 | 必须支持 | 不强求 |
| --- | --- | --- | --- |
| CLI | 脚本化、本地调试、直接操作 | 明确目录、JSON、交互会话、取消、管理命令 | 富可视化。 |
| ACP/IDE | 编辑器内连续开发 | 流式更新、会话 load/fork、权限请求、IDE 文件/终端能力 | CLI 全部管理子命令。 |
| Telegram Bot | 轻量远程触发与通知 | 流式回复、Bot 隔离、基本会话和命令 | 高风险本地修改默认体验。 |

## 4. 一致性契约

### 4.1 项目和会话

- E-01：入口必须显示或能查询有效 working directory；ACP 接收绝对工作目录并映射到 `RunOptions::working_folder`。
- E-02：会话 ID 不得在不同项目间无提示复用。项目变更时，入口必须新建会话或明确显示切换影响。
- E-03：新建、加载、fork、reset、取消与删除的语义以共享 session/thread 模型为准；Bot 若不支持某操作，必须明确降级而非伪装支持。

### 4.2 模型、上下文和工具

- E-04：provider、model、tier、effort 的优先级在 CLI/ACP 一致；会话级持久化仅影响该会话。
- E-05：同一项目的 memory、skill、MCP 和 Agent profile 的解析规则一致，入口只改变呈现。
- E-06：工具成功、失败、拒绝和取消使用共享终止分类；CLI JSON 与 ACP 流事件可相互映射。

### 4.3 权限和可观察性

- E-07：ACP 使用协议的 `session/request_permission`；CLI 提供等价的交互提示/策略；Bot 对不能安全确认的高风险操作默认拒绝或转交确认。
- E-08：每个入口均能向用户展示任务状态、关键工具活动、最终结果、错误分类和日志/实例的下一步入口。
- E-09：协议通道纯净：ACP stdout 仅为 JSON-RPC；CLI `--json` 不混入日志；Bot 输出遵从平台长度与格式限制。

## 5. 事件映射

```text
Shared Run event       CLI              ACP/IDE                  Bot
run_started            status line      session/update           “处理中”消息
tool/phase progress    compact/verbose  tool call update          节流后的进度编辑
permission_request     prompt           request_permission        拒绝或交互确认
run_completed/failed   final summary    stop reason + content     最终消息
run_cancelled          cancelled        Cancelled stop reason     已取消消息
```

入口可以降低事件密度，但不得改变顺序、漏掉终态或把拒绝表达为成功。

## 6. 验收标准

1. 同一 fixture 项目、相同 session 配置和提示在 CLI 与 ACP 中获得相同 working directory、模型选择和终止分类。
2. ACP 的 stdout 没有配置报告、调试或普通日志；所有内容通过协议/日志文件输出。
3. CLI Ctrl+C、ACP `session/cancel` 均产生 `cancelled`，且不会在取消后追加成功终态。
4. 从 ACP fork 的会话与源会话随后执行互不污染；加载会话恢复工作目录和持久化模型设置。
5. Telegram Bot 文档明确哪些能力不可用或受限制，且不会绕过权限策略。

## 7. 发布规则

新能力先在 CLI 形成可脚本化、可测试的共享语义，再接入 ACP；Bot 只在风险、会话和观测均可解释后接入。任何入口专属扩展必须标明作用域，不能改变核心对象的含义。
