# Loom TUI 交互文档 - 视图系统 (PaneStack)

## 概述

PaneStack 是 Loom TUI 交互架构的核心组件，负责管理用户界面的视图栈。采用栈式视图管理架构，通过 push/pop 操作实现多层视图的叠加和切换。PaneStack 是所有交互视图的容器，确保用户操作的一致性和界面状态的清晰性。

**核心设计理念**：栈式管理确保单一焦点，每次只有一个视图处于活跃状态，用户操作自然且符合直觉。

---

## 1. PaneView Trait 定义

PaneView 是所有视图组件必须实现的统一接口，定义了视图的基本行为和生命周期。

```rust
pub trait PaneView: Renderable {
    /// 处理按键事件
    /// 返回 Handled 表示已处理，NotHandled 表示传递给下一层
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;

    /// 视图是否已完成
    /// 返回 true 时，PaneStack 自动 pop 此视图
    fn is_complete(&self) -> bool;

    /// Ctrl+C 处理
    /// 视图可以返回 Cancel 来触发中断流程
    fn on_ctrl_c(&mut self) -> CtrlCAction;

    /// 视图标识符
    fn view_id(&self) -> Option<&'static str>;
}

/// 事件处理结果
pub enum Handled {
    /// 事件已处理，无需继续传递
    Handled,
    /// 事件未处理，传递给下一层
    NotHandled,
}

/// Ctrl+C 处理结果
pub enum CtrlCAction {
    /// 未处理，执行默认中断行为
    NotHandled,
    /// 已处理，无需中断
    Handled,
    /// 确认中断
    Cancel,
}
```

**接口设计来源推导**：
- `handle_key_event()` - 基于 Codex `ChatComposer` 的键盘事件处理机制 (`reference-codex.md` 第201-212行)
- `is_complete()` - 对比 Codex `ApprovalManager` 的 pending 生命周期管理 (`approval.rs:19`)
- `on_ctrl_c()` - 基于 Codex 中断处理机制 (`reference-codex.md` 第193行)
- `view_id()` - 支持调试和监控，对应 Codex 底部面板的视图标识 (`reference-codex.md` 第80-95行)
- `Renderable` 约束 - 统一渲染接口，与渲染层架构集成 (`architecture.md` 第206-222行)

---

## 2. PaneStack 实现

PaneStack 采用 `Vec<Box<dyn PaneView>>` 结构实现栈式视图管理。

### 2.1 核心结构

```rust
pub struct PaneStack {
    /// 视图栈，基座视图始终在底部
    views: Vec<Box<dyn PaneView>>,
    /// 回调通道，用于将视图结果传递给应用层
    callback_tx: mpsc::Sender<ViewEvent>,
}

/// 视图事件，用于视图与应用层通信
pub enum ViewEvent {
    /// 用户确认选择
    SelectionConfirmed(SelectionResult),
    /// 用户取消操作
    UserCancelled,
    /// 审批结果
    ApprovalResult(ApprovalResponse),
    /// 其他视图特定事件
    Custom { view_id: &'static str, data: serde_json::Value },
}
```

### 2.2 Push/Pop 操作语义

```
Push 操作:
┌─────────────────────────────────────────────────────────┐
│  PaneStack 初始状态                                       │
│  [ChatComposer]                                          │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼ push ApprovalOverlay
┌─────────────────────────────────────────────────────────┐
│  PaneStack push 后状态                                    │
│  [ChatComposer, ApprovalOverlay]                         │
│                          ↑ 活跃视图                      │
└─────────────────────────────────────────────────────────┘

Pop 操作:
┌─────────────────────────────────────────────────────────┐
│  PaneStack 完成 Approval 后                                │
│  检测到 is_complete() == true                             │
│  自动 pop → [ChatComposer]                                │
│                          ↑ 恢复为活跃视图                │
└─────────────────────────────────────────────────────────┘
```

**对比 Codex ApprovalManager**：
- Codex 使用 `pending: HashMap<String, oneshot::Sender<bool>>` 管理审批请求 (`approval.rs:19`)
- Loom PaneStack 使用栈式管理，更直观地表示视图层级
- 两者都支持异步响应传递，但 PaneStack 的栈结构更适合 UI 场景

### 2.3 事件路由规则

