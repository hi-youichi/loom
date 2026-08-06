# 对抗性审查报告

## 1. 术语不一致

### 1.1 PaneView vs PaneStack

**问题发现**：
- 在 `view-system.md` 中：PaneView 被定义为 `trait`，但使用方式不统一
- 在 `architecture.md` 中：PaneStack 被定义为 `Vec<Box<dyn PaneView>>`
- 在 `chat-composer.md` 中：PaneStack 结构与 architecture.md 一致，但 PaneView trait 定义不同

**具体差异**：
```rust
// view-system.md 中的定义
pub trait PaneView {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    fn is_complete(&self) -> bool;
}

// chat-composer.md 中的定义
pub trait PaneView {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    fn render(&mut self, area: Rect, buf: &mut Buffer);
    fn is_complete(&self) -> bool;
}
```

**建议修复方案**：
1. 统一 PaneView trait 定义，包含所有必需方法
2. 在 architecture.md 中明确说明 Renderable trait 的分离
3. 更新所有文档以使用一致的定义

### 1.2 AppState vs InputState

**问题发现**：
- `state-machines.md` 定义了 7 种 AppState：Idle, Submitting, Waiting, Streaming, WaitingApproval, Executing, Interrupted
- `chat-composer.md` 定义了 4 种 InputState：Empty, Editing, PopupActive, Submitting
- 两个状态机之间的关系和同步机制未明确说明

**具体差异**：
```rust
// state-machines.md 中的 AppState
pub enum AppState {
    Idle, Submitting, Waiting, Streaming, WaitingApproval, Executing, Interrupted
}

// chat-composer.md 中的 InputState  
pub enum InputState {
    Empty, Editing, PopupActive, Submitting
}
```

**建议修复方案**：
1. 明确两个状态机的职责边界和同步机制
2. 在 architecture.md 中添加状态机关系图
3. 说明状态转换时的协调逻辑

### 1.3 TuiEvent 事件类型

**问题发现**：
- `event-system.md` 定义了 7 种 TuiEvent：Key, Paste, Resize, FocusGained, FocusLost, Draw, Resume
- `notifications-system.md` 中提到了 `Notification::ApprovalRequest` 等通知类型
- `approval-feedback.md` 中使用了不同的通知命名规范

**建议修复方案**：
1. 统一事件命名规范
2. 明确区分 TuiEvent 和 Notification 的边界
3. 在 architecture.md 中添加完整的事件分类体系

## 2. 接口/契约冲突

### 2.1 PaneView::handle_key_event 返回值

**问题发现**：
- `event-system.md` 中定义的返回值类型为 `Handled` 枚举：`Handled, NotHandled`
- `architecture.md` 中还提到了 `CtrlCAction` 枚举：`NotHandled, Handled, Confirm`
- `state-machines.md` 中描述的中断处理使用不同的返回值语义

**具体冲突**：
```rust
// event-system.md 中的定义
pub enum Handled {
    Handled,
    NotHandled,
}

// architecture.md 中的定义  
pub enum CtrlCAction {
    NotHandled,
    Handled,
    Confirm,
}
```

**建议修复方案**：
1. 统一事件处理返回值类型体系
2. 明确区分普通事件处理和中断事件处理
3. 在 `view-system.md` 中添加完整的返回值规范

### 2.2 ApprovalManager 接口

**问题发现**：
- `approval-feedback.md` 中 ApprovalManager 使用 `oneshot::channel` 进行同步
- `event-system.md` 中提到的事件系统是异步的
- 同步审批与异步事件系统的协调机制未说明

**具体冲突**：
```rust
// approval-feedback.md 中的同步接口
pub async fn request(&self, call_id: String, command: String) -> ApprovalResult {
    let (tx, rx) = oneshot::channel();
    // 等待用户响应
    match rx.await {
        Ok(true) => ApprovalResult::Approved,
        _ => ApprovalResult::Denied,
    }
}

// event-system.md 中的异步架构
pub struct EventBroker {
    event_queue: mpsc::Sender<TuiEvent>,
}
```

**建议修复方案**：
1. 明确同步审批与异步事件系统的协调机制
2. 添加状态转换的时序图
3. 说明审批超时和异常处理流程

