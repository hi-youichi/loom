# Loom TUI 产品架构

## 概述

本文档描述 Loom TUI 的产品架构（Product Architecture）——从产品视角出发，定义 TUI 系统的整体结构、组件划分、交互方式、以及各组件之间的协作关系。聚焦于"系统由哪些部分组成、各部分如何协同工作"。

**参考来源**：
- Codex CLI TUI 工程架构分析（`docs/tui/design/engineering.md`）
- Loom TUI 交互设计（`docs/tui/interaction/reference-codex.md`）
- Loom TUI 交互架构（`docs/tui/interaction/architecture.md`）
- Codex CLI TUI 产品定位（`docs/tui/product/product.md`）
- TUI 实现方案（`docs/tui/design/impl-plan.md`）

---

## 1. 产品架构总览

### 1.1 系统层级

Loom TUI 系统由五个层级组成，自下而上：

```
┌─────────────────────────────────────────────────────────────────┐
│  5. 应用层 (App Layer)                                           │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  App 主循环 · 会话管理 · 配置管理 · 插件集成                  │ │
│  └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  4. 交互层 (Interaction Layer)                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  输入框 · 审批弹窗 · 选择列表 · 状态指示 · 通知              │ │
│  └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  3. 渲染层 (Rendering Layer)                                     │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Renderable trait · 布局组件 · 历史渲染 · 差异渲染 · Spinner │ │
│  └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  2. 终端层 (Terminal Layer)                                      │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  内联视图 · 事件系统 · 历史行插入 · ^Z 暂停 · 光标管理       │ │
│  └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  1. 基础设施层 (Infrastructure Layer)                             │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  ratatui · crossterm · tokio · 终端检测 · ANSI 工具         │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 核心组件图

```
┌─────────────────────────────────────────────────────────────────┐
│                        Loom TUI 系统                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌─────────────────┐     ┌────────────────────────────────────┐ │
│  │   用户输入       │     │      Agent 后端                     │ │
│  │   (键盘/粘贴)    │────▶│   (run_agent / callback)           │ │
│  └────────┬────────┘     └────────────┬───────────────────────┘ │
│           │                           │                          │
│           ▼                           ▼                          │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                     App 主循环                                │ │
│  │          tokio::select! 事件分发                             │ │
│  └──────────┬──────────────────────────────────┬───────────────┘ │
│             │                                  │                  │
│             ▼                                  ▼                  │
│  ┌─────────────────────┐          ┌───────────────────────────┐  │
│  │   交互子系统         │          │     渲染子系统              │  │
│  │  ┌─────────────────┐│          │  ┌───────────────────────┐│  │
│  │  │ PaneStack 面板栈 ││          │  │ 历史行插入 → 终端区域  ││  │
│  │  │ ├ ChatComposer  ││          │  │ 内联视图 → ratatui    ││  │
│  │  │ ├ ApprovalOverlay││          │  │ Spinner → stderr      ││  │
│  │  │ └ SelectionView  ││          │  └───────────────────────┘│  │
│  │  └─────────────────┘│          └───────────────────────────┘  │
│  └─────────────────────┘                                         │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    终端管理层                                  │ │
│  │  raw mode · bracketed paste · 焦点事件 · ^Z 暂停/恢复       │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 1.3 运行模式

| 模式 | 触发条件 | 渲染方式 | 交互方式 | 适用场景 |
|------|----------|----------|----------|----------|
| **TUI 模式** | `--interactive` / `-i` | 内联视图 (ratatui) | 交互式输入框 | 对话式编程 |
| **传统模式** | 默认（无 flag） | ANSI 文本 (stderr) | 无输入，输出后退出 | 脚本/管道/CI |
| **审批模式** | `--approve` | ANSI 文本 + 弹窗 | 混合式 | 单次运行+审批 |

---

## 2. 终端层架构

### 2.1 职责

终端层负责终端硬件抽象、事件捕获、以及内联视图的管理。是所有上层组件的基础。

### 2.2 组件

| 组件 | 职责 | 关键接口 |
|------|------|----------|
| **终端初始化** | raw mode、bracketed paste、panic hook | `init()`, `restore()` |
| **内联视图** | 管理 viewport 位置、渲染区域 | `draw(height, f)`, `draw_with_resize_reflow()` |
| **事件系统** | 事件捕获、分发、pause/resume | `TuiEvent`, `EventBroker`, `TuiEventStream` |
| **历史行插入** | 将聊天历史写入终端滚动区域 | `insert_history_lines()`, `flush_pending_history_lines()` |
| **Job Control** | `^Z` 暂停/恢复 | `SuspendContext` |
| **Alt Screen** | 全屏模式切换（文件编辑器等） | `enter_alt_screen()`, `leave_alt_screen()` |
| **光标管理** | 光标位置、样式控制 | `cursor_pos()`, `cursor_style()` |
| **帧调度** | 控制绘制频率，避免过度绘制 | `FrameRequester` |