```
用户按键事件
    │
    ▼
PaneStack::handle_key_event(key)
    │
    ├── 检查视图栈是否为空?
    │   ├── 是 → 事件丢弃
    │   └── 否 → 继续处理
    │
    ├── 获取栈顶视图
    │   │
    │   ├── 调用 top_view.handle_key_event(key)
    │   │       ├── Handled → 事件处理完成，停止传递
    │   │       └── NotHandled → 继续传递
    │   │
    │   ├── 如果 NotHandled 且栈深度 > 1
    │   │   └── 递归调用下一层视图的 handle_key_event()
    │   │
    │   └── 如果所有视图都返回 NotHandled
    │       └── 事件丢弃
    │
    └── 处理完成后检查栈顶状态
        ├── is_complete() == true → pop 视图
        └── is_complete() == false → 保持当前状态
```

### 2.4 视图完成检测

PaneStack 在每次事件处理后自动检查栈顶视图的完成状态：

```rust
impl PaneStack {
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if let Some(top_view) = self.views.last_mut() {
            match top_view.handle_key_event(key) {
                Handled => {
                    // 事件已处理，检查是否完成
                    if top_view.is_complete() {
                        self.pop_completed_view();
                    }
                }
                NotHandled => {
                    // 事件未处理，向下传递
                    if self.views.len() > 1 {
                        // 递归处理下一层
                    }
                }
            }
        }
    }

    fn pop_completed_view(&mut self) {
        if let Some(view) = self.views.pop() {
            // 发送视图完成事件
            let _ = self.callback_tx.send(ViewEvent::UserCancelled);
        }
    }
}
```

---

## 3. 视图类型清单

Loom TUI 定义了 12 种标准视图类型，每种视图都有明确的用途、生命周期和交互方式。

### 3.1 ChatComposer（基座视图）

**视图 ID**: `"composer"`

**用途**: 主输入框，用户与 AI 对话的主要界面，始终存在于视图栈底部。

**生命周期**: 永久基座，从不 pop，永不完成。

**完成条件**: 无（基座视图不参与完成检测）

**按键映射**:
| 按键 | 功能 | 来源 |
|------|------|------|
| `Enter` | 提交输入 | `reference-codex.md:190` |
| `Shift+Enter` | 换行 | `reference-codex.md:191` |
| `↑/↓` | 历史导航 | `reference-codex.md:205` |
| `Ctrl+K` | 删除到行尾 | `reference-codex.md:206` |
| `Ctrl+U` | 删除到行首 | `reference-codex.md:207` |
| `Ctrl+W` | 删除前一个词 | `reference-codex.md:208` |
| `/` | Slash 命令 | `reference-codex.md:212` |

**特殊状态**: ChatComposer 维护输入状态机 (`architecture.md` 第329-360行)

---

### 3.2 ApprovalOverlay（审批弹窗）

**视图 ID**: `"approval"`

**用途**: AI 请求执行敏感操作时的审批界面，确保用户充分知情并控制。

**生命周期**: push → 用户响应 → pop

**完成条件**: 用户做出最终选择（Y/N/A）

**按键映射**:
| 按键 | 功能 | 来源 |
|------|------|------|
| `Y` / `Enter` | 接受请求 | `reference-codex.md:220` |
| `N` | 拒绝请求 | `reference-codex.md:221` |
| `D` | 查看差异详情 | `reference-codex.md:222` |
| `A` | 始终允许（当前会话） | `reference-codex.md:223` |
| `Esc` | 取消操作 | `reference-codex.md:224` |
| `↑/↓` | 多文件选择 | `reference-codex.md:225` |

**协作流程**: 与 ApprovalManager 对应 (`approval.rs:18-24`)

---

### 3.3 ListSelectionView（选择列表）

**视图 ID**: `"selection"`

**用途**: 快速做出结构化决策，避免复杂的命令输入。

**生命周期**: push → 选择/取消 → pop

**完成条件**: 用户选择或取消

**按键映射**:
| 按键 | 功能 | 来源 |
|------|------|------|
| `↑/↓` | 选择项 | `reference-codex.md:233` |
| `Enter` | 确认选择 | `reference-codex.md:234` |
| `Esc` | 取消 | `reference-codex.md:235` |
| `/` | 搜索过滤 | `reference-codex.md:236` |

**使用场景**: 模型选择、文件选择、技能选择等

---

### 3.4 FeedbackView（反馈提交）

**视图 ID**: `"feedback"`

**用途**: 收集用户对 AI 回复的反馈。

**生命周期**: push → 提交/取消 → pop

**完成条件**: 用户提交反馈或取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `Enter` | 提交反馈 |
| `Esc` | 取消 |
| `↑/↓` | 选择反馈类型 |

---

### 3.5 CustomPromptView（自定义提示词）

**视图 ID**: `"custom-prompt"`

**用途**: 编辑和提交自定义提示词模板。

