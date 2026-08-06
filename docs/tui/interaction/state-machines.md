# Loom TUI 交互文档 - 状态机设计

## 概述

本文档详细描述 Loom TUI 的状态机设计，包括应用状态机、输入状态机、状态转换表、中断处理机制和状态一致性保证。状态机是确保用户交互行为可预测、可追溯的核心机制。

**设计目标**：
- **确定性**：所有状态转换都有明确的触发条件和预期行为
- **可观测性**：每个状态都有清晰的视觉表现和用户反馈
- **可恢复性**：支持中断、暂停、恢复等操作
- **可扩展性**：易于添加新状态和转换规则

---

## 1. 应用状态机 (App State Machine)

### 1.1 状态定义

应用状态机管理 Loom TUI 的整体交互状态，包含 9 种状态：

```rust
pub enum AppState {
    /// 等待用户输入
    Idle,
    /// 提交处理中
    Submitting, 
    /// 等待 AI 响应
    Waiting,
    /// AI 流式输出中
    Streaming,
    /// 等待用户审批
    WaitingApproval,
    /// AI 执行操作中
    Executing,
    /// 中断状态 (用户 Ctrl+C)
    Interrupted,
    /// 暂停状态 (Unix ^Z)
    Suspended,
    /// 错误状态
    Error,
}
```

### 1.2 状态详细描述

#### Idle (空闲状态)

**进入条件**：
- 系统启动完成
- 上一轮对话完成
- 用户中断恢复
- 用户拒绝审批

**退出条件**：
- 用户提交输入 → Submitting

**允许的交互**：
- 所有输入框编辑功能
- 历史记录浏览
- 配置修改
- 会话管理操作

**视觉表现**：
- 输入框正常显示
- 无 spinner 动画
- 光标在输入框内
- 状态栏显示 "Ready"

**参考实现**：`ChatComposer::is_empty()` + 无 pending 消息

#### Submitting (提交状态)

**进入条件**：
- Idle 状态下用户提交输入

**退出条件**：
- 提交处理完成 → Waiting
- 提交失败 → Idle

**允许的交互**：
- 仅允许 Ctrl+C 中断
- 其他交互被禁用

**视觉表现**：
- 输入框禁用（灰色）
- 显示 spinner 动画
- 状态栏显示 "Submitting..."
- 光标隐藏

**参考实现**：Codex `ThreadSessionState` 的 turn 开始阶段

#### Waiting (等待状态)

**进入条件**：
- Submitting 处理完成

**退出条件**：
- AI 开始输出 → Streaming
- AI 请求审批 → WaitingApproval
- AI 错误 → Idle
- 超时 → Idle

**允许的交互**：
- Ctrl+C 中断
- 状态查看

**视觉表现**：
- 输入框禁用
- 显示 spinner（思考状态）
- 状态栏显示 "Waiting for AI..."
- 可能显示 "thinking..." 提示

**参考实现**：Codex EventState 的 `turn` 状态

#### Streaming (流式输出状态)

**进入条件**：
- Waiting 状态下 AI 开始输出
- Executing 状态下 AI 继续输出

**退出条件**：
- AI 请求审批 → WaitingApproval
- AI 完成 → Idle
- AI 错误 → Idle
- 用户中断 → Interrupted

**允许的交互**：
- Ctrl+C 中断
- 查看输出内容
- 滚动历史记录

**视觉表现**：
- 输入框禁用
- 实时显示流式输出
- 无 spinner（流式内容本身即是进度）
- 状态栏显示 "AI responding..."
- 可能显示字符计数

**参考实现**：Codex EventState 的 `reply_started` 状态

#### WaitingApproval (审批状态)

**进入条件**：
- Streaming 状态下 AI 请求审批

**退出条件**：
- 用户同意 → Executing
- 用户拒绝 → Idle
- 用户中断 → Interrupted

**允许的交互**：
- Y/N/D/A 按键响应
- 查看差异详情
- Ctrl+C 中断

**视觉表现**：
- 输入框被覆盖
- 显示审批弹窗
- 状态栏显示 "Awaiting approval..."
- 高亮显示待审批内容

**参考实现**：Codex `ApprovalOverlay` 显示状态

#### Executing (执行状态)

**进入条件**：
- WaitingApproval 状态下用户同意

**退出条件**：
- 执行完成 → Idle
- 执行失败 → Idle
- 需要进一步审批 → WaitingApproval
- 用户中断 → Interrupted