### 2.3 内联视图原理

内联视图是 TUI 最核心的架构决策。它不在终端的 alternate screen 中渲染，而是在终端正常滚动区域中"划出一块"作为渲染区域。

```
┌──────────────────────────────────────────────┐
│ 终端历史（正常滚动区域）                        │
│ $ cd ~/project                                │
│ $ ls -la                                      │
│                                               │
├──────────────────────────────────────────────┤  ← viewport 起始
│ 聊天区域（ratatui 渲染）                       │
│ ┌─ User ──────────────────────────────────┐  │
│ │ 帮我优化这个函数                          │  │
│ └──────────────────────────────────────────┘  │
│ ┌─ Assistant ──────────────────────────────┐  │
│ │ 好的，我来帮你修改...                      │  │
│ └──────────────────────────────────────────┘  │
│ > _                                           │
├──────────────────────────────────────────────┤  ← viewport 结束
│ $ _                                           │
└──────────────────────────────────────────────┘
```

**关键实现机制**：
- 每次 `draw()` 时，先 flush 待处理的历史行到 viewport 上方
- 然后在 viewport 区域内用 `ratatui::Terminal::draw_with_size()` 渲染
- 所有输出通过 `crossterm::SynchronizedUpdate` 包裹，确保无闪烁
- 终端 resize 时重新计算 viewport 位置

### 2.4 事件流

```
crossterm EventStream
  → TuiEventStream (包装: 焦点事件、粘贴解析、resize 检测)
  → TuiEvent 枚举 (Key/Paste/Resize/Draw/Resume)
  → EventBroker (Arc 共享, 支持 pause/resume, 多消费者)
  → App 主循环的 tokio::select! 分支
```

---

## 3. 渲染层架构

### 3.1 职责

渲染层提供统一的 UI 组件渲染接口，管理布局、高亮、差异显示等视觉呈现。

### 3.2 Renderable 抽象

所有 UI 组件实现 `Renderable` trait，形成统一的渲染接口：

```rust
pub trait Renderable {
    /// 渲染到 ratatui buffer
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// 告知布局系统需要多少高度
    fn desired_height(&self, width: u16) -> u16;
    /// 光标位置（用于输入框）
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)>;
    /// 光标样式
    fn cursor_style(&self, _area: Rect) -> SetCursorStyle;
}
```

### 3.3 布局组件

| 组件 | 功能 | 说明 |
|------|------|------|
| `ColumnRenderable` | 垂直堆叠 | 多个 Renderable 从上到下排列 |
| `RowRenderable` | 水平排列 | 多个 Renderable 从左到右排列 |
| `FlexRenderable` | 弹性布局 | 按比例分配空间 |
| `InsetRenderable` | 内边距包装 | 为 Renderable 添加边距 |

### 3.4 渲染组件

| 组件 | 功能 | 复用来源 |
|------|------|----------|
| **历史渲染** | 渲染已完成的对话 cell | 新建 (`history_cell/`) |
| **流式渲染** | 渲染正在流式输出的内容 | 复用 `display/streaming_markdown.rs` |
| **差异渲染** | 文件差异高亮（+/-） | 新建 (`diff.rs`) |
| **Spinner** | 动画进度指示 | 复用 `display/spinner.rs` 的帧动画 |
| **状态指示** | AI 状态行（思考中/执行中） | 新建 (`status.rs`) |

### 3.5 渲染管线

```
App::render()
  → 计算所需高度（desired_height 递归求和）
  → Tui::draw(height, |frame| {
      // 聊天区域渲染
      for cell in history {
        cell.render(area, buf);
      }
      // 活跃 cell 渲染
      if let Some(active) = active_cell {
        active.render(area, buf);
      }
      // 底部面板渲染
      pane_stack.render(area, buf);
      // 光标设置
      frame.set_cursor_position(pane_stack.cursor_pos(area));
    })
```

---

## 4. 交互层架构

### 4.1 职责

交互层负责用户输入处理、弹窗管理、审批流程、以及状态反馈。

### 4.2 栈式面板管理

交互层最核心的架构是**栈式面板（PaneStack）**——一个 `Vec<Box<dyn PaneView>>`，通过 push/pop 管理不同视图：