**生命周期**: push → 提交/取消 → pop

**完成条件**: 用户提交或取消编辑

**按键映射**:
| 按键 | 功能 |
|------|------|
| `Enter` | 提交模板 |
| `Ctrl+S` | 保存草稿 |
| `Esc` | 取消编辑 |
| `↑/↓` | 光标移动 |

---

### 3.6 EffortIgnition（推理模式选择）

**视图 ID**: `"effort-ignition"`

**用途**: 选择 AI 推理模式（快速/深度思考）。

**生命周期**: push → 选择 → pop

**完成条件**: 用户选择推理模式

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 浏览模式 |
| `Enter` | 选择模式 |
| `Esc` | 取消 |
| `1/2` | 快速选择（1=快速，2=深度） |

---

### 3.7 FileSearchPopup（文件搜索弹窗）

**视图 ID**: `"file-search"`

**用途**: 在文件系统中搜索文件，支持路径补全。

**生命周期**: push → 选择/取消 → pop

**完成条件**: 用户选择文件或取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 浏览结果 |
| `Enter` | 选择文件 |
| `Esc` | 取消搜索 |
| `Tab` | 路径补全 |

---

### 3.8 CommandPopup（命令弹窗）

**视图 ID**: `"command-popup"`

**用途**: 执行或配置终端命令。

**生命周期**: push → 执行/取消 → pop

**完成条件**: 命令执行完成或用户取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `Enter` | 执行命令 |
| `Esc` | 取消执行 |
| `Tab` | 命令补全 |

---

### 3.9 SkillPopup（技能弹窗）

**视图 ID**: `"skill-popup"`

**用途**: 选择或配置 AI 技能。

**生命周期**: push → 选择/取消 → pop

**完成条件**: 用户选择技能或取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 浏览技能 |
| `Enter` | 选择技能 |
| `Esc` | 取消 |
| `?` | 查看技能详情 |

---

### 3.10 HooksBrowserView（Hooks 浏览器）

**视图 ID**: `"hooks-browser"`

**用途**: 浏览和管理系统 Hooks。

**生命周期**: push → 完成 → pop

**完成条件**: 用户完成操作

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 浏览 Hooks |
| `Enter` | 查看详情 |
| `D` | 删除 Hook |
| `Esc` | 关闭浏览器 |

---

### 3.11 MemoriesSettingsView（记忆设置）

**视图 ID**: `"memories-settings"`

**用途**: 配置 AI 记忆系统的参数。

**生命周期**: push → 保存/取消 → pop

**完成条件**: 用户保存设置或取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 选择设置项 |
| `Enter` | 编辑设置 |
| `Ctrl+S` | 保存设置 |
| `Esc` | 取消修改 |

---

### 3.12 McpServerElicitation（MCP 服务器选择）

**视图 ID**: `"mcp-server-elicitation"`

**用途**: 选择 MCP（Model Context Protocol）服务器。

**生命周期**: push → 选择 → pop

**完成条件**: 用户选择服务器或取消

**按键映射**:
| 按键 | 功能 |
|------|------|
| `↑/↓` | 浏览服务器列表 |
| `Enter` | 选择服务器 |
| `Esc` | 取消选择 |
| `+` | 添加新服务器 |

---

## 4. 视图生命周期

视图从创建到销毁经历完整的状态转换，PaneStack 负责管理这些转换。

### 4.1 生命周期状态机

```
视图生命周期:
  ┌──────────┐
  │  Created  │ ← push 到 PaneStack
  └────┬─────┘
       │
       ▼
  ┌──────────┐
  │  Active   │ ← 栈顶，接收事件
  └────┬─────┘
       │
       ├── is_complete() == true → pop
       │
       └── 用户按 Esc → 取消 → pop
              │
              ▼
  ┌──────────┐
  │  Destroyed│ ← 从 PaneStack 移除
  └──────────┘
```

### 4.2 Push 时机

```
应用层触发 push:
┌─────────────────────────────────────────────────────────┐
│  应用层检测到需要用户交互                                 │
│  (如 AI 请求审批、需要用户选择等)                         │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  创建对应视图实例                                         │
│  let view = ApprovalOverlay::new(request);               │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  调用 PaneStack::push(view)                              │
│  views.push(Box::new(view));                             │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  新视图变为栈顶，开始接收事件                             │
│  触发 render() 更新显示                                   │
└─────────────────────────────────────────────────────────┘
```

### 4.3 Pop 时机