**允许的交互**：
- Ctrl+C 中断
- 查看执行进度

**视觉表现**：
- 输入框禁用
- 显示 spinner（执行状态）
- 状态栏显示 "Executing..."
- 可能显示进度条

**参考实现**：Codex EventState 的 `in_thinking` 状态

#### Interrupted (中断状态)

**进入条件**：
- 任意状态下用户按 Ctrl+C（除非被视图处理）

**退出条件**：
- 清理完成 → Idle
- 恢复操作 → Streaming/Executing（如果支持）

**允许的交互**：
- 确认中断操作
- 查看中断原因

**视觉表现**：
- 输入框可能显示错误信息
- 状态栏显示 "Interrupted"
- 可能显示清理进度
- 警告色高亮

**参考实现**：Codex `agent.rs` 的 `turn/cancel` 路由处理 + `cancel_flag: AtomicBool` 机制

### 1.3 状态转换图

```
                    ┌───────────┐
                    │   Idle    │ ← 等待用户输入
                    └─────┬─────┘
                          │ 用户提交输入
                          ▼
                    ┌──────────────┐
                    │  Submitting  │ ← 提交处理中
                    └──────┬───────┘
                           │ 提交完成
                           ▼
                    ┌─────────────┐
              ┌─────│  Waiting    │ ← 等待 AI 响应
              │     └──────┬──────┘
              │            │ AI 开始输出
              │            ▼
              │     ┌──────────────┐
              │     │  Streaming   │ ← AI 流式输出中
              │     └──────┬───────┘
              │            │ AI 请求审批 / AI 完成
              │            ▼
              │     ┌──────────────────┐
              │     │  WaitingApproval │ ← 等待用户审批
              │     └────────┬─────────┘
              │              │ 用户响应
              │              ▼
              │     ┌──────────────┐
              │     │  Executing   │ ← AI 执行操作
              │     └──────┬───────┘
              │            │ 执行完成
              │            ▼
              │     ┌───────────┐
              └─────│   Idle    │ ← 回到空闲
                    └───────────┘

其他状态转换:
  Any State ──→ Interrupted (Ctrl+C)
  Any State ──→ Suspended (^Z)
  Any State ──→ Error (异常)
```

---

## 2. 输入状态机 (Input State Machine)

### 2.1 状态定义

输入状态机管理 ChatComposer 的输入状态，包含 4 种状态：

```rust
pub enum InputState {
    /// 输入框为空
    Empty,
    /// 正在编辑
    Editing,
    /// 弹出菜单活跃
    PopupActive,
    /// 提交中
    Submitting,
}
```

### 2.2 状态详细描述

#### Empty (空状态)

**进入条件**：
- 系统启动
- 提交完成后清空输入框
- 用户清空输入框

**退出条件**：
- 用户输入字符 → Editing
- 用户触发 slash 命令 → PopupActive

**允许的交互**：
- 输入字符
- 查看历史记录
- 触发命令模式

**视觉表现**：
- 输入框显示提示符 "> "
- 光标闪烁
- 显示帮助信息 "[Ctrl+Enter 提交]"

**参考实现**：Codex `ChatComposer` 初始状态

#### Editing (编辑状态)

**进入条件**：
- Empty 状态下用户输入字符
- PopupActive 状态下用户取消弹出

**退出条件**：
- 用户提交 → Submitting
- 用户触发弹出菜单 → PopupActive
- 用户清空输入 → Empty

**允许的交互**：
- 所有文本编辑功能
- 光标移动
- 历史记录浏览
- 命令触发

**视觉表现**：
- 输入框显示当前内容
- 光标位置可见
- 可能显示字符计数
- 显示帮助信息

**参考实现**：Codex `ChatComposer` 正常编辑状态

#### PopupActive (弹出活跃状态)

**进入条件**：
- Editing 状态下用户触发 `/`（slash 命令）
- Editing 状态下用户触发 `@`（mention）
- Editing 状态下用户触发文件搜索

**退出条件**：
- 用户选择菜单项 → Editing
- 用户取消 → Editing
- 用户提交 → Submitting（如果菜单支持）

**允许的交互**：
- 菜单导航（↑/↓）
- 搜索过滤
- 选择确认
- 取消操作

