# Loom TUI 项目立项文档

**版本**: 1.0  
**日期**: 2025-01-14  
**作者**: Loom Team  
**状态**: Draft

---

## 目录

- [1. 项目概述](#1-项目概述)
  - [1.1 项目名称](#11-项目名称)
  - [1.2 项目目标](#12-项目目标)
  - [1.3 范围定义](#13-范围定义)
  - [1.4 用户场景](#14-用户场景)
- [2. 技术设计](#2-技术设计)
  - [2.1 系统集成方案](#21-系统集成方案)
  - [2.2 事件流架构](#22-事件流架构)
  - [2.3 组件清单](#23-组件清单)
  - [2.4 数据流图](#24-数据流图)
- [3. 功能规格](#3-功能规格)
  - [3.1 启动方式](#31-启动方式)
  - [3.2 实时更新机制](#32-实时更新机制)
- [4. 实现计划](#4-实现计划)
- [5. 技术选型](#5-技术选型)
- [6. 风险和挑战](#6-风险和挑战)
- [7. 成功指标](#7-成功指标)

---

## 1. 项目概述

### 1.1 项目名称

**loom-tui** - Loom Agent 监控终端界面

### 1.2 项目目标

创建一个独立的终端用户界面（TUI），让用户能够：

1. **实时监控**：查看所有运行中的 agent 及其状态
2. **可视化追踪**：以直观的方式展示 agent 的执行流程和进度
3. **交互式操作**：选择特定 agent 查看详细信息、日志输出
4. **独立运行**：作为独立的 TUI 应用启动，不依赖其他界面

### 1.3 范围定义

#### 包含内容

- ✅ 独立的 TUI 应用启动命令（`loom tui`）
- ✅ Agent 列表实时显示和状态更新
- ✅ Agent 详情面板（任务描述、执行节点、结果/错误）
- ✅ 实时滚动日志输出
- ✅ 键盘导航和交互
- ✅ 多 agent 并发监控
- ✅ 与现有 loom agent 系统的无缝集成

#### 不包含内容

- ❌ Agent 创建或配置（通过 CLI 参数或其他方式完成）
- ❌ 图形化界面（GUI）
- ❌ Web 界面
- ❌ Agent 执行控制（暂停、恢复、取消）
- ❌ 历史数据持久化和查询
- ❌ 远程连接功能

### 1.4 用户场景

#### 场景 1：并发 Agent 监控

**用户**: 开发者调试多 agent 系统  
**目标**: 同时监控 5 个不同类型的 agent（ReAct、DUP、ToT）的执行状态  
**流程**:
1. 启动 `loom tui`
2. 在另一个终端启动多个 agent 任务
3. 在 TUI 中实时查看所有 agent 的状态变化
4. 观察某个 agent 执行时间过长，选择查看详情
5. 查看该 agent 的当前执行节点和进度消息
6. 根据日志输出判断问题所在

**价值**: 快速定位并发 agent 执行中的问题，提高调试效率

#### 场景 2：长时间任务监控

**用户**: 研究人员运行复杂的 agent 实验  
**目标**: 监控运行数小时的 agent 任务，偶尔查看进度  
**流程**:
1. 启动 agent 任务并开启 TUI
2. TUI 显示任务开始，状态为 Running
3. 定期切回 TUI 查看进度消息
4. 看到当前节点变化，确认任务在正常执行
5. 任务完成后，状态变为 Completed
6. 查看最终结果摘要

**价值**: 无需持续关注，可随时了解长时间任务的状态

#### 场景 3：错误诊断

**用户**: 测试人员验证 agent 行为  
**目标**: 快速发现并诊断 agent 执行错误  
**流程**:
1. 启动 TUI 并运行测试 agent
2. 观察到某个 agent 状态变为 Error（红色高亮）
3. 选择该 agent 查看详情
4. 查看错误信息和相关日志
5. 根据错误信息修复问题

**价值**: 快速发现和定位错误，缩短调试周期

#### 场景 4：演示和展示

**用户**: 技术演示人员  
**目标**: 向团队展示 loom agent 系统的工作方式  
**流程**:
1. 在大屏幕上启动 TUI
2. 运行几个典型 agent 任务
3. 实时展示 agent 的执行流程和状态变化
4. 切换不同 agent 展示详细信息

**价值**: 直观展示系统工作原理，提升演示效果

---

## 2. 技术设计

### 2.1 系统集成方案

#### 架构概览

```
┌─────────────────────────────────────────────────────┐
│                   Loom Agent System                 │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │ Agent 1  │  │ Agent 2  │  │ Agent N  │        │
│  │ (ReAct)  │  │  (ToT)   │  │  (GoT)   │        │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘        │
│       │             │             │               │
│       └──────────────┴─────────────┘               │
│                      │                             │
│              ┌───────▼────────┐                   │
│              │ Event Channel  │                   │
│              │  (mpsc)        │                   │
│              └───────┬────────┘                   │
└──────────────────────┼────────────────────────────┘
                       │
                       │ TuiEvent
                       │
              ┌────────▼────────┐
              │   TUI System    │
              │                 │
              │  ┌───────────┐  │
              │  │    App    │  │
              │  │  (State)  │  │
              │  └─────┬─────┘  │
              │        │        │
              │  ┌─────▼─────┐  │
              │  │  UI Render│  │
              │  └───────────┘  │
              └─────────────────┘
```

#### 集成点

1. **Agent 执行层** (`cli/src/run/agent.rs`)
   - 在 agent 生命周期的关键点发送事件
   - 开始、进度更新、完成、错误

2. **事件通道** (`cli/src/tui/event.rs`)
   - 使用 `tokio::sync::mpsc::unbounded_channel`
   - 支持多生产者（多个 agent）单消费者（TUI）

3. **状态管理** (`cli/src/tui/app.rs`)
   - `App` 结构维护所有 agent 状态
   - `handle_event()` 处理事件并更新状态

4. **UI 渲染** (`cli/src/tui/ui.rs`)
   - 基于 ratatui 的即时模式渲染
   - 响应式布局，适配终端大小

### 2.2 事件流架构

#### 事件类型

```rust
pub enum TuiEvent {
    /// Agent 生命周期事件
    AgentStarted { id: String, name: String, task: String },
    AgentProgress { id: String, node: String, message: String },
    AgentCompleted { id: String, result: String },
    AgentError { id: String, error: String },
    
    /// 系统事件
    Tick,  // 定时刷新
    Quit,  // 用户退出
}
```

#### 事件流时序图

```
Agent                    EventChannel              TUI App              UI Render
  │                           │                       │                     │
  │  AgentStarted             │                       │                     │
  ├──────────────────────────>│                       │                     │
  │                           │  TuiEvent::Started    │                     │
  │                           ├──────────────────────>│                     │
  │                           │                       │  handle_event()     │
  │                           │                       ├──────┐              │
  │                           │                       │      │ update state │
  │                           │                       │<─────┘              │
  │                           │                       │                     │
  │                           │  Tick (every 250ms)   │                     │
  │                           ├──────────────────────>│                     │
  │                           │                       │  render()           │
  │                           │                       ├────────────────────>│
  │                           │                       │                     │
  │  AgentProgress            │                       │                     │
  ├──────────────────────────>│                       │                     │
  │                           │  TuiEvent::Progress   │                     │
  │                           ├──────────────────────>│                     │
  │                           │                       │  handle_event()     │
  │                           │                       ├──────┐              │
  │                           │                       │      │ update state │
  │                           │                       │<─────┘              │
  │                           │                       │                     │
  │  AgentCompleted           │                       │                     │
  ├──────────────────────────>│                       │                     │
  │                           │  TuiEvent::Completed  │                     │
  │                           ├──────────────────────>│                     │
  │                           │                       │  handle_event()     │
  │                           │                       ├──────┐              │
  │                           │                       │      │ update state │
  │                           │                       │<─────┘              │
  │                           │                       │                     │
  │                           │  Tick                 │                     │
  │                           ├──────────────────────>│                     │
  │                           │                       │  render()           │
  │                           │                       ├────────────────────>│
```

#### 事件处理优先级

| 优先级 | 事件类型 | 处理方式 |
|--------|----------|----------|
| High | Quit | 立即处理，设置 `should_quit = true` |
| Normal | AgentStarted/Progress/Completed/Error | 更新状态，标记需要重绘 |
| Low | Tick | 触发周期性重绘 |

### 2.3 组件清单

#### 需要新增的组件

| 组件 | 文件路径 | 功能描述 | 优先级 |
|------|----------|----------|--------|
| TUI Runner | `cli/src/tui/runner.rs` | TUI 主循环，事件分发 | High |
| Input Handler | `cli/src/tui/input.rs` | 键盘输入处理 | High |
| Layout Manager | `cli/src/tui/layout.rs` | 布局计算和管理 | High |
| Log Buffer | `cli/src/tui/log.rs` | 日志缓冲和滚动 | Medium |
| Agent Selector | `cli/src/tui/selector.rs` | Agent 选择逻辑 | Medium |
| Detail View | `cli/src/tui/detail.rs` | 详情面板渲染 | Medium |
| Status Bar | `cli/src/tui/status.rs` | 状态栏组件 | Low |
| Help Overlay | `cli/src/tui/help.rs` | 帮助信息覆盖层 | Low |

#### 需要修改的组件

| 组件 | 文件路径 | 修改内容 |
|------|----------|----------|
| App | `cli/src/tui/app.rs` | 添加选择状态、日志缓冲、运行时间等 |
| AgentInfo | `cli/src/tui/app.rs` | 添加时间戳、节点历史等字段 |
| UI Render | `cli/src/tui/ui.rs` | 重构为多面板布局 |
| EventChannel | `cli/src/tui/event.rs` | 添加日志事件类型 |
| CLI Main | `cli/src/main.rs` | 添加 `tui` 子命令 |

### 2.4 数据流图

#### 启动流程

```
User Input: loom tui
      │
      ▼
┌─────────────┐
│ Parse CLI   │
│ Arguments   │
└─────┬───────┘
      │
      ▼
┌─────────────┐
│ Initialize  │
│ Terminal    │
│ (alternate  │
│  screen)    │
└─────┬───────┘
      │
      ▼
┌─────────────┐
│ Create      │
│ EventChannel│
└─────┬───────┘
      │
      ▼
┌─────────────┐
│ Create App  │
│ with State  │
└─────┬───────┘
      │
      ▼
┌─────────────┐
│ Start Tick  │
│ Timer       │
└─────┬───────┘
      │
      ▼
┌─────────────┐
│ Enter Main  │
│ Loop        │
└─────────────┘
```

#### 运行时数据流

```
┌──────────────────────────────────────────────────────────┐
│                      Main Event Loop                      │
│                                                           │
│  ┌─────────────┐                                         │
│  │ Select on   │                                         │
│  │ - Event RX  │                                         │
│  │ - Tick      │                                         │
│  │ - Input     │                                         │
│  └─────┬───────┘                                         │
│        │                                                  │
│        ▼                                                  │
│  ┌─────────────┐      ┌─────────────┐                   │
│  │ Event?      │──Yes─>│ handle_event│                   │
│  └─────┬───────┘      │ update state│                   │
│        │ No           └─────────────┘                   │
│        ▼                                                  │
│  ┌─────────────┐      ┌─────────────┐                   │
│  │ Input?      │──Yes─>│ handle_input│                   │
│  └─────┬───────┘      │ (nav/select)│                   │
│        │ No           └─────────────┘                   │
│        ▼                                                  │
│  ┌─────────────┐                                         │
│  │ Should      │──Yes───> Break Loop                     │
│  │ Quit?       │                                         │
│  └─────┬───────┘                                         │
│        │ No                                               │
│        ▼                                                  │
│  ┌─────────────┐      ┌─────────────┐                   │
│  │ Render UI   │─────>│ Draw to     │                   │
│  │ (if needed) │      │ terminal    │                   │
│  └─────────────┘      └─────────────┘                   │
│        │                                                  │
│        └──────────────> Loop Back                         │
└──────────────────────────────────────────────────────────┘
```

#### 状态更新流程

```
TuiEvent::AgentProgress { id, node, message }
      │
      ▼
App::handle_event()
      │
      ├──> Find AgentInfo by id
      │
      ├──> Update current_node
      │
      ├──> Update progress_message
      │
      ├──> Add to log buffer
      │
      └──> Mark UI for redraw
```

---

## 3. 功能规格

### 3.1 启动方式

#### CLI 命令设计

```bash
# 基本启动
loom tui

# 带配置文件
loom tui --config path/to/config.toml

# 指定刷新率
loom tui --tick-rate 100ms

# 只监控特定 agent
loom tui --filter "agent-name-pattern"

# 显示帮助
loom tui --help
```

#### 启动参数

| 参数 | 简写 | 类型 | 默认值 | 描述 |
|------|------|------|--------|------|
| `--config` | `-c` | Path | - | 配置文件路径 |
| `--tick-rate` | `-t` | Duration | 250ms | UI 刷新间隔 |
| `--filter` | `-f` | String | - | Agent 名称过滤（正则表达式） |
| `--log-level` | `-l` | Enum | info | 日志级别 (trace/debug/info/warn/error) |
| `--no-color` | | Bool | false | 禁用颜色 |
| `--help` | `-h` | | | 显示帮助信息 |
| `--version` | `-V` | | | 显示版本信息 |

#### 启动行为

1. **终端初始化**
   - 进入 alternate screen 模式（退出后恢复原终端内容）
   - 启用 raw mode（禁用行缓冲，捕获所有按键）
   - 隐藏光标
   - 设置颜色支持检测

2. **资源初始化**
   - 创建 event channel
   - 初始化 App 状态
   - 启动 tick timer（tokio interval）
   - 启动 input listener（crossterm event poll）

3. **退出清理**
   - 恢复 terminal 原始状态
   - 显示 alternate screen 内容（如果有）
   - 显示退出消息

**注意**: 界面布局和交互设计的详细说明请参考 [../design/visual.md](../design/visual.md) 和 [../design/interaction.md](../design/interaction.md)。

---

### 

```
┌─ Agents (3) ────────────────────┐
│                                 │
│ ● dev-agent         Running     │ ← 选中高亮
│ ● code-review       Running     │
│ ✓ test-runner       Completed   │
│ ✗ failed-agent      Error       │
│                                 │
│                                 │
└─────────────────────────────────┘
```

状态图标：
- ● (绿色) Running
- ✓ (蓝色) Completed
- ✗ (红色) Error

#### 详情面板

```
┌─ Agent Details ────────────────────────────────┐
│                                                 │
│ Name:        dev-agent                         │
│ Type:        ReAct                             │
│ Status:      ● Running                         │
│                                                 │
│ Task:                                          │
│ Implement user authentication feature with     │
│ OAuth2 support and session management          │
│                                                 │
│ Current Node: generate_code                    │
│ Progress:    Writing authentication module...  │
│                                                 │
│ Started:     2025-01-14 14:23:45              │
│ Duration:    00:02:15                          │
│                                                 │
│ ─────────────────────────────────────────────  │
│ Result: (pending)                              │
│                                                 │
│ Error: (none)                                  │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### 日志/输出区

```
┌─ Output Log ───────────────────────────────────────────┐
│                                                         │
│ [14:23:45] ▶ dev-agent started                         │
│            Task: Implement user authentication...       │
│                                                         │
│ [14:24:12] ◐ dev-agent: generate_code                  │
│            Generating code for authentication module... │
│                                                         │
│ [14:25:33] ◐ code-review: analyze                      │
│            Analyzing PR #123 for security issues...     │
│                                                         │
│ [14:26:01] ✓ test-runner completed                     │
│            All tests passed successfully                │
│                                                         │
│ [14:26:15] ◐ dev-agent: run_tests                      │
│            Running unit tests...                        │
│                                                         │
│ ▼ Auto-scroll                                           │
└─────────────────────────────────────────────────────────┘
```

特性：
- 时间戳（可配置显示/隐藏）
- Agent 名称高亮
- 不同日志级别不同颜色
- 自动滚动到最新（可锁定）
- 最多保留 1000 条日志（可配置）

#### 状态栏

```
┌─────────────────────────────────────────────────────────────┐
│  q: Quit  |  ↑↓: Navigate  |  Enter: Details  |  h: Help   │
└─────────────────────────────────────────────────────────────┘
```

或者更详细版本：

```
┌─────────────────────────────────────────────────────────────┐
│  q Quit | ↑↓ Nav | Enter Details | Tab Switch | h Help     │
│  Memory: 45 MB | CPU: 2.3% | Events: 156                     │
└─────────────────────────────────────────────────────────────┘
```

**注意**: 交互设计详细说明请参考 [../design/interaction.md](../design/interaction.md)。

### 3.3 实时更新机制

#### 键盘快捷键

| 按键 | 功能 | 上下文 |
|------|------|--------|
| `q` / `Ctrl+C` | 退出 TUI | 全局 |
| `h` / `?` | 显示帮助覆盖层 | 全局 |
| `↑` / `k` | 向上移动选择 | Agent 网格 |
| `↓` / `j` | 向下移动选择 | Agent 网格 |
| `Enter` | 查看选中 agent 详情 | Agent 网格 |
| `Esc` | 返回网格/关闭弹窗 | 详情/弹窗 |
| `Tab` | 切换焦点面板 | 全局 |
| `Shift+Tab` | 反向切换焦点 | 全局 |
| `l` | 切换日志显示 | 全局 |
| `f` | 过滤 agent | 网格 |
| `r` | 刷新/重置视图 | 全局 |
| `g` | 跳转到第一个 agent | 网格 |
| `G` | 跳转到最后一个 agent | 列表 |
| `/` | 搜索日志 | 日志区 |
| `n` | 下一个搜索结果 | 日志区 |
| `N` | 上一个搜索结果 | 日志区 |
| `Space` | 切换日志自动滚动 | 日志区 |
| `Page Up` | 日志向上翻页 | 日志区 |
| `Page Down` | 日志向下翻页 | 日志区 |

#### 导航模式

1. **列表模式**（默认）
   - 焦点在 agent 列表
   - ↑↓ 选择 agent
   - Enter 查看详情

2. **详情模式**
   - 焦点在详情面板
   - 显示完整 agent 信息
   - Esc 返回列表

3. **日志模式**
   - 焦点在日志区
   - 可以滚动、搜索
   - Esc 返回列表

#### 视觉反馈

- **选中行**: 反色背景或高亮边框
- **状态变化**: 短暂闪烁或颜色变化
- **新日志**: 新条目高亮显示 1 秒
- **错误**: 红色边框或背景
- **完成**: 绿色/蓝色高亮

#### 帮助覆盖层

按 `h` 或 `?` 显示：

```
┌─────────────────────────────────────────┐
│           Keyboard Shortcuts            │
├─────────────────────────────────────────┤
│  Navigation                             │
│    ↑/k  Move up          ↓/j  Move down │
│    g    Go to first      G    Go to last│
│    Tab  Next panel       Esc  Back      │
│                                         │
│  Actions                                │
│    Enter  View details   f    Filter    │
│    l      Toggle log     r    Refresh   │
│                                         │
│  General                                │
│    q      Quit           h    Help      │
│    /      Search log     Space Auto-scr │
│                                         │
│  Press any key to close                 │
└─────────────────────────────────────────┘
```

### 3.4 实时更新机制（已合并到 3.3）

#### Tick 事件频率

| 配置 | 频率 | 适用场景 |
|------|------|----------|
| 默认 | 250ms (4 FPS) | 平衡性能和响应性 |
| 高速 | 100ms (10 FPS) | 快速调试 |
| 省电 | 500ms (2 FPS) | 长时间监控 |
| 静态 | 1000ms (1 FPS) | 演示模式 |

#### 状态刷新策略

```rust
enum RefreshStrategy {
    /// 每次 tick 都重绘
    Always,
    /// 仅在状态变化时重绘
    OnChange,
    /// 混合模式：状态变化立即重绘，否则按 tick 重绘
    Hybrid,
}
```

推荐：**Hybrid** 模式

- Agent 事件到达 → 立即标记 `needs_redraw = true`
- Tick 事件 → 如果 `needs_redraw`，执行重绘并清除标记
- 用户输入 → 立即重绘

#### 性能优化

1. **增量渲染**
   - 仅重绘变化的区域
   - 使用 ratatui 的 `render_widget` 而非全屏清除

2. **状态缓存**
   ```rust
   struct RenderCache {
       last_frame: Vec<Line<'static>>,
       hash: u64,
   }
   ```
   - 计算状态哈希
   - 如果哈希相同，跳过重绘

3. **日志缓冲**
   ```rust
   struct LogBuffer {
       entries: VecDeque<LogEntry>,
       max_size: usize,  // 默认 1000
       scroll_position: usize,
   }
   ```
   - 使用 `VecDeque` 作为环形缓冲
   - 超过容量自动丢弃最旧条目

4. **懒加载详情**
   - 仅在用户选择时渲染详情面板
   - 未选中时不计算详情内容

5. **事件合并**
   - 快速连续的 Progress 事件可合并
   - 例如：100ms 内的多个 progress 只保留最后一个

#### 内存管理

| 资源 | 限制策略 | 默认值 |
|------|----------|--------|
| Agent 数量 | 无限制（建议 <100） | - |
| 日志条目 | 环形缓冲丢弃 | 1000 条 |
| 每个日志长度 | 截断 | 500 字符 |
| 渲染缓存 | 单帧 | - |

---

## 4. 实现计划

### 阶段 1: MVP - 基础框架和启动（1-2 天）

#### 目标
- 实现基本的 TUI 启动和退出
- 集成现有事件系统
- 渲染简单的 agent 列表

#### 任务

- [ ] **TUI Runner 实现** (`cli/src/tui/runner.rs`)
  - 实现 main event loop
  - 集成 crossterm input handling
  - 实现 terminal 初始化和清理
  - 实现 tick timer

- [ ] **CLI 集成** (`cli/src/main.rs`)
  - 添加 `tui` 子命令
  - 解析命令行参数
  - 调用 TUI runner

- [ ] **基础 UI** (`cli/src/tui/ui.rs`)
  - 实现简单的单面板布局
  - 渲染 agent 列表（仅名称和状态）
  - 实现基础颜色和样式

- [ ] **测试和文档**
  - 手动测试启动/退出
  - 更新 README

#### 交付物
- 可运行的 `loom tui` 命令
- 显示 agent 列表的基础界面
- 正确的终端清理

#### 依赖
- 无

#### 工作量
- 开发: 8-12 小时
- 测试: 2 小时

---

### 阶段 2: 监控功能 - Agent 网格和 Session 会话（2-3 天）

#### 目标
- 完整的 agent 状态显示
- 实时更新机制
- 多面板布局

#### 任务

- [ ] **状态管理增强** (`cli/src/tui/app.rs`)
  - 添加时间戳字段（started_at, updated_at）
  - 添加选择状态（selected_index）
  - 添加统计信息（running_count, completed_count）

- [ ] **布局系统** (`cli/src/tui/layout.rs`)
  - 实现多面板布局计算
  - 响应式设计（适配不同终端大小）
  - 边界检查

- [ ] **Agent 网格面板**
  - 状态图标和颜色
  - 高亮选中卡片
  - 网格布局（多列显示）
  - 滚动支持（超过可视区域）

- [ ] **Session 会话面板**
  - 显示当前 Session 的对话历史
  - 用户消息和 Agent 回复
  - 滚动查看历史消息
  - 消息格式化和高亮

- [ ] **详情面板**
  - 显示完整的 AgentInfo
  - 格式化显示（字段对齐、换行）
  - 时间计算和显示

- [ ] **标题栏和状态栏**
  - Logo 和版本显示
  - 运行时间计时器
  - 快捷键提示

- [ ] **实时更新**
  - 实现 tick-based 刷新
  - 状态变化高亮
  - 性能优化（避免不必要的重绘）

#### 交付物
- 完整的多面板界面
- 实时状态更新
- 响应式布局

#### 依赖
- 阶段 1 完成

#### 工作量
- 开发: 16-20 小时
- 测试: 4 小时

---

### 阶段 3: 交互功能 - 选择、详情查看、日志（2-3 天）

#### 目标
- 完整的键盘交互
- 日志查看和滚动
- 用户体验优化

#### 任务

- [ ] **输入处理** (`cli/src/tui/input.rs`)
  - 实现所有快捷键
  - 导航逻辑（↑↓、g/G、Tab）
  - 输入事件映射

- [ ] **Agent 选择器** (`cli/src/tui/selector.rs`)
  - 跟踪选中状态
  - 边界处理（第一/最后一个）
  - 过滤功能

- [ ] **日志系统** (`cli/src/tui/log.rs`)
  - 实现 LogBuffer（环形缓冲）
  - 日志滚动和分页
  - 自动滚动切换
  - 日志搜索（基础）

- [ ] **日志渲染**
  - 时间戳格式化
  - 不同级别不同颜色
  - Agent 名称高亮
  - 长行截断或换行

- [ ] **新增事件类型** (`cli/src/tui/event.rs`)
  ```rust
  TuiEvent::AgentLog { id: String, level: LogLevel, message: String }
  ```

- [ ] **帮助系统**
  - 帮助覆盖层
  - 快捷键列表显示

- [ ] **UX 优化**
  - 视觉反馈（选中高亮、状态变化）
  - 平滑滚动
  - 加载状态指示

#### 交付物
- 完整的键盘交互
- 可滚动的日志查看
- 帮助系统

#### 依赖
- 阶段 2 完成

#### 工作量
- 开发: 16-20 小时
- 测试: 4 小时

---

### 阶段 4: 增强 - 性能优化、错误处理、测试（2-3 天）

#### 目标
- 生产就绪的质量
- 性能优化
- 完整的测试覆盖

#### 任务

- [ ] **性能优化**
  - 实现增量渲染
  - 状态哈希和缓存
  - 事件合并
  - 内存使用优化
  - 性能分析（benchmark）

- [ ] **错误处理**
  - Terminal 错误恢复
  - Panic 处理（确保终端恢复）
  - Event channel 错误处理
  - 用户友好的错误消息

- [ ] **配置系统**
  - 支持配置文件（TOML）
  - 运行时配置（tick-rate, log-size, colors）
  - 默认配置嵌入

- [ ] **日志增强**
  - 日志级别过滤
  - 日志导出（写入文件）
  - 日志搜索增强（正则、高亮）

- [ ] **单元测试**
  - App 状态管理测试
  - 事件处理测试
  - LogBuffer 测试
  - 布局计算测试

- [ ] **集成测试**
  - 端到端流程测试
  - 多 agent 场景测试
  - 边界条件测试

- [ ] **文档**
  - 用户文档（README、USAGE）
  - 代码注释
  - 架构文档更新
  - 示例和截图

- [ ] **CI/CD**
  - 添加 TUI 测试到 CI
  - 自动化构建和发布

#### 交付物
- 性能优化的 TUI
- 完整的测试覆盖
- 用户和开发者文档

#### 依赖
- 阶段 3 完成

#### 工作量
- 开发: 12-16 小时
- 测试: 6-8 小时
- 文档: 4 小时

---

### 实现时间线

```
Week 1
├── Day 1-2: Phase 1 (MVP)
└── Day 3-5: Phase 2 (Monitoring)

Week 2
├── Day 1-3: Phase 3 (Interaction)
└── Day 4-5: Phase 4 (Enhancement)
```

**总计**: 8-12 天（约 2 周）

---

## 5. 技术选型

### 已使用的技术栈

| 技术 | 版本 | 用途 | 评估 |
|------|------|------|------|
| **ratatui** | 0.29 | TUI 框架 | ✅ 成熟、活跃维护、文档完善 |
| **crossterm** | 0.28 | 终端控制 | ✅ 跨平台、功能完整 |
| **tokio** | latest | 异步运行时 | ✅ 已集成、性能优秀 |

### ratatui 评估

**优点**:
- 即时模式渲染（简单直观）
- 丰富的 widget 库
- 良好的社区支持
- 活跃维护（从 tui-rs 迁移）

**缺点**:
- 无内置事件处理（需自己实现）
- 布局系统相对简单

**结论**: ✅ 满足需求，继续使用

### 是否需要新增依赖

#### 建议 1: `tui-logger` (可选)

```toml
tui-logger = "0.12"
```

**用途**: 日志面板 widget  
**理由**:
- 提供现成的日志 widget
- 支持日志级别过滤
- 减少自定义代码

**决策**: ⚠️ 可选。如果自定义需求简单，可以自己实现

#### 建议 2: `chrono` (已有)

```toml
chrono = "0.4"  # 已在依赖中
```

**用途**: 时间处理  
**理由**: 已有依赖，用于时间戳和持续时间计算

**决策**: ✅ 使用现有依赖

#### 建议 3: `regex` (可选)

```toml
regex = "1.10"
```

**用途**: 日志搜索和过滤  
**理由**:
- 支持正则表达式搜索
- Agent 名称过滤

**决策**: ⚠️ 可选。阶段 3-4 如需高级搜索时添加

### 备选方案

#### 备选 1: `cursive` (替代 ratatui)

```toml
cursive = "0.21"
```

**对比**:

| 特性 | ratatui | cursive |
|------|---------|---------|
| 渲染模式 | 即时模式 | 保留模式 |
| 学习曲线 | 低 | 中 |
| 灵活性 | 高 | 中 |
| 社区 | 大 | 中 |
| Widget 数量 | 中 | 多 |

**决策**: ❌ 不切换。ratatui 已够用，切换成本高

#### 备选 2: `termion` (替代 crossterm)

```toml
termion = "4.0"
```

**对比**:

| 特性 | crossterm | termion |
|------|-----------|---------|
| 跨平台 | ✅ Win/Linux/Mac | ❌ Unix only |
| 维护状态 | 活跃 | 活跃 |
| 功能 | 完整 | 完整 |

**决策**: ❌ 不切换。crossterm 跨平台更好

### 最终技术栈

```toml
[dependencies]
# TUI 核心
ratatui = "0.29"
crossterm = "0.28"

# 异步运行时
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }

# 时间处理
chrono = "0.4"  # 已有

# 可选：日志 widget
# tui-logger = "0.12"

# 可选：正则搜索
# regex = "1.10"
```

---

## 6. 风险和挑战

### 技术风险

| 风险 | 影响 | 可能性 | 缓解措施 |
|------|------|--------|----------|
| **终端兼容性问题** | 高 | 中 | 在主流终端测试（iTerm2, Windows Terminal, GNOME Terminal）；使用 crossterm 抽象层 |
| **性能瓶颈（大量 agent）** | 中 | 中 | 实现虚拟滚动；限制日志缓冲；增量渲染 |
| **事件通道阻塞** | 中 | 低 | 使用 unbounded channel；实现背压机制；监控队列长度 |
| **Terminal state 损坏** | 高 | 低 | 实现 panic hook 清理；使用 RAII guard；异常处理 |
| **并发竞争条件** | 高 | 低 | 使用 tokio sync primitives；充分测试；代码审查 |

### 用户体验挑战

| 挑战 | 影响 | 解决方案 |
|------|------|----------|
| **键盘快捷键学习曲线** | 中 | 提供帮助覆盖层；使用直观快捷键（vim 风格）；状态栏提示 |
| **小终端适配** | 中 | 响应式布局；最小尺寸警告；自适应隐藏元素 |
| **信息过载** | 中 | 分层次显示；折叠详情；过滤和搜索功能 |
| **颜色可读性** | 中 | 支持颜色方案；高对比度模式；遵守 terminal 颜色设置 |
| **无鼠标支持** | 低 | 文档说明；键盘导航优化 |

### 兼容性问题

#### 终端兼容性

| 终端 | 平台 | 支持级别 | 测试优先级 |
|------|------|----------|------------|
| **Windows Terminal** | Windows | ✅ 完全支持 | High |
| **iTerm2** | macOS | ✅ 完全支持 | High |
| **GNOME Terminal** | Linux | ✅ 完全支持 | High |
| **Alacritty** | 跨平台 | ✅ 完全支持 | Medium |
| ** Kitty** | Linux/macOS | ✅ 完全支持 | Medium |
| **cmd.exe** | Windows | ⚠️ 有限支持 | Low |
| **PowerShell** | Windows | ✅ 完全支持 | Medium |

#### 平台特定问题

**Windows**:
- 旧版 cmd.exe 可能不支持 alternate screen
- 解决：检测并警告，建议使用 Windows Terminal

**Linux**:
- 某些 SSH 环境可能颜色支持有限
- 解决：检测 COLORTERM 环境变量，降级到单色模式

**macOS**:
- 通常无问题
- 注意：默认 terminal 可能性能较差

### 依赖风险

| 依赖 | 风险 | 缓解 |
|------|------|------|
| ratatui | 低（活跃维护） | 定期更新 |
| crossterm | 低（稳定） | 定期更新 |
| tokio | 极低（广泛使用） | 跟随 LTS 版本 |

---

## 7. 成功指标

### 功能完整性

| 功能 | 目标 | 验收标准 |
|------|------|----------|
| **基础启动** | 100% | `loom tui` 命令成功启动并显示界面 |
| **Agent 网格** | 100% | 显示所有 agent，实时更新状态 |
| **Session 会话** | 100% | 显示对话历史，支持滚动 |
| **状态更新** | 100% | AgentStarted/Progress/Completed/Error 事件正确处理 |
| **详情查看** | 100% | 选择 agent 可查看完整信息 |
| **日志显示** | 100% | 实时滚动日志，支持翻页 |
| **键盘交互** | 100% | 所有快捷键正常工作 |
| **帮助系统** | 100% | 帮助覆盖层正确显示 |
| **退出清理** | 100% | 退出后 terminal 状态完全恢复 |

### 性能指标

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| **启动时间** | < 100ms | 从命令执行到界面显示 |
| **事件处理延迟** | < 10ms | 从事件发送到 UI 更新 |
| **渲染 FPS** | ≥ 4 FPS | 帧率（tick-rate 250ms） |
| **CPU 使用率** | < 5% | 空闲状态（无事件） |
| **内存使用** | < 50 MB | 100 个 agent，1000 条日志 |
| **响应时间** | < 50ms | 键盘输入到视觉反馈 |

### 用户体验目标

| 目标 | 指标 | 验收标准 |
|------|------|----------|
| **易用性** | 新用户 < 2 分钟上手 | 用户测试或团队内测 |
| **可发现性** | 所有功能可通过帮助发现 | 帮助覆盖层完整 |
| **错误恢复** | 无需手动恢复 terminal | Panic 后 terminal 正常 |
| **视觉清晰** | 信息层次清晰，易于扫描 | 设计审查 |
| **响应性** | 无明显卡顿 | 主观评估 + 性能测试 |

### 质量指标

| 指标 | 目标 | 验收标准 |
|------|------|----------|
| **代码覆盖率** | ≥ 70% | `cargo tarpaulin` |
| **文档覆盖率** | 100% public API | `cargo doc` 无警告 |
| **代码质量** | 无 clippy 警告 | `cargo clippy` |
| **格式化** | 100% 符合 rustfmt | `cargo fmt --check` |

### 测试矩阵

| 场景 | Agent 数量 | 日志量 | 终端大小 | 预期结果 |
|------|------------|--------|----------|----------|
| 基础 | 1 | 少 | 标准 80x24 | ✅ 正常显示 |
| 中等 | 10 | 中 | 标准 | ✅ 流畅滚动 |
| 大量 | 50 | 多 | 大 120x40 | ✅ 性能良好 |
| 极限 | 100 | 大量 | 大 | ⚠️ 可接受性能 |
| 小屏 | 5 | 少 | 小 60x20 | ✅ 警告但可用 |
| 长时间 | 5 | 持续增长 | 标准 | ✅ 内存稳定 |

---

## 附录

### A. 参考资源

- [ratatui 文档](https://docs.rs/ratatui/)
- [ratatui 示例](https://github.com/ratatui-org/ratatui/tree/main/examples)
- [crossterm 文档](https://docs.rs/crossterm/)
- [tokio tutorial](https://tokio.rs/tokio/tutorial)

### B. 相关 Issue 和 PR

- (待补充：创建后的 GitHub issue 链接)

### C. 设计草图

(待补充：UI 原型图、流程图等)

### D. 变更历史

| 版本 | 日期 | 作者 | 变更说明 |
|------|------|------|----------|
| 1.0 | 2025-01-14 | Loom Team | 初始版本 |

---

**文档结束**

如有问题或建议，请在 GitHub 上开 issue 或联系开发团队。
