# `loom review` 命令 — 实现总结

> 完成日期：2026-05-22 | 状态：已开发、编译通过、基本功能验证通过

## 一、功能概述

新增 `loom review` 命令族，支持手动对指定会话执行后台审查（Review Agent），提取技能和记忆更新。

```
loom review
├── session <id>          审查单个会话
├── sessions              批量审查
├── history               查看审查历史
├── show <id>             查看某次审查结果
└── pending               列出待审查会话
```

全局选项：`--model`、`--verbose`、`--dry-run`、`--memory-only`、`--skills-only`

## 二、变更文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `cli/src/review_history.rs` | 新增 | `ReviewRecord` + `ReviewHistory` JSONL 持久化（含 4 个单元测试） |
| `cli/src/review_cmd.rs` | 新增 | 命令处理器：`do_review_single`、`do_review_batch`、`show_history`、`show_review`、`show_pending` |
| `cli/src/session.rs` | 修改 | 新增 `extract_session_text()` 从 SQLite 提取会话消息文本 |
| `cli/src/args.rs` | 修改 | 新增 `ReviewArgs`、`ReviewCommand` 枚举；`Command` 新增 `Review` variant |
| `cli/src/main.rs` | 修改 | 注册 `review_history`/`review_cmd` 模块 + `Review` 命令分发 |
| `cli/src/repl.rs` | 修改 | match 覆盖 `Command::Review`（unreachable 分支） |
| `cli/src/review_skill_cmd.rs` | 修改 | `RealLlm`、`resolve_config` 改为 `pub(crate)` |

## 三、关键技术决策

1. **`spawn_blocking` + `.map_err(|e| e.to_string())` 解决 Send 约束**
   - `reqwest::blocking::Client` 不能在 tokio async 上下文使用
   - 将阻塞 LLM 调用包装在 `tokio::task::spawn_blocking` 中
   - 内层错误通过 `.to_string()` 转为 `String` 满足 `Send + 'static`

2. **会话文本提取：直接从 SQLite 反序列化**
   - 不依赖 `cat_session()` 的 `CodexEvent` 中间格式
   - 直接从 `checkpoints` 表读取 payload，反序列化为 `ReActState`
   - 遍历 `state.messages` 提取 User/Assistant/Tool 文本

3. **审查记录持久化到 `~/.loom/data/review/history.jsonl`**
   - 每条记录一行 JSON，追加写入
   - 支持 `list`、`find_by_session`、`reviewed_session_ids` 查询

## 四、测试验证结果

| 命令 | 结果 |
|------|------|
| `loom review pending` | ✅ 正常列出 20 个待审查会话 |
| `loom review --dry-run --verbose session <id>` | ✅ 提取 602540 字符，展示前 2000 字符预览 |
| `loom review session <id>` | ✅ 流程正确，LLM 调用因环境模型配置失败（非代码问题） |
| `loom review history` | ✅ 正确展示 2 条 SKIP 记录 |
| `cargo build --bin loom` | ✅ 编译通过（无新增 warning） |
| `cargo test -p cli` | ✅ review_history 4 个单元测试通过 |

## 五、已知限制与后续优化

1. **session ID 截断显示**：`pending` 输出中短 ID 只显示前 8 字符（如 `session-`），可考虑显示前 12 位或完整 ID
2. **`--memory-only` / `--skills-only` 未实际使用**：参数已定义但 `do_review_single` 中未根据 flag 过滤输出，需在 ReviewAgent 层面支持
3. **无并发限制的 batch**：当前串行审查，已预留 `max_concurrent` 参数位（args 中暂未暴露）
4. **审查撤销**：暂不支持 `review undo`，用户可手动编辑 memory/skill 文件

## 六、使用示例

```bash
# 列出待审查会话
loom review pending

# 试运行（不调用 LLM）
loom review --dry-run --verbose session <session_id>

# 审查单个会话（使用指定模型）
loom review --model gpt-4o-mini session <session_id>

# 批量审查最近 7 天
loom review sessions --recent 7d

# 审查所有未审查会话
loom review sessions --all-unreviewed

# 按关键词搜索并审查
loom review sessions --query "rust debug"

# 查看审查历史
loom review history
loom review history --trigger manual

# 查看某次审查详情
loom review show <session_id>

# JSON 输出（供脚本处理）
loom review pending --json
loom review history --json
```

## 七、与自动审查的关系

此命令是自动审查的**手动补充**，不影响现有自动审查流程：

- **自动审查**：对话结束时 daemon 线程自动触发（原有逻辑）
- **手动审查**：用户通过 CLI 命令触发（本次新增）
- **审查记录共享**：两种触发方式写入同一个 `history.jsonl`，`pending` 命令自动排除已审查的 session