**视觉表现**：
- 输入框上方显示弹出菜单
- 当前选择项高亮
- 可能显示过滤提示
- 主输入框可能部分被覆盖

**参考实现**：Codex `CommandPopup`、`FileSearchPopup` 等弹出状态

#### Submitting (提交状态)

**进入条件**：
- Editing 状态下用户按 Enter 提交

**退出条件**：
- 提交完成 → Empty
- 提交失败 → Editing

**允许的交互**：
- 仅允许 Ctrl+C 中断
- 其他交互被禁用

**视觉表现**：
- 输入框内容保留但不可编辑
- 输入框灰色显示
- 光标隐藏
- 显示提交状态

**参考实现**：Codex 提交到 Agent 时的状态

### 2.3 状态转换图

```
                    ┌───────────┐
                    │   Empty   │ ← 输入框为空
                    └─────┬─────┘
                          │ 用户输入字符
                          ▼
                    ┌───────────┐
              ┌─────│  Editing  │ ← 正在编辑
              │     └─────┬─────┘
              │           │ 用户输入 /
              │           ▼
              │     ┌──────────────┐
              │     │ PopupActive  │ ← 弹出菜单活跃
              │     └──────┬───────┘
              │            │ 选择/取消
              │            ▼
              │     ┌───────────┐
              └─────│  Editing  │
                    └─────┬─────┘
                          │ 用户按 Enter (提交)
                          ▼
                    ┌──────────────┐
                    │  Submitting  │ ← 提交中
                    └──────┬───────┘
                           │ 提交完成
                           ▼
                    ┌───────────┐
                    │   Empty   │ ← 清空输入框
                    └───────────┘
```

---

## 3. 状态转换表

### 3.1 完整转换矩阵

| 当前状态 | 事件 | 下一状态 | 副作用 | 优先级 |
|----------|------|----------|--------|--------|
| **Idle** | 用户提交 | Submitting | 禁用输入框，启动 spinner | 高 |
| Idle | 配置修改 | Idle | 更新配置，重新渲染 | 中 |
| Idle | 会话切换 | Idle | 切换会话上下文 | 中 |
| **Submitting** | 提交完成 | Waiting | 发送请求到 Agent，显示思考 spinner | 高 |
| Submitting | 提交失败 | Idle | 显示错误信息，恢复输入框 | 高 |
| Submitting | Ctrl+C | Idle | 取消提交，恢复输入框 | 最高 |
| **Waiting** | AI 开始输出 | Streaming | 开始流式输出显示 | 高 |
| Waiting | AI 请求审批 | WaitingApproval | push ApprovalOverlay | 高 |
| Waiting | AI 错误 | Idle | 显示错误信息 | 高 |
| Waiting | 超时 | Idle | 显示超时错误 | 高 |
| Waiting | Ctrl+C | Interrupted | 中断 AI 连接 | 最高 |
| **Streaming** | AI 请求审批 | WaitingApproval | 暂停输出，push ApprovalOverlay | 高 |
| Streaming | AI 完成 | Idle | 完成当前消息，发送通知 | 高 |
| Streaming | AI 错误 | Idle | 显示错误，清理部分输出 | 高 |
| Streaming | Ctrl+C | Interrupted | 中断流式输出 | 最高 |
| Streaming | 网络错误 | Idle | 显示网络错误，尝试重连 | 高 |
| **WaitingApproval** | 用户同意 | Executing | 通知 AI 继续，pop ApprovalOverlay | 高 |
| WaitingApproval | 用户拒绝 | Idle | 通知 AI 取消，pop ApprovalOverlay | 高 |
| WaitingApproval | 用户查看详情 | WaitingApproval | push DiffView，保持 ApprovalOverlay | 中 |
| WaitingApproval | Ctrl+C | Interrupted | 中断审批流程 | 最高 |
| **Executing** | 执行完成 | Idle | 显示执行结果，回到空闲 | 高 |
| Executing | 执行失败 | Idle | 显示失败原因 | 高 |
| Executing | 需要进一步审批 | WaitingApproval | push 新的 ApprovalOverlay | 高 |
| Executing | Ctrl+C | Interrupted | 中断执行 | 最高 |
| **Interrupted** | 清理完成 | Idle | 恢复正常状态 | 高 |
| Interrupted | 恢复操作 | Streaming/Executing | 尝试恢复被中断的操作 | 中 |
| **Any State** | ^Z | Suspended | 暂停事件流，恢复终端模式 | 最高 |
| Any State | ^Z (恢复) | 之前状态 | 恢复事件流，继续交互 | 最高 |
| Any State | 窗口 Resize | 当前状态 | 重新计算布局，可能触发重绘 | 高 |
| Any State | 终端失焦 | 当前状态 | 更新 terminal_focused 状态 | 低 |
| Any State | 终端获焦 | 当前状态 | 更新 terminal_focused 状态 | 低 |

