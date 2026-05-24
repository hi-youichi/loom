# 关键设计决策记录

记录"为什么这样设计"的决策，避免后来者重复争论。

> 进化相关的决策（D3 文件系统、D6 纯 Rust 进化模块、D7 Review 异步）已移至 [evolution/decisions.md](../evolution/decisions.md)。

## D1: 追加-还原模式注入 Context

**决策**：在 CLAUDE.md / AGENTS.md 末尾追加 Levol 上下文，会话结束后还原。

**备选方案**：
1. ✅ 追加-还原（选择）
2. ❌ 单独 `.levol-context.md` 文件
3. ❌ 环境变量注入

**理由**：
- 追加-还原零配置，底层 CLI 天然读取自己的 context 文件
- 单独文件需要底层 CLI 支持 `--context-file` flag，Loom 和 Codex 都不支持
- 环境变量需要底层 CLI 的 Agent 能读取，不可靠

**风险**：用户可能在会话中手动编辑 context 文件。缓解：用 `<!-- levol-context-start/end -->` 标记，还原时精确删除。

## D2: Stdout Pipe 而非 PTY

**决策**：起步用 Stdout pipe + 解析器，后续按需升级 PTY。

**理由**：
- Stdout pipe 实现简单（几行 Rust），能满足大部分场景
- PTY 需要额外依赖（`portable-pty`），跨平台兼容性问题多
- 如果 pipe 方案不够，再升级不迟

**何时升级**：如果 Loom/Codex 的输出包含 ANSI 转义序列或交互式 UI 元素导致解析失败。

## D4: Backend Adapter 模式

**决策**：通过 trait 抽象底层 CLI，每个 CLI 实现自己的 Adapter。

**理由**：
- Loom 和 Codex 的命令行参数、输出格式完全不同
- Adapter 模式让上层代码不感知底层差异
- 未来支持新 CLI（Claude Code、Aider 等）只需新增一个 Adapter 文件

## D5: Rust 而非 Python/TypeScript

**决策**：用 Rust 实现 Levol 核心。

**备选方案**：
1. ✅ Rust（选择）
2. ❌ Python
3. ❌ TypeScript

**理由**：
- 和 Loom 同生态（Rust），未来可能集成更深入
- 单二进制分发，无运行时依赖
- Python 已经用于进化模块（DSPy），核心用 Rust 可以隔离
- TypeScript 需要安装 Node.js

**代价**：Rust 编译慢，开发效率低于 Python/TS。但 Levol 代码量不大（~4000 行），可接受。