```
自动 pop (is_complete()):
┌─────────────────────────────────────────────────────────┐
│  用户在视图中完成操作                                     │
│  (如选择 Y 同意审批)                                      │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  视图内部设置完成标志                                     │
│  self.completed = true;                                  │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  PaneStack 检测到 is_complete() == true                   │
│  自动调用 pop()                                           │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  视图从栈中移除，下一层变为活跃                           │
│  发送 ViewEvent 给应用层                                 │
└─────────────────────────────────────────────────────────┘

手动 pop (Esc 取消):
┌─────────────────────────────────────────────────────────┐
│  用户按 Esc 键                                            │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  视图 handle_key_event() 处理 Esc                        │
│  返回 Handled 并设置取消标志                              │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  PaneStack 检测到取消，立即 pop()                        │
│  发送 UserCancelled 事件                                 │
└─────────────────────────────────────────────────────────┘
```

### 4.4 与 ItemTracker 的生命周期对比

```
ItemTracker 生命周期 (Codex):
┌─────────────────────────────────────────────────────────┐
│  ItemStarted → ItemUpdated → ItemCompleted              │
│  对应 AI 流式输出的一条消息                               │
└─────────────────────────────────────────────────────────┘
         来源: event_bridge.rs:70-82, 86-99

PaneView 生命周期 (Loom):
┌─────────────────────────────────────────────────────────┐
│  Created → Active → Destroyed                            │
│  对应用户界面视图的显示和交互                             │
└─────────────────────────────────────────────────────────┘

关键区别:
- ItemTracker: 单向推进，不可回退
- PaneView: 可能回退（用户取消），可能跳过（中断）
- ItemTracker: 数据流生命周期
- PaneView: 用户交互生命周期
```

---

## 5. 对抗性验证

### 5.1 边缘情况

#### 空栈情况
```
边缘情况: PaneStack 为空 (views.is_empty())
风险: 所有事件无法处理，用户失去控制

防护措施:
1. ChatComposer 作为永久基座，永远不在 pop 范围内
2. 空栈时直接丢弃事件，不 panic
3. 启动时强制 push ChatComposer

实现:
pub fn new() -> Self {
    let mut stack = Self {
        views: Vec::new(),
        callback_tx: /* ... */,
    };
    stack.push(ChatComposer::new()); // 永久基座
    stack
}
```

#### 栈溢出情况
```
边缘情况: 无限 push 导致栈溢出
风险: 内存耗尽，应用崩溃

防护措施:
1. 设置最大栈深度限制 (MAX_STACK_DEPTH = 10)
2. 达到限制时拒绝新 push，记录警告
3. 强制 pop 最底层非基座视图

实现:
const MAX_STACK_DEPTH: usize = 10;

pub fn push(&mut self, view: Box<dyn PaneView>) {
    if self.views.len() >= MAX_STACK_DEPTH {
        log::warn!("Stack overflow, forcing pop of oldest non-base view");
        self.views.remove(1); // 保留基座，移除最老的非基座视图
    }
    self.views.push(view);
}
```

#### 推入重复视图
```
边缘情况: 相同视图被重复推入栈
风险: 用户体验混乱，状态不一致

防护措施:
1. 检查 view_id() 重复
2. 对于单例视图（如 ChatComposer），拒绝重复 push
3. 对于允许多实例的视图，允许但警告

实现:
pub fn push(&mut self, view: Box<dyn PaneView>) {
    let view_id = view.view_id();
    if let Some(id) = view_id {
        if id == "composer" {
            log::warn!("Attempted to push duplicate base view");
            return;
        }
        // 检查重复...
    }
    self.views.push(view);
}
```

### 5.2 失败模式

#### 视图无法完成
```
失败模式: 视图卡在非完成状态，用户无法退出
风险: 界面冻结，用户失去控制

恢复机制:
1. 全局 Esc 处理：优先于视图处理，强制 pop
2. 超时检测：视图活跃超过 TIMEOUT（如 5 分钟）自动 pop
3. Ctrl+C 硬中断：直接清空栈，回到基座状态

实现:
pub fn handle_key_event(&mut self, key: KeyEvent) {
    match key {
        KeyEvent { code: KeyCode::Esc, .. } => {
            // 全局 Esc，强制 pop
            if self.views.len() > 1 {
                self.pop();
            }
        }
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
        } => {
            // Ctrl+C 硬中断
            self.force_reset_to_base();
        }
        _ => {
            // 正常事件路由...
        }
    }
}
```