### 3.2 非法转换检测

系统会检测并拒绝以下非法状态转换：

| 当前状态 | 非法事件 | 处理方式 |
|----------|----------|----------|
| Submitting | 用户提交 | 忽略（已在提交中） |
| Waiting | 用户提交 | 忽略（等待 AI 响应） |
| Streaming | 用户提交 | 忽略（AI 正在输出） |
| WaitingApproval | 用户提交 | 忽略（等待审批） |
| Executing | 用户提交 | 忽略（执行中） |
| Interrupted | 用户提交 | 忽略（中断处理中） |
| Submitting | AI 事件 | 忽略（还未到 AI 阶段） |

**检测机制**：
```rust
impl AppState {
    pub fn can_submit(&self) -> bool {
        matches!(self, AppState::Idle)
    }
    
    pub fn can_interrupt(&self) -> bool {
        !matches!(self, AppState::Idle | AppState::Interrupted)
    }
}
```

### 3.3 转换规则优先级

1. **最高优先级**：系统中断（Ctrl+C, ^Z）
2. **高优先级**：状态关键转换（用户操作、AI 事件）
3. **中优先级**：配置修改、UI 更新
4. **低优先级**：后台更新、统计信息

---

## 4. 中断处理 (Interrupt Handling)

### 4.1 三级中断机制

Loom TUI 采用三级中断机制，提供不同强度的中断能力：

```rust
pub enum InterruptLevel {
    /// 软中断：等待当前 token 完成后停止
    Soft,
    /// 硬中断：立即断开连接
    Hard,
    /// 取消中断：仅 pop 视图，不中断 AI
    Cancel,
}
```

#### Soft Interrupt (软中断)

**触发条件**：
- Streaming 状态下的 Ctrl+C
- 首次 Ctrl+C（如果未在 2 秒内再次按下）

**处理方式**：
- 设置 cancel_flag = true
- 等待 AI 完成当前 token
- 优雅停止流式输出
- 转换到 Interrupted 状态

**实现参考**：Codex `ThreadSession.cancel_flag: Arc<AtomicBool>`

**用户体验**：
- 显示 "正在中断..."
- 完成当前 token 后停止
- 保留已输出的内容

#### Hard Interrupt (硬中断)

**触发条件**：
- 2 秒内连续两次 Ctrl+C
- 等待/执行状态下的长时间无响应

**处理方式**：
- 立即断开与 AI 的连接
- 强制终止 Agent 进程
- 清理所有 pending 操作
- 转换到 Interrupted 状态

**实现参考**：Codex `RunCancellation` 机制

**用户体验**：
- 立即停止
- 可能丢失部分输出
- 显示强制中断提示

#### Cancel Interrupt (取消中断)

**触发条件**：
- 弹出视图（如 ApprovalOverlay）内的 Ctrl+C
- 用户按 Esc 取消当前操作

**处理方式**：
- 仅 pop 当前视图
- 不中断 AI 连接
- 返回到之前状态

**用户体验**：
- 平滑取消当前交互
- 保持 AI 连接状态
- 返回正常输入状态

### 4.2 Ctrl+C 双击强制退出

```
用户按 Ctrl+C
  │
  ├── 2 秒内未再次按下
  │   └── Soft Interrupt
  │       ├── 设置 cancel_flag
  │       ├── 等待当前 token 完成
  │       └── 优雅停止
  │
  └── 2 秒内再次按下
      └── Hard Interrupt
          ├── 立即断开连接
          ├── 强制终止进程
          └── 清理所有状态
```

**时间窗口**：2 秒

**视觉反馈**：
- 第一次 Ctrl+C：显示 "中断中... (再次 Ctrl+C 强制退出)"
- 第二次 Ctrl+C：显示 "强制退出中..."

### 4.3 中断恢复流程

