# Session List Release Sign-off

> 本文件是 `session-list-redesign.md` §12 的签收模板。发布前复制一份，填入实际提交版本、CI run 和 artifact 链接；未完成项必须保留为 `FAIL` 或 `BLOCKED`，不能用“未验证”替代通过。

## 1. 版本与环境

| 项目 | 值 |
| --- | --- |
| Loom commit / tag | `TODO` |
| Loom Desk commit / tag | `TODO` |
| ACP spec revision | `37-session-list.md` / `TODO` |
| Loom binary(s) | `TODO` |
| Binary SHA-256 / size | `TODO` |
| Desk build | `TODO` |
| 签收日期（UTC） | `TODO` |
| 签收人 | `TODO` |

## 2. 自动化门禁

| 门禁 | 命令 / CI job | 结果 | 证据 |
| --- | --- | --- | --- |
| Loom lib tests | `cargo test -p loom-acp --lib -- --test-threads=1` | `TODO` | `TODO` |
| Loom checks | `cargo check -p loom-acp --tests` | `TODO` | `TODO` |
| Loom clippy | `cargo clippy -p loom-acp --lib -- -D warnings` | `TODO` | `TODO` |
| Model-free canonical wire | `cargo test -p loom-acp --test e2e_session_list -- --nocapture` | `TODO` | `TODO` |
| New/old binary runner | `scripts/run-session-list-compat.ps1 -NewLoomBinary ... -OldLoomBinary ...` | `TODO` | `manifest.json` / logs |
| Desk session tests | `bun run test:session` | `TODO` | `TODO` |
| Desk type-check/lint | `bun run --cwd packages/ui type-check` / `lint` | `TODO` | `TODO` |
| Cross-platform wire CI | `session-list-compat` workflow | `TODO` | CI run + artifacts |
| Cross-platform performance | `session-index-performance` workflow | `TODO` | per-OS JSON artifacts |

## 3. Protocol and compatibility assertions

| Assertion | Result | Evidence |
| --- | --- | --- |
| New peer advertises `list` and `list-global` during migration | `TODO` | `TODO` |
| Canonical list uses stable snapshot/cursor and cursor-only continuation | `TODO` | `TODO` |
| Legacy projection omits `revision` / `indexVersion` | `TODO` | `TODO` |
| Standard ACP `session/list` remains active, owner/cwd scoped and single-page | `TODO` | `TODO` |
| Archive response/event and archived projection agree | `TODO` | `TODO` |
| Delete returns durable tombstone and authoritative absence | `TODO` | `TODO` |
| Legacy alias metrics endpoint reports calls | `TODO` | `TODO` |
| Business/permission/storage errors never trigger legacy fallback | `TODO` | `TODO` |
| New Desk + old Loom, old Desk + new Loom, new Desk + new Loom | `TODO` | request logs |

## 4. Migration and rollback

| Check | Result | Evidence |
| --- | --- | --- |
| Backup includes `memory.db`, `memory.db-wal`, `memory.db-shm` when present | `TODO` | `TODO` |
| Startup migration completes atomically | `TODO` | logs |
| `PRAGMA integrity_check` passes | `TODO` | output |
| `PRAGMA foreign_key_check` passes | `TODO` | output |
| Orphan metadata count is zero or explicitly accepted | `TODO` | query output / decision |
| Rollback/resync rehearsal completed | `TODO` | runbook + result |

## 5. Alias removal decision

| Requirement | Result | Evidence |
| --- | --- | --- |
| `legacyListGlobalCalls=0` for 14 consecutive production days | `TODO` | metrics export |
| Minimum supported Desk version contains `listIndex` | `TODO` | release matrix |
| Rollback window and owner approved | `TODO` | approval reference |
| Alias removal commit and negative fixture recorded | `TODO` | commit / CI |

## 6. Decision

- Overall result: `PASS` / `FAIL` / `BLOCKED`
- Failed or blocked items: `TODO`
- Follow-up owner and due date: `TODO`
- Approver: `TODO`

## 7. 本地验证快照（非发布签收）

以下结果来自 2026-08-22 的工作区，只证明当前源码在本地环境可通过；不能替代上方的跨版本、跨平台、生产观测和数据库恢复证据。

| 范围 | 命令/环境 | 结果 |
|---|---|---|
| Loom 单元测试 | `cargo test -p loom-acp --lib -- --test-threads=1` | `602 passed, 0 failed` |
| Loom clippy | `cargo clippy -p loom-acp --lib -- -D warnings` | 通过 |
| Loom canonical wire | `CARGO_TARGET_DIR=target/session-list-build cargo test -p loom-acp --test e2e_session_list -- --nocapture` | `1 passed` |
| Loom legacy wire | 同上，另设 `LOOM_SESSION_LIST_EXPECT_LEGACY=1` | `1 passed` |
| Desk session gate | `packages/ui: bun run test:session` | `65/65`，12 个测试文件，183 个 expect |
| Desk 静态检查 | `bun run type-check`、`bun run lint` | 通过 |
| 文档结构 | `bun run docs:validate` | 387 pages，43 sidebar links |

默认 `target` 的 wire 测试曾因 Windows 文件锁无法链接；使用独立 `target/session-list-build` 后 canonical 与 legacy 均通过。该现象不应记录为协议失败。
