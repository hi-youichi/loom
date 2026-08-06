# Codex CLI TUI 交互设计文档

## 概述

本文档描述 Codex CLI TUI 的用户交互流程、界面布局、按键映射以及视觉反馈机制。所有描述均基于实际代码库验证。

**基于代码库**：`https://github.com/openai/codex`（`codex-rs/tui/` crate），commit `5af85998c2`

---

## 1. 界面布局

### 1.1 整体视图

Codex TUI 采用**内联视图（Inline Viewport）**，不占用全屏终端，而是在终端历史中嵌入一个聊天区域：

```
┌─────────────────────────────────────────────────────┐
│ 终端历史（正常滚动区域）                                │
│ 用户之前的 shell 输出                                  │
│                                                        │
├─────────────────────────────────────────────────────┤  ← viewport 顶部
│ 聊天区域（渲染在 tui.rs 的 draw() 中）                  │
│ ┌─ User ──────────────────────────────────────────┐ │
│ │ 帮我优化这个 Rust 函数                            │ │
│ └──────────────────────────────────────────────────┘ │
│ ┌─ Assistant ──────────────────────────────────────┐ │
│ │ 好的，我来帮你修改...                              │ │
│ │ ◌ 正在思考...                                    │ │  ← spinner
│ └──────────────────────────────────────────────────┘ │
│ ┌─ 输入框 ─────────────────────────────────────────┐ │
│ │ > _                                                │ │
│ │ [Ctrl+Enter 提交]                                  │ │
│ └──────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────┤  ← viewport 底部
│ $ _   ← 光标在下方（正常终端提示符）                     │
└─────────────────────────────────────────────────────┘
```

### 1.2 界面区域划分

| 区域 | 说明 | 实现文件 |
|------|------|----------|
| 终端历史 | 用户启动 Codex 之前的 shell 输出 | 终端原生 |
| **聊天区域** | AI 对话渲染区 | `chatwidget.rs` + `chatwidget/rendering.rs` |
| ├ User cell | 用户消息 | `history_cell/` 模块 |
| ├ Assistant cell | AI 回复 | `history_cell/` 模块 |
| ├ Status row | 正在思考/执行中的 spinner | `status_indicator_widget.rs` |
| └ **底部面板** | 输入框、审批、弹窗等 | `bottom_pane/` 模块 |
| 终端提示符 | 退出后的正常 shell | 终端原生 |

### 1.3 底部面板（Bottom Pane）

底部面板是交互的核心区域，实现为 `bottom_pane/mod.rs` 中的 `BottomPane` 结构体（`pub(crate)`），采用栈式视图管理，可以 push/pop 不同视图：