```rust
中断恢复:
  1. 清理未完成的流式输出
  2. 将状态切换为 Interrupted
  3. 显示中断提示信息
  4. 清理 UI 状态（spinner、弹窗等）
  5. 回到 Idle 状态
  
支持恢复的中断:
  - Soft Interrupt: 可恢复（如果 AI 支持断点续传）
  - 硬中断: 不可恢复
  - Cancel Interrupt: 无需恢复（仅取消视图）
```

**参考实现**：Codex `agent.rs` 的 `turn/cancel` 路由处理 + `cancel_flag: AtomicBool` 机制

### 4.4 中断状态下的行为

| 中断类型 | 视觉表现 | 用户可操作 | AI 连接 | 数据保留 |
|----------|----------|------------|---------|----------|
| Soft Interrupt | 警告色提示 | 等待清理完成 | 优雅断开 | 保留已输出 |
| Hard Interrupt | 红色警告提示 | 立即回到空闲 | 强制断开 | 可能丢失 |
| Cancel Interrupt | 正常提示 | 立即返回输入 | 保持连接 | 完全保留 |

---

## 5. 状态一致性 (State Consistency)

### 5.1 单一状态来源原则

Loom TUI 采用**单一状态来源（Single Source of Truth）**设计：

```rust
pub struct App {
    /// 应用状态机状态（唯一状态来源）
    app_state: AppState,
    
    /// 视图栈状态
    pane_stack: PaneStack,
    
    /// 输入状态
    chat_composer: ChatComposer,
    
    /// 对话历史
    history: Vec<HistoryCell>,
    
    /// 终端焦点状态（跨线程共享）
    terminal_focused: Arc<AtomicBool>,
    
    /// 取消标志（跨线程共享）
    cancel_flag: Arc<AtomicBool>,
}
```

**状态更新规则**：
1. 所有状态变更必须通过 App 的方法
2. 状态更新原子性操作
3. 每次状态更新触发重新渲染
4. 不允许直接修改状态字段

### 5.2 状态同步机制

```rust
状态更新流程:
  1. 事件处理 → App::handle_event()
  2. App::handle_event() → 更新 App 内部状态
  3. 状态更新 → App::set_state(new_state)
  4. App::set_state() → 触发 render()
  5. render() → 从最新状态生成 UI
  6. UI 反映最新状态

状态同步保证:
  - 所有状态变更通过 App 方法
  - 渲染始终从最新状态生成
  - 无并发状态修改（单线程事件循环）
  - 跨线程状态使用 Arc<AtomicBool>
```

### 5.3 并发安全

**单线程事件循环**：
- TUI 事件在主线程中处理
- 状态更新顺序保证
- 无需复杂锁机制

**跨线程状态**：
```rust
// 终端焦点状态（跨线程）
terminal_focused: Arc<AtomicBool>

// 取消标志（跨线程）
cancel_flag: Arc<AtomicBool>

// Agent 事件通道
agent_event_rx: mpsc::Receiver<AgentEvent>
```

**线程安全保证**：
- 只读状态使用 Arc<T>
- 可变状态仅主线程访问
- 跨线程通信使用 channel

**参考实现**：Codex `ThreadSession { cancel_flag: Arc<AtomicBool> }`

### 5.4 状态验证机制

```rust
impl AppState {
    /// 验证状态转换的合法性
    pub fn can_transition_to(&self, target: &AppState) -> bool {
        match (self, target) {
            (AppState::Idle, AppState::Submitting) => true,
            (AppState::Submitting, AppState::Waiting) => true,
            (AppState::Waiting, AppState::Streaming | AppState::WaitingApproval) => true,
            (AppState::Streaming, AppState::WaitingApproval | AppState::Idle) => true,
            (AppState::WaitingApproval, AppState::Executing | AppState::Idle) => true,
            (AppState::Executing, AppState::Idle) => true,
            (AppState::Interrupted, AppState::Idle) => true,
            // 任意状态都可以被中断
            (_, AppState::Interrupted) => true,
            _ => false,
        }
    }
}
```

---

## 6. 对抗性验证 (Adversarial Validation)

### 6.1 边缘情况处理

#### 快速连续状态转换

**场景**：用户快速连续提交多次输入