### 2.3 状态转换验证接口

**问题发现**：
- `state-machines.md` 中定义了 `can_transition_to` 方法进行状态转换验证
- `chat-composer.md` 中的状态转换没有明确的验证机制
- 两个状态机之间的转换协调未定义

**建议修复方案**：
1. 在 `chat-composer.md` 中添加状态转换验证
2. 定义跨状态机的转换协调机制
3. 添加状态转换的单元测试策略

## 3. 内容缺口

### 3.1 缺失的视图类型

**发现缺口**：
- `view-system.md` 提到了 `DiffView`、`CommandPopup`、`FeedbackView`
- 但这些视图的详细交互文档缺失
- `TranscriptOverlay` 在 `reference-codex.md` 中提及但文档不全

**建议补充**：
1. 创建专门的视图交互文档
2. 补充 DiffView 的差异浏览交互
3. 添加 CommandPopup 的命令选择流程
4. 完善 FeedbackView 的表单验证逻辑

### 3.2 错误处理与恢复机制

**发现缺口**：
- 各文档都提到了错误处理，但缺乏统一的错误处理策略
- 错误恢复的用户交互流程不完整
- 错误状态的视觉表现未统一

**建议补充**：
1. 创建统一的错误处理文档
2. 定义错误状态的视觉表现规范
3. 补充错误恢复的用户交互流程

### 3.3 国际化与可访问性

**发现缺口**：
- 所有文档都没有讨论国际化支持
- 没有提及可访问性特性（如屏幕阅读器支持）
- Unicode 字符处理在各文档中描述不一致

**建议补充**：
1. 添加国际化支持的架构设计
2. 定义可访问性标准和支持策略
3. 统一 Unicode 处理规范

## 4. 技术错误

### 4.1 源码推导错误

**错误1**：`event-system.md` 中声称某些设计从 Codex 源码推导，但实际源码中不存在对应实现

**具体错误**：
- `event-system.md:45` 声称 `TuiEventStream` 来自 `tui/event_stream.rs:45`，但实际源码中该文件不存在
- `event-system.md:62` 声称 bracketed paste 解析来自 `tui/event_stream.rs:62`，但该实现与 Codex 实际实现不符

**修复建议**：
1. 移除虚假的源码引用
2. 对于确实从 Codex 推导的内容，提供准确的源码位置
3. 对于原创设计，明确标注为 Loom TUI 原创设计

### 4.2 枚举定义错误

**错误2**：`state-machines.md` 中的 AppState 定义与实际使用不一致

**具体错误**：
- `state-machines.md:22-37` 定义了 AppState 枚举，但后续的状态转换表中使用了不存在的状态
- 状态转换表中提到了 `Error` 状态，但在枚举定义中不存在
- 转换表中提到了 `Suspended` 状态，但枚举中只有 `Interrupted`

**修复建议**：
1. 统一 AppState 枚举定义
2. 要么在枚举中添加缺失的状态，要么从转换表中移除不一致的状态
3. 添加状态转换的单元测试确保一致性

### 4.3 事件优先级错误

**错误3**：`event-system.md` 与 `notifications-system.md` 中的事件优先级描述不一致

**具体错误**：
- `event-system.md` 定义了 5 级事件优先级：Critical, High, Important, Normal, Low
- `notifications-system.md` 中的 Ctrl+C 处理使用了不同的优先级语义
- 两个文档中对 `Resize` 事件的优先级分类不同

**修复建议**：
1. 统一事件优先级定义
2. 在 architecture.md 中建立权威的事件优先级规范
3. 更新所有文档以使用一致的优先级分类

## 5. 重复内容

### 5.1 事件处理管道描述

**重复内容**：
- `event-system.md` 中的事件流管道图与 `architecture.md` 中的交互架构总览高度重复
- 两个文档都描述了从 crossterm 到 App 主循环的事件流，内容重叠度 > 80%

**建议合并**：
1. 在 architecture.md 中保留权威的事件流描述
2. event-system.md 中引用 architecture.md 的相关部分
3. 避免在多个文档中重复相同的基础架构描述

### 5.2 按键映射表