```
┌─ 底部面板（默认：输入框） ──────────────────────────┐
│ > 帮我写一个 Rust 解析器_                             │
│ [Ctrl+Enter 提交]                                    │
└──────────────────────────────────────────────────────┘

┌─ 底部面板（审批弹窗，approval_overlay.rs） ──────────┐
│ ┌─ 审批请求 ────────────────────────────────────┐  │
│ │ AI 想修改 src/parser.rs                        │  │
│ │ 差异: +36 / -12 行                            │  │
│ │ [Y] 接受  [N] 拒绝  [D] 查看差异               │  │
│ └────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘

┌─ 底部面板（选择列表，list_selection_view.rs） ────────┐
│ ┌─ 选择模型 ────────────────────────────────────┐  │
│ │ → GPT-4o (推荐)                               │  │
│ │   GPT-4o-mini                                 │  │
│ │   o1-mini                                     │  │
│ │   o1-preview                                  │  │
│ └────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

### 1.4 底部面板视图类型

| 视图 | 实现文件 | 用途 |
|------|----------|------|
| `ChatComposer` | `bottom_pane/chat_composer.rs` | 主输入框（基座视图） |
| `ApprovalOverlay` | `bottom_pane/approval_overlay.rs` | 审批弹窗 |
| `ListSelectionView` | `bottom_pane/list_selection_view.rs` | 选择列表 |
| `FeedbackView` | `bottom_pane/feedback_view.rs` | 反馈提交 |
| `CustomPromptView` | `bottom_pane/custom_prompt_view.rs` | 自定义提示词 |
| `EffortIgnition` | `bottom_pane/effort_ignition.rs` | 推理模式选择 |
| `FileSearchPopup` | `bottom_pane/file_search_popup.rs` | 文件搜索弹窗 |
| `CommandPopup` | `bottom_pane/command_popup.rs` | 命令弹窗 |
| `SkillPopup` | `bottom_pane/skill_popup.rs` | 技能弹窗 |
| `HooksBrowserView` | `bottom_pane/hooks_browser_view.rs` | Hooks 浏览器 |
| `MemoriesSettingsView` | `bottom_pane/memories_settings_view.rs` | 记忆设置 |
| `McpServerElicitation` | `bottom_pane/mcp_server_elicitation.rs` | MCP 服务器选择 |

---

## 2. 用户工作流

### 2.1 主流程

```
启动 Codex
  │
  ├── 首次运行 → 配置向导（模型选择、权限设置）
  │
  ├── 恢复会话 → resume_picker.rs 选择之前的会话继续
  │
  └── 新会话
        │
        ▼
  输入提示词（ChatComposer）
        │
        ▼
  提交（Enter / Tab）
        │
        ▼
  AI 处理中...
   ├── status_indicator_widget.rs spinner 动画
   ├── chatwidget/streaming.rs 流式输出
   └── app/agent_status_feed.rs 子代理状态
        │
        ▼
  AI 输出结果
   ├── 文本回复（history_cell/messages.rs）
   ├── 代码块（history_cell/patches.rs）
   ├── 文件修改（需审批，approval_overlay.rs）
   └── 命令执行（需审批，exec_command.rs）
        │
        ▼
  用户交互
   ├── 继续对话
   ├── 接受/拒绝修改
   ├── 允许/阻止命令
    └── 中断（Ctrl+C → agent.rs turn/cancel + cancel_flag）
```

### 2.2 审批流程

```
AI 发起文件修改请求
  → bottom_pane push ApprovalOverlay 视图
  → 用户选择：
      ├── Y / Enter → 接受修改
      ├── N → 拒绝修改
      ├── D → 查看差异预览
      └── A → 始终允许本次会话
  → 结果通过 app_server_requests.rs 反馈给 AI
  → pop 返回 ChatComposer
```

### 2.3 命令执行流程

```
AI 提议执行命令
  → bottom_pane push 命令审批视图
  → 用户选择：
      ├── Y → 允许执行
      ├── N → 阻止
      └── A → 始终允许本次会话
  → 命令执行（exec_command.rs）
  → 输出结果返回给 AI
  → 继续对话
```

### 2.4 多代理工作流

```
用户提交复杂任务
  → 主代理接收任务
  → app/agent_navigation.rs 管理代理导航
  → app/agent_status_feed.rs 展示子代理状态
  → app/side.rs 管理 SideThread
  → 子代理完成
  → 主代理汇总结果
  → 输出给用户