**处理方式**：
```rust
pub struct App {
    /// 防止重复提交的锁
    submit_lock: Arc<AtomicBool>,
}

impl App {
    pub fn submit(&mut self) -> Result<(), AppError> {
        // 检查是否已在提交中
        if self.submit_lock.swap(true, Ordering::SeqCst) {
            return Err(AppError::AlreadySubmitting);
        }
        
        // 执行提交逻辑
        let result = self.do_submit();
        
        // 释放锁
        self.submit_lock.store(false, Ordering::SeqCst);
        
        result
    }
}
```

**测试用例**：
- 用户在 100ms 内连续按 10 次 Enter
- 用户在 Streaming 状态下连续按 Ctrl+C
- 用户在 WaitingApproval 状态下快速切换 Y/N/A

#### 非法状态转换

**场景**：尝试进行不合法的状态转换

**处理方式**：
```rust
impl App {
    pub fn transition_to(&mut self, target: AppState) -> Result<(), StateError> {
        if !self.app_state.can_transition_to(&target) {
            return Err(StateError::InvalidTransition {
                from: self.app_state.clone(),
                to: target,
            });
        }
        
        self.app_state = target;
        self.render();
        Ok(())
    }
}
```

**测试用例**：
- Submitting 状态下尝试提交
- Idle 状态下尝试中断
- Streaming 状态下直接跳到 Executing

### 6.2 失败模式分析

#### 状态卡死

**风险场景**：
- AI 长时间无响应，状态停留在 Waiting
- 网络断开但未检测到，状态停留在 Streaming
- 审批弹窗异常，状态停留在 WaitingApproval

**缓解措施**：
```rust
pub struct AppStateMonitor {
    /// 状态超时检测
    state_timeouts: HashMap<AppState, Duration>,
    /// 当前状态进入时间
    state_enter_time: Instant,
}

impl AppStateMonitor {
    pub fn check_timeout(&self, current_state: AppState) -> Option<Duration> {
        let timeout = self.state_timeouts.get(&current_state)?;
        let elapsed = self.state_enter_time.elapsed();
        
        if elapsed > *timeout {
            Some(elapsed - *timeout)
        } else {
            None
        }
    }
}
```

**默认超时设置**：
- Submitting: 30 秒
- Waiting: 60 秒
- Streaming: 120 秒
- WaitingApproval: 无限制（等待用户）
- Executing: 300 秒

#### 状态丢失

**风险场景**：
- 渲染失败但状态已更新
- 事件处理异常导致状态不一致
- 跨线程状态同步失败

**缓解措施**：
```rust
pub struct StateBackup {
    /// 状态历史
    history: VecDeque<AppState>,
    /// 最大历史长度
    max_history: usize,
}

impl StateBackup {
    pub fn backup(&mut self, state: AppState) {
        self.history.push_back(state);
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }
    
    pub fn rollback(&mut self) -> Option<AppState> {
        self.history.pop_back()
    }
}
```

#### 状态不一致

**风险场景**：
- UI 显示与内部状态不匹配
- 多个组件持有冲突的状态副本
- 事件处理顺序错误

**缓解措施**：
```rust
pub struct StateValidator {
    /// 状态验证规则
    validators: Vec<Box<dyn StateValidationRule>>,
}

pub trait StateValidationRule {
    fn validate(&self, app: &App) -> Result<(), ValidationError>;
}

// 示例：输入框状态验证
struct InputStateValidator;
impl StateValidationRule for InputStateValidator {
    fn validate(&self, app: &App) -> Result<(), ValidationError> {
        match app.app_state {
            AppState::Idle => {
                if app.chat_composer.is_disabled() {
                    return Err(ValidationError::InputDisabledInIdle);
                }
            }
            AppState::Submitting | AppState::Waiting | AppState::Streaming => {
                if !app.chat_composer.is_disabled() {
                    return Err(ValidationError::InputEnabledInProcessing);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
```

### 6.3 攻击面分析

#### 恶意状态注入

**风险场景**：
- Agent 返回恶意构造的状态事件
- 外部进程修改共享状态
- 配置文件中的恶意状态值

**防御措施**：
```rust
pub enum AppStateEvent {
    TurnStart,
    TurnComplete,
    TurnCancel,
    ApprovalRequired,
    // ... 其他合法事件
}

impl AppStateEvent {
    /// 从 Agent 事件验证状态转换
    pub fn validate_transition(&self, current: AppState) -> Option<AppState> {
        match (self, current) {
            (AppStateEvent::TurnStart, AppState::Idle) => Some(AppState::Submitting),
            (AppStateEvent::TurnComplete, AppState::Streaming) => Some(AppState::Idle),
            (AppStateEvent::TurnCancel, _) => Some(AppState::Interrupted),
            _ => None, // 非法转换
        }
    }
}
```