```
PaneStack
  └── Vec<Box<dyn PaneView>>
      ├── [基座] ChatComposer — 主输入框
      ├── [push] ApprovalOverlay — 审批弹窗
      ├── [push] ListSelectionView — 选择列表
      ├── [push] FeedbackView — 反馈提交
      └── [push] ... 更多视图
```

**视图优先级**：
- 栈顶视图优先处理按键事件
- 如果栈顶视图不处理，传递给下一层
- 当视图完成（`is_complete()`）后自动 pop

### 4.3 PaneView 接口

```rust
pub trait PaneView: Renderable {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    fn is_complete(&self) -> bool;
    fn on_ctrl_c(&mut self) -> CtrlCAction;
    fn view_id(&self) -> Option<&'static str>;
}
```

### 4.4 视图类型

| 视图 | 用途 | 键盘映射 | 生命周期 |
|------|------|----------|----------|
| **ChatComposer** | 主输入框 | Enter 提交, Shift+Enter 换行, ↑/↓ 历史, / 命令 | 始终存在（基座） |
| **ApprovalOverlay** | 审批弹窗 | Y/N/D/A, Esc 取消 | push → 用户响应 → pop |
| **ListSelectionView** | 选择列表 | ↑/↓ 选择, Enter 确认, / 搜索, Esc 取消 | push → 选择 → pop |
| **FeedbackView** | 反馈提交 | 文本输入, Enter 提交, Esc 取消 | push → 提交 → pop |
| **CustomPromptView** | 自定义提示词 | 文本输入, Enter 提交, Esc 取消 | push → 提交 → pop |

### 4.5 状态指示系统

| 状态 | 触发条件 | 视觉表现 | 优先级 |
|------|----------|----------|--------|
| Idle | 等待用户输入 | 无 spinner，显示输入框 | 低 |
| Submitting | 用户提交输入 | 短暂的提交动画 | 低 |
| Thinking | AI 正在思考 | Spinner + "思考中..." | 中 |
| Executing | AI 正在执行工具 | Spinner + "执行中..." | 中 |
| WaitingApproval | 等待用户审批 | 底部弹出审批视图 | 高 |
| Interrupted | 用户中断 | 显示中断提示 | 高 |
| Error | 发生错误 | 红色错误信息 | 高 |

### 4.6 通知系统

| 通知类型 | 触发条件 | 发送方式 |
|----------|----------|----------|
| 回复完成 | AI 完成回复 | 桌面通知（终端失焦时） |
| 需要审批 | AI 请求文件修改/命令执行 | 桌面通知 + 审批弹窗 |
| 错误 | 发生错误 | 桌面通知（可选） |

---

## 5. 应用层架构

### 5.1 职责

应用层负责协调所有子系统，管理会话生命周期、配置、以及 Agent 集成。

### 5.2 App 主循环

```rust
loop {
    tokio::select! {
        // 终端事件（按键、粘贴、resize）
        Some(event) = tui.event_stream().next() => {
            handle_event(event);
        }
        // Agent 后端事件（流式输出、工具调用、状态更新）
        Some(event) = agent_rx.recv() => {
            handle_agent_event(event);
        }
    }
    render();
}
```

### 5.3 会话管理

| 组件 | 职责 |
|------|------|
| `SessionState` | 会话状态机 (`Idle → Submitting → Waiting → Streaming → Idle`) |
| `SessionResume` | 会话恢复（`--resume` 和 `--fork`） |
| `SessionArchive` | 会话归档和清理 |
| `TranscriptReflow` | 终端 resize 时的对话历史重排 |

### 5.4 配置管理

| 配置项 | 默认值 | 用户可修改 |
|--------|--------|-----------|
| 模型 | 配置文件中的默认模型 | 运行时通过选择列表切换 |
| Spinner 风格 | dots | 通过配置修改 |
| 通知条件 | Unfocused | 通过配置修改 |
| 审批策略 | 每次询问 | 会话中可设置为"始终允许" |
| 主题 | 默认 | 通过选择列表切换 |

### 5.5 Agent 集成

```
Agent 后端 (run/agent.rs)
  │
  ├── 回调模式 (传统模式)
  │   → StreamCallback → display/event_handler.rs → ANSI 输出
  │
  └── Channel 模式 (TUI 模式)
      → tokio::sync::mpsc → App 主循环 → 更新 UI 状态
```

---

## 6. 数据架构

### 6.1 数据流图

```
用户按键
  → App::handle_key()
  → PaneStack::handle_key_event()
  → ChatComposer 更新状态
  → 用户提交
  → App::submit()
  → Agent 后端 (run_agent)
  → StreamEvent → mpsc channel
  → App::handle_agent_event()
  → history_cell 更新 / active_cell 更新
  → App::render()
  → Tui::draw()
  → 终端输出
```

