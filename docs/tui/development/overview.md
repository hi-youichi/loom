# Loom TUI 开发方案概览

## 概述

本文档是 Loom TUI 的**总体开发方案**，定义从零开始构建 Loom TUI 的完整路径。基于 Codex CLI TUI 的成熟架构（237K 行已验证代码）进行合理简化，适配 Loom 的实际需求。

**核心目标**：为 Loom CLI 提供一个可选的交互式 TUI 模式（`--interactive` / `-i`），在终端内联视图中实现对话式 AI 编程体验。

**参考文档**：
- Codex CLI TUI 产品分析：`docs/tui/product/product.md`
- Codex CLI TUI 交互设计：`docs/tui/interaction/reference-codex.md`
- Codex CLI TUI 工程架构：`docs/tui/design/engineering.md`
- Loom TUI 交互架构：`docs/tui/interaction/architecture.md`
- Loom TUI 产品架构：`docs/tui/design/product-architecture.md`
- 现有实现方案：`docs/tui/design/impl-plan.md`

---

## 1. 架构原则

### 1.1 核心设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 视图模式 | **内联视图**（非全屏） | 融入终端工作流，保留 shell 历史 |
| UI 框架 | **ratatui + crossterm** | 成熟可靠，Codex 已验证 |
| 运行模式 | **双模式**（TUI + 传统 ANSI） | 向后兼容，渐进采用 |
| 编译方式 | **条件编译**（feature flag） | 不增加传统模式依赖 |
| 模块拆分 | **按职责拆分，单文件 ≤500 行** | 避免 Codex 单文件过大问题 |
| 状态管理 | **App 持有单一状态** | 状态一致性好，无需跨组件同步 |

### 1.2 与 Codex 的关键差异

| 维度 | Codex CLI | Loom CLI |
|------|-----------|----------|
| 后端通信 | in-process app server (内部协议) | StreamEvent 事件流 (已定义) |
| 事件类型 | AppServer 通知 | StreamEvent + CodexEvent |
| 交互模式 | 始终运行守护进程 | 可选 `--interactive` 模式 |
| 现有输入 | 无（纯 TUI） | 已有 `repl.rs` 简单 REPL |
| 审批流程 | 完整的 approval_overlay | 可复用 `approval.rs` (codex bridge) |
| 多代理 | 完整的 agent_navigation | 暂不实现，后续扩展 |

### 1.3 模块层级