#### 竞态条件攻击

**风险场景**：
- 快速按键导致状态转换冲突
- 多个事件同时到达
- 跨线程状态竞争

**防御措施**：
```rust
pub struct EventQueue {
    /// 事件队列
    queue: VecDeque<TuiEvent>,
    /// 处理锁
    processing_lock: Arc<AtomicBool>,
}

impl EventQueue {
    pub fn push(&mut self, event: TuiEvent) -> Result<(), QueueError> {
        if self.processing_lock.load(Ordering::SeqCst) {
            return Err(QueueError::ProcessingInProgress);
        }
        self.queue.push_back(event);
        Ok(())
    }
    
    pub fn process_next<F>(&mut self, handler: F) -> Option<TuiEvent>
    where
        F: FnOnce(TuiEvent),
    {
        self.processing_lock.store(true, Ordering::SeqCst);
        let event = self.queue.pop_front();
        if let Some(ref ev) = event {
            handler(ev.clone());
        }
        self.processing_lock.store(false, Ordering::SeqCst);
        event
    }
}
```

### 6.4 设计权衡

#### 有限状态机 vs 状态模式

**有限状态机（FSM）**：
- ✅ 优势：状态明确，转换规则清晰，易于验证
- ❌ 劣势：状态扩展需要修改核心逻辑

**状态模式**：
- ✅ 优势：状态行为封装，易于扩展新状态
- ❌ 劣势：状态转换逻辑分散，难以全局验证

**Loom TUI 选择**：**有限状态机**
- 理由：状态数量有限（9种），转换规则明确，便于测试和验证

#### 事件溯源 vs 当前状态

**事件溯源**：
- ✅ 优势：完整历史，可回放，易于调试
- ❌ 劣势：复杂度高，存储开销大

**当前状态**：
- ✅ 优势：实现简单，性能好
- ❌ 劣势：历史丢失，难以回溯

**Loom TUI 选择**：**当前状态 + 有限历史**
- 理由：TUI 场景下当前状态足够，保留有限历史用于回滚和调试

#### 集中式状态 vs 分布式状态

**集中式状态**：
- ✅ 优势：一致性好，管理简单
- ❌ 劣势：单点故障，扩展性差

**分布式状态**：
- ✅ 优势：扩展性好，容错性强
- ❌ 劣势：一致性复杂，同步开销大

**Loom TUI 选择**：**集中式状态**
- 理由：TUI 单进程场景，集中式状态足够且简单

---

## 7. 实现参考与最佳实践

### 7.1 Codex 实现参考

#### Agent 状态管理

**文件**：`experimental/codex/src/agent.rs`

**关键机制**：
```rust
struct ThreadSession {
    thread_id: String,
    model: String,
    working_dir: PathBuf,
    system_prompt: Option<String>,
    cancel_flag: Arc<AtomicBool>,  // 取消标志
}

// 状态转换：turn/start → turn/completed → turn/failed
async fn handle_turn_start(&self, id: serde_json::Value, params: serde_json::Value) {
    // 设置 cancel_flag
    let cancel_flag = session.cancel_flag.clone();
    
    // 运行 agent，支持取消
    let result = run_agent_with_cancellation(config, cancel_flag).await;
    
    // 发送完成事件
    match result {
        Ok(_) => emit_turn_completed(),
        Err(_) => emit_turn_failed(),
    }
}
```

#### ItemTracker 状态跟踪

**文件**：`experimental/codex/src/event_bridge.rs`

**关键机制**：
```rust
/// Tracks sequential item IDs and the current streaming message item.
pub struct ItemTracker {
    counter: usize,
    current_message_id: Option<String>,
    /// Maps tool call_id → item_id so ToolStart/ToolOutput/ToolEnd can reference the same item.
    tool_item_ids: HashMap<String, String>,
    /// Accumulated output per tool item, keyed by item_id.
    tool_output: HashMap<String, String>,
    /// Command string per tool item (needed when emitting ToolEnd).
    tool_command: HashMap<String, String>,
    /// MCP tool metadata: item_id → (server, tool_name, arguments).
    mcp_meta: HashMap<String, (String, String, serde_json::Value)>,
}
```