#### 视图泄露
```
失败模式: 视图完成但未正确 pop，或资源未释放
风险: 内存泄露，性能下降

检测机制:
1. 使用 RAII 模式，视图析构时自动清理
2. 定期扫描 completed 状态，强制清理
3. 监控栈深度异常

实现:
impl Drop for PaneStack {
    fn drop(&mut self) {
        // 确保所有视图资源正确释放
        while let Some(view) = self.views.pop() {
            // 视图的 Drop trait 会自动清理
        }
    }
}
```

#### 事件丢失
```
失败模式: 视图返回 NotHandled 但未正确传递到下一层
风险: 用户体验不连贯，按键无响应

防护措施:
1. 严格的事件传递协议
2. 事件处理审计日志
3. 丢失事件时提示用户

实现:
pub fn handle_key_event(&mut self, key: KeyEvent) {
    let mut handled = false;
    let mut processed_count = 0;

    for view in self.views.iter_mut().rev() {
        match view.handle_key_event(key.clone()) {
            Handled => {
                handled = true;
                processed_count += 1;
                break;
            }
            NotHandled => {
                processed_count += 1;
            }
        }
    }

    if !handled && processed_count > 0 {
        log::warn!("Event lost after {} views", processed_count);
        // 可选：显示用户提示
    }
}
```

### 5.3 设计权衡

#### 栈式 vs 树形视图管理

```
栈式管理（PaneStack）:
优势:
✅ 实现简单，无需复杂的层级管理
✅ 用户直觉清晰，先进后出
✅ 内存占用可预测
✅ 适合对话式交互的单焦点场景

劣势:
❌ 不支持多视图同时可见
❌ 不支持视图间的并行交互
❌ 复杂嵌套场景可能需要深层栈

树形管理:
优势:
✅ 支持多视图同时可见
✅ 支持复杂的层级关系
✅ 适合工作区式界面

劣势:
❌ 实现复杂，需要焦点管理系统
❌ 内存占用不可控
❌ 用户学习成本高
❌ 不适合终端有限屏幕空间

结论: 对于对话式 TUI，栈式管理更合适
```

#### 栈式 vs 多窗口视图管理

```
栈式管理:
优势:
✅ 单一焦点，避免冲突
✅ 事件路由简单
✅ 屏幕空间利用高效

劣势:
❌ 无法同时查看多个内容
❌ 无法拖拽和重新排列

多窗口管理:
优势:
✅ 支持并排查看
✅ 支持拖拽重新排列
✅ 适合复杂工作流

劣势:
❌ 需要复杂窗口管理器
❌ 焦点管理复杂
❌ 终端屏幕空间受限
❌ 实现成本高

结论: TUI 场景下，栈式管理更适合
```

### 5.4 设计限制

```
1. 不支持多视图同时可见
   - 原因：栈式结构天然单焦点
   - 影响：无法同时查看多个弹窗或列表
   - 缓解：通过快速切换和状态指示提供足够信息

2. 不支持视图拖拽和重新排列
   - 原因：栈式顺序由 push/pop 决定
   - 影响：用户无法自定义视图顺序
   - 缓解：合理的默认顺序，符合用户预期

3. 深层嵌套可能降低用户体验
   - 原因：深层栈需要多次 Esc 才能返回
   - 影响：复杂流程可能导致用户迷失
   - 缓解：限制最大栈深度，优化交互流程

4. 无并发视图状态
   - 原因：栈是线性的，不支持并行
   - 影响：后台任务无法实时展示状态
   - 缓解：通过状态指示器和通知系统
```

---

## 6. 总结

PaneStack 视图系统是 Loom TUI 交互架构的核心，通过精心设计的栈式管理机制，为用户提供了清晰、一致、可控的交互体验。

**核心优势**:
1. **架构简洁**: 栈式管理逻辑清晰，易于理解和维护
2. **用户体验自然**: 先进后出符合用户直觉，学习成本低
3. **实现高效**: 单一焦点模型，事件路由简单
4. **可扩展性强**: 通过 PaneView trait 统一接口，易于添加新视图

**设计一致性**:
- 与 Codex BottomPane 架构一脉相承 (`reference-codex.md:52-78`)
- 与交互架构的 5 层模型无缝集成 (`architecture.md:187-278`)
- 遵循 Rust 的 trait 系统设计模式

**对抗性验证**:
- 考虑了空栈、溢出、重复等边缘情况
- 针对视图无法完成、泄露、事件丢失等失败模式提供了防护
- 分析了栈式 vs 树形、多窗口等设计权衡
- 明确了设计限制和缓解措施

PaneStack 通过严格的生命周期管理和事件路由机制，确保了 Loom TUI 交互系统的稳定性和用户体验的一致性，为构建复杂的 AI 协作界面提供了坚实的基础。