### 6.2 核心数据结构

| 类型 | 说明 | 生命周期 |
|------|------|----------|
| `HistoryCell` | 已完成的对话单元（用户消息/AI 回复） | 追加到 `Vec<HistoryCell>`，不可变 |
| `ActiveCell` | 正在流式输出的 AI 回复 | 创建 → 流式追加 → 完成 → 转为 HistoryCell |
| `PendingHistoryLines` | 待写入终端滚动区域的历史行 | 创建 → 追加 → draw() 时 flush → 清空 |
| `PaneStack` | 底部面板视图栈 | 始终存在，通过 push/pop 管理 |
| `EventState` | 事件处理状态（token 计数、当前节点等） | 单个 Agent 运行周期内有效 |

### 6.3 状态一致性

- **渲染状态**：`App` 持有完整状态，每次 render() 从状态生成 UI
- **事件顺序**：`mpsc` channel 保证 Agent 事件按序处理
- **并发安全**：`Arc<AtomicBool>` 用于 `terminal_focused` 等跨线程状态

---

## 7. 集成架构

### 7.1 与现有系统集成

```
现有 Loom CLI 系统
  │
  ├── display/ 模块 ───→ 复用：spinner 帧、terminal 检测、panel_format
  │
  ├── run/agent.rs ────→ 适配：回调 → mpsc channel
  │
  ├── config/ ─────────→ 直接使用：配置加载
  │
  ├── agent-core ──────→ 直接使用：agent 运行
  │
  └── stream-event ────→ 直接使用：事件类型
```

### 7.2 条件编译

```
Cargo.toml
  [features]
  default = []
  tui = ["ratatui", "crossterm"]

  [dependencies]
  ratatui = { version = "0.28", optional = true, features = ["crossterm"] }
  crossterm = { version = "0.28", optional = true, features = ["event-stream"] }
```

```
main.rs
  #[cfg(feature = "tui")]
  mod tui;

  fn main() {
    if args.interactive && cfg!(feature = "tui") {
      run_tui_mode().await;
    } else {
      run_traditional_mode().await;
    }
  }
```

### 7.3 事件适配

现有 `run/agent.rs` 回调模式 → TUI Channel 模式：

```rust
// 现有回调
type StreamCallback = Arc<Mutex<dyn FnMut(Value) + Send>>;

// TUI 适配
let (tx, rx) = tokio::sync::mpsc::channel(256);
let callback = move |event: Value| {
    tx.blocking_send(event).ok();
};

// App 主循环接收
tokio::select! {
    Some(event) = rx.recv() => {
        self.handle_agent_event(event);
    }
}
```

---

## 8. 质量属性架构

### 8.1 性能

| 场景 | 目标 | 实现策略 |
|------|------|----------|
| 启动时间 | < 500ms | 条件编译，TUI 依赖不增加传统模式启动时间 |
| 渲染帧率 | ≥ 30fps | FrameRequester 控制绘制频率，避免过度绘制 |
| 大对话历史 | 流畅滚动 | 虚拟化渲染，只渲染可见区域 |
| 流式输出 | 无延迟卡顿 | 异步 channel + 增量渲染 |

### 8.2 可靠性

| 场景 | 策略 |
|------|------|
| 终端尺寸变化 | 自动检测 resize，重新计算 viewport |
| ^Z 暂停/恢复 | SuspendContext 管理完整生命周期 |
| panic 恢复 | set_panic_hook 确保终端恢复 raw mode |
| 非 TTY 环境 | 自动降级到传统 ANSI 模式 |

### 8.3 安全性

| 场景 | 策略 |
|------|------|
| 文件修改 | 审批弹窗，用户确认后执行 |
| 命令执行 | 审批弹窗，可配置"始终允许" |
| 敏感信息 | 不显示在终端历史中（flush 前过滤） |

### 8.4 兼容性

| 终端 | 支持级别 | 说明 |
|------|----------|------|
| macOS Terminal | ✅ 完全支持 | 标准终端 |
| iTerm2 | ✅ 完全支持 | 标准终端 |
| Alacritty | ✅ 完全支持 | 标准终端 |
| tmux | ✅ 支持 | 内联视图正常 |
| Zellij | ⚠️ 实验性 | 需特殊处理历史行插入 |
| Windows Terminal | ✅ 支持 | 通过 VT 处理 |
| 非 TTY | ✅ 降级 | 自动切换到传统模式 |

---

## 9. 模块依赖关系

### 9.1 模块依赖图