```
┌─────────────────────────────────────────────────────────────────┐
│  5. 应用层 (App Layer)                                           │
│  App 主循环 · 会话管理 · 配置管理                                │
├─────────────────────────────────────────────────────────────────┤
│  4. 交互层 (Interaction Layer)                                   │
│  输入框 · 审批弹窗 · 选择列表 · 状态指示                        │
├─────────────────────────────────────────────────────────────────┤
│  3. 渲染层 (Rendering Layer)                                     │
│  Renderable trait · 布局组件 · 历史渲染 · Spinner                │
├─────────────────────────────────────────────────────────────────┤
│  2. 终端层 (Terminal Layer)                                      │
│  内联视图 · 事件系统 · 历史行插入 · ^Z 暂停                     │
├─────────────────────────────────────────────────────────────────┤
│  1. 基础设施层 (Infrastructure Layer)                             │
│  ratatui · crossterm · tokio · stream-event                      │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. 代码库现状

### 2.1 现有 CLI 结构

```
apps/cli/src/
├── main.rs                    # 入口：CLI 参数解析
├── lib.rs                     # 模块声明
├── repl.rs                    # 简单 REPL 循环（stdin 行读取）
├── run/agent.rs               # Agent 运行（回调模式）
├── run_flow.rs                # 运行流程控制
├── display/                   # ANSI 显示系统（现有）
│   ├── mod.rs                 # 公共导出
│   ├── event_handler.rs       # 流事件回调 → 格式化输出
│   ├── format.rs              # 格式化工具
│   ├── markdown.rs            # markdown 渲染
│   ├── spinner.rs             # 单行 \r 动画 spinner
│   ├── streaming_markdown.rs  # 流式 markdown 渲染
│   ├── terminal.rs            # 终端检测 + ANSI 包装
│   ├── tool_preview.rs        # 工具预览格式
│   └── tool_summary.rs        # 工具调用摘要格式
├── output.rs                  # 输出管理
├── envelope.rs                # 事件信封处理
├── codex_event_builder.rs     # Codex 事件构建
└── ...
```

### 2.2 现有能力评估

| 特性 | 状态 | 说明 |
|------|------|------|
| 流式输出 | ✅ 已有 | `StreamingMarkdownRenderer` |
| Spinner | ✅ 已有 | 单行 \r 动画 |
| 面板格式 | ✅ 已有 | `_CATEGORY message` 格式 |
| 终端检测 | ✅ 已有 | `get_terminal_width()`, `is_tty()` |
| 分页器 | ✅ 已有 | `minus` crate |
| Codex 事件桥接 | ✅ 已有 | `experimental/codex/src/event_bridge.rs` |
| 审批管理器 | ✅ 已有 | `experimental/codex/src/approval.rs` |
| **内联视图** | ❌ 无 | 需新建 |
| **交互式输入** | ⚠️ 部分 | `repl.rs` 简单行读取，非 TUI 输入框 |
| **审批弹窗** | ❌ 无 | 需新建 |
| **事件驱动主循环** | ❌ 无 | 需新建 `tokio::select!` 循环 |
| **^Z 暂停** | ❌ 无 | 需新建 |
| **桌面通知** | ❌ 无 | 需新建 |

### 2.3 可复用组件

| 现有组件 | 复用方式 | 文件 |
|----------|----------|------|
| `display/spinner.rs` | 直接复用帧动画 | 保持 `SpinnerTrait` 兼容 |
| `display/terminal.rs` | 复用终端检测 | `get_terminal_width()`, `is_tty()` |
| `display/streaming_markdown.rs` | TUI 中渲染为流式文本 | 渲染器复用 |
| `display/markdown.rs` | 渲染到 history cell | markdown → ratatui text |
| `codex_event_builder.rs` | 事件构建逻辑 | 复用 |
| `experimental/codex/src/event_bridge.rs` | StreamEvent → CodexEvent 转换 | 核心桥接逻辑 |
| `experimental/codex/src/approval.rs` | 审批管理 | 审批状态机 |

---

## 3. 分阶段实现计划

### 3.1 阶段总览

| 阶段 | 名称 | 依赖 | 预计文件数 | 预计代码行数 |
|------|------|------|-----------|-------------|
| **Phase 1** | 基础设施 | crossterm | 6-8 个文件 | ~800 行 |
| **Phase 2** | 渲染系统 | ratatui | 8-10 个文件 | ~1200 行 |
| **Phase 3** | 交互系统 | Phase 1+2 | 10-12 个文件 | ~2000 行 |
| **Phase 4** | 集成与优化 | Phase 3 | 4-6 个文件 | ~800 行 |
| **总计** | | | **28-36 个文件** | **~4800 行** |

> 对比：Codex TUI 为 ~109 个文件 / 237K 行。Loom TUI 的简化版本约为其 2%。

### 3.2 Phase 1: 基础设施（优先级: 最高）

**目标**：建立 TUI 基础运行环境，不依赖 ratatui，纯 crossterm 实现

| 任务 | 文件 | 说明 |
|------|------|------|
| 1.1 终端初始化 | `tui/terminal.rs` | raw mode、bracketed paste、panic hook |
| 1.2 事件系统 | `tui/event.rs` | `TuiEvent` + `EventBroker` + 事件流 |
| 1.3 内联视图 | `tui/viewport.rs` | viewport 位置管理、光标探测 |
| 1.4 历史行插入 | `tui/history.rs` | 历史行缓冲与 flush |
| 1.5 App 主循环 | `tui/app.rs` | `tokio::select!` 事件分发骨架 |
| 1.6 模块入口 | `tui/mod.rs` | 公共导出 + 条件编译 |

**详细方案**：`docs/tui/development/phase-1.md`

### 3.3 Phase 2: 渲染系统（优先级: 高）

**目标**：引入 ratatui，建立渲染管线

| 任务 | 文件 | 说明 |
|------|------|------|
| 2.1 Renderable trait | `tui/render.rs` | 核心 trait + 布局组件 |
| 2.2 渲染管线 | `tui/terminal.rs` | `draw_with_size()` + SynchronizedUpdate |
| 2.3 Resize 处理 | `tui/viewport.rs` | `update_inline_viewport()` |
| 2.4 状态指示器 | `tui/status.rs` | 状态行 + spinner 集成 |
| 2.5 差异渲染 | `tui/diff.rs` | 文件差异高亮 |
| 2.6 历史 cell 渲染 | `tui/history_cell.rs` | 对话历史渲染 |

**详细方案**：`docs/tui/development/phase-2.md`

### 3.4 Phase 3: 交互系统（优先级: 高）

**目标**：实现完整的交互体验

| 任务 | 文件 | 说明 |
|------|------|------|
| 3.1 PaneView trait | `tui/pane.rs` | trait + PaneStack |
| 3.2 ChatComposer | `tui/composer.rs` | 输入框（TextArea + 历史 + slash 命令） |
| 3.3 审批弹窗 | `tui/approval.rs` | 审批视图（Y/N/D/A） |
| 3.4 选择列表 | `tui/selection.rs` | 通用选择列表 |
| 3.5 状态机 | `tui/state.rs` | 应用状态机 + 输入状态机 |

**详细方案**：`docs/tui/development/phase-3.md`

### 3.5 Phase 4: 集成与优化（优先级: 中）

**目标**：与现有 Agent 系统集成，添加高级功能

| 任务 | 文件 | 说明 |
|------|------|------|
| 4.1 Agent 集成 | `tui/agent.rs` | 对接现有 `run_agent()` |
| 4.2 流式输出 | `tui/streaming.rs` | 流式 markdown 渲染到 TUI |
| 4.3 Job control | `tui/job_control.rs` | `^Z` 暂停/恢复 |
| 4.4 桌面通知 | `tui/notification.rs` | 失焦时通知 |
| 4.5 主入口 | `main.rs` | `--interactive` 参数处理 |

**详细方案**：`docs/tui/development/phase-4.md`

---

## 4. 关键接口定义

### 4.1 TuiEvent

```rust
// tui/event.rs
pub enum TuiEvent {
    Key(KeyEvent),       // 按键
    Paste(String),       // 粘贴内容
    Resize(Size),        // 终端尺寸变化
    Draw,                // 定时重绘
    Resume,              // 从 ^Z 恢复
}
```

### 4.2 Renderable Trait

```rust
// tui/render.rs
pub trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16;
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)>;
    fn cursor_style(&self, area: Rect) -> SetCursorStyle;
}
```

### 4.3 PaneView Trait

```rust
// tui/pane.rs
pub trait PaneView: Renderable {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    fn is_complete(&self) -> bool { false }
    fn on_ctrl_c(&mut self) -> CtrlCAction { CtrlCAction::NotHandled }
    fn view_id(&self) -> Option<&'static str> { None }
}
```

### 4.4 App 主循环

```rust
// tui/app.rs
loop {
    tokio::select! {
        Some(event) = tui.event_stream().next() => {
            self.handle_event(event);
        }
        Some(event) = agent_rx.recv() => {
            self.handle_agent_event(event);
        }
    }
    self.render();
}
```

---

## 5. 文件清单

### 5.1 新增文件

```
apps/cli/src/tui/                  # TUI 模块（新增目录）
├── mod.rs                         # 公共导出 + 条件编译
├── app.rs                         # App 主循环
├── terminal.rs                    # 终端初始化 + 内联视图
├── event.rs                       # 事件系统
├── viewport.rs                    # Viewport 管理
├── history.rs                     # 历史行插入
├── render.rs                      # Renderable trait + 布局
├── pane.rs                        # PaneView trait + PaneStack
├── composer.rs                    # 输入框
├── approval.rs                    # 审批弹窗
├── selection.rs                   # 选择列表
├── status.rs                      # 状态指示器
├── state.rs                       # 状态机
├── diff.rs                        # 差异渲染
├── streaming.rs                   # 流式输出
├── job_control.rs                 # ^Z 暂停/恢复
├── notification.rs                # 桌面通知
└── agent.rs                       # Agent 集成适配
```

### 5.2 修改文件

| 文件 | 修改内容 |
|------|----------|
| `apps/cli/Cargo.toml` | 添加 `ratatui`、`crossterm` 依赖（可选 feature） |
| `apps/cli/src/main.rs` | 添加 `--interactive` / `-i` 参数处理 |
| `apps/cli/src/lib.rs` | 添加 `#[cfg(feature = "tui")] mod tui;` |

