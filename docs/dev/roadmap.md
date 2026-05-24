# 路线图与风险

## 开发路线图

| Phase | 内容 | 时间 | 依赖 |
|-------|------|------|------|
| **0** | 验证 Loom/Codex IO 行为，实现 Backend trait 骨架 | 1-2 天 | - |
| **1** | MVP：`levol init/chat`，context 注入，会话录制，backend 切换 | 1 周 | Phase 0 |

### Phase 0 详细任务

- [ ] 测试 Loom CLI 的输入输出行为
- [ ] 测试 Codex CLI 的 `--quiet` + `--approval-mode full-auto` 输出格式
- [ ] 确认可行性，确定用 PTY 还是 Pipe
- [ ] 实现 Backend trait + LoomAdapter + CodexAdapter 骨架
- [ ] 搭建 Rust 项目骨架

### Phase 1 详细任务

- [ ] `levol init` 初始化数据目录（含 backend 选择）
- [ ] `levol chat` 包装底层 CLI + context 注入 + 会话录制
- [ ] `levol config set cli.backend codex` 切换 backend
- [ ] `levol sessions list` 会话历史列表
- [ ] 基础的 context 文件注入/还原机制

> Phase 2-6（Review、Skills、Curator、GEPA）的详细任务、风险和 Hermes 对比已移至 [evolution/roadmap.md](../evolution/roadmap.md)。

## 核心风险

| 风险 | 缓解 |
|------|------|
| 底层 CLI 输出格式变更 | Backend Adapter 隔离；PTY 兜底 |
| Context 文件注入冲突 | 标记区间 + 原子还原 |
| 底层 CLI 升级破坏兼容 | Adapter 隔离 + 版本适配 |

> 进化子系统的风险见 [evolution/roadmap.md](../evolution/roadmap.md)。