```
main.rs
  └── lib.rs
        ├── tui/ (feature-gated)
        │     ├── mod.rs
        │     ├── app.rs
        │     │     ├── render.rs
        │     │     ├── pane.rs
        │     │     ├── composer.rs
        │     │     ├── status.rs
        │     │     └── notification.rs
        │     ├── terminal.rs
        │     │     ├── event.rs
        │     │     └── history.rs
        │     └── spinner.rs
        ├── run/ (现有，不变)
        │     └── agent.rs
        └── display/ (现有，复用)
              ├── terminal.rs
              ├── spinner.rs
              ├── panel_format.rs
              ├── streaming_markdown.rs
              └── markdown.rs
```

### 9.2 模块间通信

| 源模块 | 目标模块 | 通信方式 | 数据 |
|--------|----------|----------|------|
| `app.rs` | `terminal.rs` | 方法调用 | `draw(height, f)`, `insert_history_lines()` |
| `app.rs` | `event.rs` | Stream | `TuiEvent` stream |
| `app.rs` | `pane.rs` | 方法调用 | `handle_key_event()`, `render()` |
| `app.rs` | `run/agent.rs` | mpsc channel | `StreamEvent` |
| `app.rs` | `spinner.rs` | 方法调用 | `update()`, `finish()` |
| `app.rs` | `notification.rs` | 方法调用 | `notify(message)` |

---

## 10. 演进路径

### 10.1 阶段规划

| 阶段 | 交付物 | 依赖 |
|------|--------|------|
| **Phase 1: 基础设施** | 终端初始化、事件系统、内联视图 | crossterm |
| **Phase 2: 渲染系统** | Renderable trait、布局组件、Spinner 集成 | ratatui |
| **Phase 3: 交互系统** | 输入框、审批弹窗、面板栈 | Phase 1+2 |
| **Phase 4: 集成** | Agent 对接、流式输出、多代理、^Z 暂停 | Phase 3 |

### 10.2 里程碑

| 里程碑 | 时间 | 功能 |
|--------|------|------|
| **M1** | Phase 1+2 | 显示聊天历史、Spinner 动画、AI 回复渲染 |
| **M2** | Phase 3 | 用户可输入文本、提交、审批、弹窗交互 |
| **M3** | Phase 4 | 完整对话体验、^Z 暂停/恢复、桌面通知 |

---

## 11. 架构决策记录

### ADR-1: 内联视图而非全屏

**决策**：采用内联视图（Inline Viewport），不使用 alternate screen。
**理由**：Loom 是 CLI 工具，用户需要在终端中同时使用其他命令。内联视图保留终端历史，退出后终端状态完全恢复。
**后果**：viewport 管理复杂，resize 有边缘情况。但用户体验更好。

### ADR-2: ratatui 而非纯 ANSI

**决策**：使用 ratatui 作为终端 UI 框架。
**理由**：提供成熟的 buffer、布局、widget 系统，避免重复实现布局算法。Codex 已验证其可靠性（237K 行代码）。
**后果**：增加约 50KB 二进制体积，但开发效率显著提升。

### ADR-3: 双模式运行

**决策**：TUI 模式 + 传统 ANSI 模式并行。
**理由**：向后兼容现有脚本和管道，用户可渐进采用。非 TTY 环境自动降级。
**后果**：维护两套输出路径，但共享 display/ 模块的公共逻辑。

### ADR-4: 条件编译

**决策**：TUI 通过 Cargo feature flag 控制编译。
**理由**：不增加传统模式的依赖和编译时间。用户可自主选择是否启用。
**后果**：需要条件编译的代码组织，`#[cfg(feature = "tui")]` 散布在代码中。

### ADR-5: 模块化拆分

**决策**：每个文件不超过 500 行，复杂模块拆分为子目录。
**理由**：避免 Codex 的 `chatwidget.rs` 单文件过大问题（83KB）。
**后果**：模块间通信需要清晰定义接口，增加少量抽象层代码。

---

## 12. 总结

Loom TUI 的产品架构以**五层结构**（基础设施 → 终端 → 渲染 → 交互 → 应用）组织，核心设计决策包括：

1. **内联视图**：不占全屏，融入终端工作流
2. **栈式面板**：统一管理弹窗和交互
3. **双模式运行**：TUI 和传统模式并行
4. **条件编译**：TUI 作为可选特性
5. **模块化拆分**：避免单文件过大

架构的核心理念是**渐进增强**——用户在终端中开始使用 Loom，随着交互深度增加，TUI 逐步提供更丰富的视觉和交互支持，但始终不脱离终端环境。