---

## 6. 依赖管理

### 6.1 Cargo.toml 变更

```toml
[features]
default = []
tui = ["ratatui", "crossterm"]

[dependencies]
ratatui = { version = "0.28", optional = true, features = ["crossterm"] }
crossterm = { version = "0.28", optional = true, features = ["event-stream"] }
```

### 6.2 依赖说明

| 依赖 | 版本 | 用途 | 是否可选 |
|------|------|------|----------|
| `ratatui` | 0.28 | 终端 UI 框架 | ✅ 是（feature `tui`） |
| `crossterm` | 0.28 | 终端控制 + 事件 | ✅ 是（feature `tui`） |
| `tokio-stream` | 0.1 | 事件流处理 | 已存在 |
| `tokio` | 1.0 | 异步运行时 | 已存在 |

---

## 7. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| ratatui 版本更新 API 变化 | 维护成本 | 锁定版本 0.28，使用最小功能集 |
| 内联视图在特殊终端异常 | 兼容性问题 | Phase 1 只支持标准终端，Zellij 延后 |
| 与现有 display/ 模块冲突 | 代码重复 | 明确职责边界，共享工具函数 |
| 大对话历史渲染性能 | 卡顿 | 只渲染可见区域，虚拟化 |
| 多平台兼容（Windows） | 功能缺失 | 条件编译 + 降级方案 |

---

## 8. 交付标准

### 8.1 Phase 1 交付物

- [ ] 终端初始化成功，raw mode 可用
- [ ] 事件流可接收按键和 resize
- [ ] 内联视图可在终端中显示固定区域
- [ ] 历史行可插入到终端滚动区域
- [ ] App 主循环可运行，`tokio::select!` 正常工作
- [ ] 退出时终端状态完全恢复

### 8.2 Phase 2 交付物

- [ ] ratatui 集成成功，渲染管线正常工作
- [ ] Renderable trait 可用，布局组件可组合
- [ ] 聊天区域可渲染文本和 markdown
- [ ] Resize 时 viewport 正确调整
- [ ] Status 行显示 AI 状态 + spinner 动画

### 8.3 Phase 3 交付物

- [ ] 用户可输入文本并提交
- [ ] AI 可请求审批，用户可交互式响应
- [ ] 底部面板栈正常工作
- [ ] 状态机状态转换正确
- [ ] 中断流程（Ctrl+C）正常工作

### 8.4 Phase 4 交付物

- [ ] 完整对话体验：输入 → 提交 → AI 处理 → 结果显示
- [ ] `^Z` 暂停/恢复
- [ ] 桌面通知
- [ ] 传统模式完全不受影响