**重复内容**：
- `event-system.md` 和 `reference-codex.md` 都包含了详细的按键映射表
- 两个表格的内容几乎完全相同，差异 < 5%

**建议合并**：
1. 在 reference-codex.md 中保留完整的按键映射参考
2. event-system.md 中只关注按键映射的架构设计
3. 使用交叉引用避免重复

### 5.3 Codex 对比分析

**重复内容**：
- 多个文档都包含 Codex vs Loom 的对比分析
- 对比的角度和内容存在重复，如事件传输、按键映射等

**建议合并**：
1. 创建专门的对比分析文档
2. 其他文档中只引用相关的对比结论
3. 避免在每个文档中重复完整的对比表格

## 6. 修复建议

### 6.1 高优先级修复（P0）

1. **统一 PaneView trait 定义**
   - 文件：view-system.md, chat-composer.md, architecture.md
   - 修复：创建统一的接口定义文档
   - 影响：核心架构一致性

2. **修复 AppState 枚举错误**
   - 文件：state-machines.md
   - 修复：统一枚举定义与状态转换表
   - 影响：状态机实现正确性

3. **移除虚假源码引用**
   - 文件：event-system.md
   - 修复：验证所有源码引用的准确性
   - 影响：文档可信度

### 6.2 中优先级修复（P1）

4. **统一事件处理返回值**
   - 文件：event-system.md, architecture.md, state-machines.md
   - 修复：建立统一的返回值类型体系
   - 影响：接口一致性

5. **补充缺失的视图文档**
   - 文件：新建 diff-view.md, command-popup.md, feedback-view.md
   - 修复：创建完整的视图交互文档
   - 影响：文档完整性

6. **明确状态机协调机制**
   - 文件：state-machines.md, chat-composer.md
   - 修复：添加状态机之间的协调逻辑
   - 影响：系统行为一致性

### 6.3 低优先级修复（P2）

7. **移除重复内容**
   - 文件：多个文档
   - 修复：合并重复内容，建立交叉引用
   - 影响：文档维护效率

8. **补充国际化支持**
   - 文件：新建 i18n.md
   - 修复：添加国际化架构设计
   - 影响：产品国际化能力

9. **统一错误处理策略**
   - 文件：新建 error-handling.md
   - 修复：创建统一的错误处理规范
   - 影响：系统健壮性

## 7. 整体评分

### 7.1 各文档独立评分（1-10分）

| 文档 | 技术准确性 | 架构一致性 | 内容完整性 | 源码验证 | 总分 |
|------|-----------|-----------|-----------|---------|------|
| event-system.md | 6 | 7 | 8 | 5 | 6.5 |
| view-system.md | 7 | 6 | 7 | 8 | 7.0 |
| state-machines.md | 6 | 7 | 9 | 7 | 7.3 |
| approval-feedback.md | 8 | 8 | 9 | 8 | 8.3 |
| chat-composer.md | 8 | 7 | 9 | 8 | 8.0 |
| notifications-system.md | 8 | 8 | 8 | 7 | 7.8 |

### 7.2 总体评分

**总体评分：7.5/10**

**优势**：
- 内容覆盖全面，涵盖了 Loom TUI 交互系统的各个方面
- 文档结构清晰，每个文档都有明确的责任边界
- 包含对抗性验证部分，显示了设计考虑的全面性
- 大部分技术描述准确，架构设计合理

**主要问题**：
- 术语使用不一致，特别是核心接口定义
- 存在源码推导错误，影响文档可信度
- 文档间有重复内容，维护效率低
- 部分技术细节冲突，需要统一

**改进建议**：
1. 建立术语词汇表，确保跨文档一致性
2. 验证所有源码引用的准确性
3. 建立文档间的交叉引用机制，减少重复
4. 定期进行跨文档一致性检查
5. 建立文档审查流程，确保质量标准

---

**审查结论**：Loom TUI 交互文档整体质量良好，涵盖了系统设计的各个方面。主要问题集中在术语一致性和部分技术细节冲突上。通过按优先级执行修复建议，可以显著提升文档质量和实现一致性。建议在实施代码实现前，优先完成 P0 和 P1 级别的修复。