**状态转换示例**（简化自 `convert_stream_event_inner`）：
```rust
// 实际代码通过 match StreamEventKind 处理以下事件：
fn convert_stream_event_inner(
    ev_kind: StreamEventKind,
    tracker: &mut ItemTracker,
    _approval: &Arc<ApprovalManager>,
) -> Vec<CodexEvent> {
    match ev_kind {
        StreamEventKind::TextDelta { content } => {
            // 文本增量：首次创建 ItemStarted，后续 ItemUpdated
        }
        StreamEventKind::ReasoningDelta { content } => {
            // 推理增量：一次性 ItemStarted + ItemCompleted
        }
        StreamEventKind::ToolCall { call_id, name, arguments } => {
            // 工具调用：区分 shell 命令、MCP 工具、未知工具
            // 分别调用 command_execution_item 或 mcp_tool_call_item
        }
        StreamEventKind::ToolOutput { .. } => {
            // 工具输出：ItemUpdated
        }
        StreamEventKind::ToolEnd { .. } => {
            // 工具结束：ItemCompleted，包含输出和状态
        }
        StreamEventKind::Finish { .. } => {
            // 转结束：关闭消息
        }
    }
}
```

#### EventState 状态管理（推断）

**注意**：Codex 代码中不存在 `EventState` 枚举。以下为基于文档描述的推断设计，描述状态转换的概念：

```rust
// 推断的状态管理概念，非实际代码
pub enum EventState {
    Turn,           // turn 开始
    ReplyStarted,   // AI 开始回复
    InThinking,     // AI 思考/执行中
    PendingToolCalls, // 等待工具调用
}
```

**实际代码**中，Codex 通过 `agent.rs` 中的 `cancel_flag: AtomicBool` 和 `ThreadSession` 的流式状态管理 Turn 生命周期，而非显式的 EventState 枚举。

### 7.2 状态机最佳实践

#### 状态设计原则

1. **状态互斥**：任意时刻只能处于一种状态
2. **状态完备**：覆盖所有可能的交互场景
3. **转换明确**：每个状态转换都有明确的触发条件
4. **可观测性**：每个状态都有清晰的视觉表现

#### 转换设计原则

1. **合法性验证**：拒绝非法的状态转换
2. **原子性**：状态转换是原子操作
3. **可逆性**：关键状态转换支持回滚
4. **幂等性**：重复执行相同转换不产生副作用

#### 测试策略

```rust
#[cfg(test)]
mod state_machine_tests {
    use super::*;
    
    #[test]
    fn test_valid_transitions() {
        let mut app = App::new();
        
        // Idle → Submitting
        assert!(app.transition_to(AppState::Submitting).is_ok());
        assert_eq!(app.app_state, AppState::Submitting);
    }
    
    #[test]
    fn test_invalid_transitions() {
        let mut app = App::new();
        
        // Streaming 状态下不能提交
        app.app_state = AppState::Streaming;
        assert!(!app.can_submit());
    }
    
    #[test]
    fn test_interrupt_from_any_state() {
        let states = vec![
            AppState::Idle,
            AppState::Submitting,
            AppState::Waiting,
            AppState::Streaming,
            AppState::WaitingApproval,
            AppState::Executing,
        ];
        
        for state in states {
            let mut app = App::new();
            app.app_state = state.clone();
            
            // 任意状态都可以被中断
            assert!(app.transition_to(AppState::Interrupted).is_ok());
        }
    }
}
```

---

## 8. 总结

Loom TUI 的状态机设计基于以下核心原则：

1. **确定性**：所有状态转换都有明确的规则和验证机制
2. **可观测性**：每个状态都有清晰的视觉表现和用户反馈
3. **可恢复性**：支持中断、暂停、恢复等操作
4. **可扩展性**：易于添加新状态和转换规则
5. **鲁棒性**：通过对抗性验证确保系统稳定

**关键设计决策**：
- 采用有限状态机而非状态模式
- 单一状态来源原则
- 三级中断机制
- 集中式状态管理
- 当前状态 + 有限历史的混合方案

**实现参考**：
- Codex CLI 的 `ThreadSession` 和 `cancel_flag` 机制
- Codex 的 `ItemTracker` 状态跟踪（`event_bridge.rs:12-24`）
- Codex 的 `EventState` 概念推断

状态机是 Loom TUI 交互架构的核心，确保用户交互行为的可预测性和系统的稳定性。