```

---

## 3. 按键映射

### 3.1 全局按键

按键映射由 `keymap.rs` + `keymap_setup.rs` 管理，绑定关系通过 `RuntimeKeymap` 运行时动态构建：

| 按键 | 功能 | 说明 |
|------|------|------|
| `Enter` | 提交输入 | 普通模式下提交当前输入 |
| `Shift+Enter` | 换行 | 在输入框中换行 |
| `Tab` | 提交或排队 | 无任务时提交，有任务时排队 |
| `Ctrl+C` | 中断/取消 | 中断当前 AI 操作（`agent.rs` turn/cancel + cancel_flag）或取消弹窗 |
| `Ctrl+D` | 退出 Codex | 结束会话 |
| `Ctrl+Z` | 暂停（Unix） | 暂停 Codex（`tui/job_control.rs`） |
| `Ctrl+T` | 转录覆盖层 | 查看完整对话历史（`pager_overlay.rs`） |
| `Ctrl+R` | 历史搜索 | 反向搜索输入历史 |

### 3.2 输入框按键

由 `bottom_pane/chat_composer.rs` 中的 `ChatComposer` 处理：

| 按键 | 功能 | 说明 |
|------|------|------|
| `↑/↓` | 历史导航 | 遍历输入历史（`ChatComposerHistory`） |
| `Ctrl+K` | 删除到行尾 | 删除光标到行尾的内容 |
| `Ctrl+U` | 删除到行首 | 删除光标到行首的内容 |
| `Ctrl+W` | 删除前一个词 | 删除光标前的一个词 |
| `Ctrl+A` | 到行首 | 移动光标到行首 |
| `Ctrl+E` | 到行尾 | 移动光标到行尾 |
| `Ctrl+←/→` | 按词移动 | 按词移动光标 |
| `/` | Slash 命令 | 触发 slash 命令弹出（`slash_command.rs`） |

### 3.3 审批视图按键

由 `bottom_pane/approval_overlay.rs` 处理：

| 按键 | 功能 |
|------|------|
| `Y` / `Enter` | 接受 |
| `N` | 拒绝 |
| `D` | 查看差异 |
| `A` | 始终允许 |
| `Esc` | 取消 |
| `↑/↓` | 选择文件（多文件时） |

### 3.4 选择列表按键

由 `bottom_pane/list_selection_view.rs` 处理：

| 按键 | 功能 |
|------|------|
| `↑/↓` | 选择项 |
| `Enter` | 确认选择 |
| `Esc` | 取消 |
| `/` | 搜索过滤 |

### 3.5 转录覆盖层按键

由 `pager_overlay.rs` 中的 `TranscriptOverlay` 处理：

| 按键 | 功能 |
|------|------|
| `↑/↓` | 滚动 |
| `PgUp/PgDn` | 翻页 |
| `Enter` | 选择并插入到输入 |
| `Esc` | 关闭覆盖层 |

---

## 4. 视觉反馈系统

### 4.1 Spinner 动画

由 `frames.rs`（71 行）定义 10 种动画风格，`ascii_animation.rs` 驱动，每帧 80ms 切换：

| 风格 | 实现位置 |
|------|----------|
| `default` | `frames/default/` |
| `codex` | `frames/codex/` |
| `openai` | `frames/openai/` |
| `blocks` | `frames/blocks/` |
| `dots` | `frames/dots/` |
| `hash` | `frames/hash/` |
| `hbars` | `frames/hbars/` |
| `vbars` | `frames/vbars/` |
| `shapes` | `frames/shapes/` |
| `slug` | `frames/slug/` |

### 4.2 状态指示

由 `status_indicator_widget.rs` 和 `app/agent_status_feed.rs` 实现：

| 状态 | 视觉表现 | 实现文件 |
|------|----------|----------|
| AI 正在思考 | Spinner 动画 + "正在思考..." | `ascii_animation.rs` |
| AI 正在执行 | Spinner 动画 + "正在编译..." | `status_indicator_widget.rs` |
| 等待审批 | 底部面板弹出审批视图 | `approval_overlay.rs` |
| 子代理活动 | 状态栏显示子代理数量 | `app/agent_status_feed.rs` |
| 网络请求 | 速率限制指示器 | `chatwidget/rate_limits.rs` |
| 错误 | 红色高亮错误信息 | `style.rs` |

### 4.3 消息样式

由 `style.rs`、`markdown_render.rs`、`diff_render.rs` 实现：

| 消息类型 | 样式 | 实现 |
|----------|------|------|
| 用户消息 | 普通文本 | `history_cell/messages.rs` |
| AI 回复 | 普通文本 + 代码块高亮 | `markdown_render.rs` |
| 代码块 | 语法高亮（bash 高亮） | `highlight.rs` |
| 文件差异 | 绿色（+）/ 红色（-） | `diff_render.rs` |
| 系统消息 | 灰色/斜体 | `style.rs` |
| 错误消息 | 红色 | `style.rs` |
| 审批请求 | 带边框的弹窗 | `approval_overlay.rs` |

### 4.4 桌面通知

由 `notifications.rs` 实现，`DesktopNotificationBackend` 自动检测平台后端：

```
通知条件：
  - Unfocused（默认）：仅终端失焦时通知
  - Always：始终通知

通知时机：
  - AI 完成回复
  - 需要用户审批
  - 出现错误
```

---

## 5. 进程管理交互

### 5.1 暂停/恢复（Unix）

由 `tui/job_control.rs` 中的 `SuspendContext` 实现：

```
用户按 ^Z
  → SuspendContext::prepare_suspend_action()
  → 终端恢复到正常模式（保留 raw mode）
  → 暂停事件轮询
  → 发送 SIGSTOP

恢复（fg）
  → 进程收到 SIGCONT
  → 重新设置终端模式
  → 恢复事件轮询
  → 继续对话
```

### 5.2 运行外部程序

由 `tui.rs` 的 `with_restored()` 方法处理：

```
1. 用户输入需要运行外部程序
2. Tui::with_restored() 暂停事件轮询
3. 恢复终端模式
4. 运行外部程序
5. 程序退出后重新设置终端模式
6. 恢复事件轮询
7. 继续对话
```

---

## 6. 状态流转

### 6.1 应用状态机

由 `app.rs` 和 `session_state.rs` 中的 `ThreadSessionState` 管理：

```
┌─────────────────────────────────────────────────────────────┐
│                      App 状态机                              │
├─────────────────────────────────────────────────────────────┤
│  Idle ──→ Submitting ──→ Waiting ──→ Streaming ──→ Idle    │
│   ↑          │            │            │                    │
│   │          │            │            │                    │
│   └──────────┴────────────┴────────────┘                    │
│                    ↑↓                                       │
│              Interrupted (agent.rs: turn/cancel)             │
│                    ↑↓                                       │
│              Suspended (tui/job_control.rs)                 │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 输入状态机

由 `bottom_pane/chat_composer.rs` 中的 `ChatComposer` 管理：

```
┌────────────────────────────────────────────────┐
│              ChatComposer 状态机                 │
├────────────────────────────────────────────────┤
│  Empty ──→ Editing ──→ Submitting ──→ Empty    │
│   ↑          ↑↓                                │
│   └──── PopupActive                             │
│         ↑↓                                      │
│   SlashCommand / FileSearch / Mention / Skill   │
└────────────────────────────────────────────────┘
```

### 6.3 底部面板视图栈

由 `bottom_pane/mod.rs` 中的 `BottomPane` 管理：

```
┌────────────────────────────────────────────────┐
│              BottomPane 视图栈                   │
├────────────────────────────────────────────────┤
│  [Composer]                                     │
│  [Composer, ApprovalOverlay]  ← push            │
│  [Composer]                    ← pop            │
│  [Composer, ListSelectionView] ← push           │
│  [Composer]                    ← pop            │
│  [Composer, FeedbackView]      ← push           │
│  [Composer]                    ← pop            │
└────────────────────────────────────────────────┘
```

---

## 7. 特殊交互场景

### 7.1 终端 Resize

由 `tui.rs` 的 `update_inline_viewport_for_resize_reflow()` 和 `transcript_reflow.rs` 处理：

```
用户调整终端窗口大小
  → 自动检测尺寸变化（tui/event_stream.rs）
  → 调整 viewport 位置：
      - 缩小 → 上滚历史区域
      - 变大 → 如果底部对齐，向下扩展
  → 需要时触发全量重绘（transcript_reflow.rs）
  → 保持光标位置正确
```

### 7.2 粘贴处理

由 `clipboard_paste.rs` 和 `tui/event_stream.rs` 处理：

```
用户粘贴文本（Ctrl+Shift+V 或鼠标中键）
  → 终端发送 bracketed paste 标记
  → 解析粘贴内容
  → 插入到输入框
  → 保持格式（多行文本）
```

### 7.3 外部编辑器

由 `external_editor.rs` 实现：

```
用户在输入框中触发外部编辑器（配置的编辑器）
  → 保存当前输入
  → 打开外部编辑器（如 vim）
  → 用户编辑
  → 保存并退出
  → 读取编辑内容（通过 input_restore.rs）
  → 恢复到输入框
```

---

## 8. 总结

Codex TUI 的交互设计遵循以下原则：

1. **最小侵入**：内联视图不占用全屏，保留终端原生体验
2. **可控安全**：每一步修改和执行都需要用户确认
3. **反馈及时**：Spinner、状态指示、通知确保用户知道 AI 正在做什么
4. **键盘驱动**：所有操作都可以通过键盘完成，无需鼠标
5. **状态可恢复**：`^Z` 暂停/恢复、会话恢复、回放等功能确保用户不会丢失上下文

所有交互逻辑均通过明确的模块边界实现，每个视图类型有独立的文件，按键映射通过 `keymap.rs` 运行时动态绑定。
