# Loom TUI Chat Composer 与输入系统交互文档

## 概述

ChatComposer 是 Loom TUI 交互层的核心组件，作为 PaneStack 的基座视图始终存在，负责所有用户输入的处理、文本编辑、历史管理和命令交互。它是用户与 AI 助手进行对话的主要界面，提供完整的行编辑能力、历史导航、Slash 命令系统和高级输入特性。

ChatComposer 实现了 `PaneView` 和 `Renderable` trait，是所有交互功能的入口点。

PaneView trait 完整定义（与 `view-system.md` 保持一致）：

```rust
pub trait PaneView: Renderable {
    /// 处理按键事件
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    /// 视图是否已完成
    fn is_complete(&self) -> bool;
    /// Ctrl+C 处理
    fn on_ctrl_c(&mut self) -> CtrlCAction;
    /// 视图标识符
    fn view_id(&self) -> Option<&'static str>;
}

pub enum Handled { Handled, NotHandled }
pub enum CtrlCAction { NotHandled, Handled, Cancel }
```

---

## 1. ChatComposer 架构

### 1.1 组件结构

```
ChatComposer 核心结构体:
├── input_buffer: String           // 当前输入缓冲区
├── cursor_position: usize         // 光标位置（字节索引）
├── selection_range: Option<Range> // 文本选择范围
├── history: ChatComposerHistory   // 输入历史管理
├── multiline_mode: bool           // 多行模式状态
├── command_mode: bool             // Slash 命令模式
└── prompt: String                 // 提示符文本
```

### 1.2 在 PaneStack 中的地位

```rust
PaneStack 结构:
[基座] ChatComposer          ← 始终存在，不可移除，永不完成
[临时] ApprovalOverlay       ← AI 请求审批时 push，完成后 pop
[临时] ListSelectionView     ← 需要用户选择时 push，完成后 pop
[临时] CommandPopup          ← Slash 命令时 push，完成后 pop
```

**关键特性：**
- **不可移除**：作为基座视图，不能被 pop
- **永不完成**：`is_complete()` 始终返回 `false`
- **事件优先**：其他视图 pop 后，事件自动回到 ChatComposer

### 1.3 渲染组件

```
ChatComposer 渲染区域:
┌──────────────────────────────────────────────────────┐
│ > 帮我优化这个 Rust 函数_                             │  ← 输入框
│ [Ctrl+Enter 提]  [Shift+Enter 换]  [/ 命令] [↑↓ 历史]│  ← 提示栏
│ [142 chars]                                          │  ← 字符数显示
└──────────────────────────────────────────────────────┘

渲染元素:
- 输入框: 显示 input_buffer 内容，光标位置高亮
- 提示符: ">" 显示在最左侧，当前状态提示
- 占位符: 空输入时显示 "输入你的问题或使用 / 查看命令"
- 字符数: 右下角显示当前输入长度
- 快捷键提示: 底部显示常用快捷键
```

### 1.4 状态机

```
ChatComposer 状态转换:

┌─────────────┐
│   Empty     │ ← 输入框为空状态
└──────┬──────┘
       │ 用户输入字符
       ▼
┌─────────────┐
│   Editing   │ ← 正在编辑状态
└──────┬──────┘
       │ 输入 "/" 或 Ctrl+R
       ▼
┌─────────────┐
│PopupActive  │ ← 弹出菜单活跃状态
└──────┬──────┘
       │ 选择/取消
       ▼
┌─────────────┐
│   Editing   │ ← 回到编辑状态
└──────┬──────┘
       │ 用户按 Enter (提交)
       ▼
┌─────────────┐
│ Submitting  │ ← 提交中状态
└──────┬──────┘
       │ 提交完成
       ▼
┌─────────────┐
│   Empty     │ ← 清空输入框，回到空闲
└─────────────┘
```

---

## 2. 文本编辑能力

### 2.1 基础字符编辑

| 按键 | 功能 | 实现细节 | 边缘情况 |
|------|------|----------|----------|
| `Printable` | 插入字符 | 在光标位置插入字符，光标右移 | Unicode 处理，正确处理多字节字符 |
| `Backspace` | 删除前一个字符 | 删除光标左侧字符，光标左移 | 行首无操作，Unicode 安全 |
| `Delete` | 删除当前字符 | 删除光标位置的字符，保持光标不变 | 行尾无操作 |
| `Enter` | 提交输入 | 触发 `App::submit()`，清空缓冲区 | 空输入提交被阻止，显示提示 |

### 2.2 光标移动

| 按键 | 功能 | 实现细节 | 边缘情况 |
|------|------|----------|----------|
| `←/→` | 单字符移动 | 按字符移动光标，跨越 Unicode 边界 | 行首/行尾边界 |
| `Ctrl+←/→` | 按词移动 | 识别词边界（空格、标点），跳到词首/词尾 | UTF-8 词边界检测 |
| `Ctrl+A` | 到行首 | 光标移动到输入框第一个字符 | 跳过提示符 |
| `Ctrl+E` | 到行尾 | 光标移动到输入框末尾 | 边界检查 |
| `Home` | 到行首 | 同 Ctrl+A | 键盘兼容性 |
| `End` | 到行尾 | 同 Ctrl+E | 键盘兼容性 |

### 2.3 删除操作

| 按键 | 功能 | 实现细节 | 边缘情况 |
|------|------|----------|----------|
| `Ctrl+K` | 删除到行尾 | 删除光标到缓冲区末尾所有内容 | 存入剪贴板缓冲区 |
| `Ctrl+U` | 删除到行首 | 删除缓冲区开始到光标所有内容 | 存入剪贴板缓冲区 |
| `Ctrl+W` | 删除前一个词 | 删除光标前的一个完整词 | 词边界识别 |
| `Backspace` | 删除前一个字符 | 逐字符删除 | 连按可快速删除 |

### 2.4 撤销/重做

| 按键 | 功能 | 实现细节 | 容量限制 |
|------|------|----------|----------|
| `Ctrl+Z` | 撤销 | 恢复上一个编辑状态，从 undo_stack pop | 最大 100 步 |
| `Ctrl+Shift+Z` | 重做 | 重新执行撤销的操作，从 redo_stack pop | 与 undo 同步 |
| `Ctrl+Y` | 重做 | 同 Ctrl+Shift+Z | 两种快捷键支持 |

**撤销栈结构：**
```rust
struct UndoStack {
    undo: Vec<EditorState>,   // 最多 100 个状态
    redo: Vec<EditorState>,   // 当前编辑状态清除 redo
}

struct EditorState {
    buffer: String,
    cursor: usize,
    selection: Option<Range>,
}
```

### 2.5 多行编辑

| 按键 | 功能 | 实现细节 | 视觉反馈 |
|------|------|----------|----------|
| `Shift+Enter` | 换行 | 在光标位置插入 `\n`，不提交 | 输入框高度自动调整 |
| `Enter` | 提交 | 提交当前内容，清空缓冲区 | 如果多行模式，仅提交当前行 |
| `Ctrl+Enter` | 强制提交 | 无论多行状态，强制提交整个内容 | 忽略多行模式 |

**多行模式状态：**
```rust
fn is_multiline_input(text: &str) -> bool {
    text.contains('\n') || text.len() > 80
}

fn render_multiline(height: u16) -> u16 {
    if is_multiline_input(&input_buffer) {
        min(input_buffer.lines().count() as u16 + 2, height / 3)
    } else {
        3  // 单行固定高度
    }
}
```

---

## 3. 输入历史系统

### 3.1 ChatComposerHistory 架构

```rust
struct ChatComposerHistory {
    buffer: RingBuffer<String>,     // 环形缓冲区，最多 1000 条
    position: Option<usize>,        // 当前历史位置
    current_input: String,          // 编辑中的输入（未提交）
    file_path: PathBuf,             // 持久化文件路径
}

impl ChatComposerHistory {
    const MAX_SIZE: usize = 1000;   // 最大历史记录数
    const SAVE_THRESHOLD: usize = 10; // 每 10 次操作保存一次
}
```

### 3.2 历史导航

| 按键 | 功能 | 实现细节 | 边缘情况 |
|------|------|----------|----------|
| `↑` | 上一条历史 | 从最新历史向后遍历 | 首次按下时保存当前编辑 |
| `↓` | 下一条历史 | 向前历史移动 | 回到最新时恢复编辑内容 |
| `Ctrl+R` | 历史搜索 | 进入反向搜索模式 | 支持 regex 搜索 |
| `Enter` (搜索模式) | 选择历史 | 选择当前搜索结果 | 退出搜索模式 |

**历史导航算法：**
```rust
fn navigate_up(&mut self) {
    if self.position.is_none() {
        // 首次按上箭头：保存当前编辑
        self.current_input = self.composer.input_buffer.clone();
        self.position = Some(self.buffer.len());
    }
    
    if let Some(pos) = self.position {
        if pos > 0 {
            self.position = Some(pos - 1);
            self.composer.input_buffer = self.buffer[pos - 1].clone();
        }
    }
}

fn navigate_down(&mut self) {
    if let Some(pos) = self.position {
        if pos < self.buffer.len() {
            self.position = Some(pos + 1);
            if pos + 1 == self.buffer.len() {
                // 回到最新：恢复编辑内容
                self.composer.input_buffer = self.current_input.clone();
                self.position = None;
            } else {
                self.composer.input_buffer = self.buffer[pos + 1].clone();
            }
        }
    }
}
```

### 3.3 历史持久化

```rust
// 持久化流程
fn save_to_disk(&self) {
    let history_data = serde_json::to_string(&self.buffer).unwrap();
    std::fs::write(&self.file_path, history_data)?;
}

// 加载流程
fn load_from_disk(&mut self) {
    if let Ok(data) = std::fs::read_to_string(&self.file_path) {
        if let Ok(loaded) = serde_json::from_str::<Vec<String>>(&data) {
            self.buffer = RingBuffer::from(loaded);
        }
    }
}

// 文件位置
fn get_history_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("loom-tui")
        .join("input-history.json")
}
```

### 3.4 反向搜索 (Ctrl+R)

**搜索模式界面：**
```
┌──────────────────────────────────────────────────────┐
│ (reverse-i-search)`optim`: 帮我优化这个函数_         │
│ ────────────────────────────────────────────────────│
│ > _                                                  │
└──────────────────────────────────────────────────────┘
```

**搜索算法：**
```rust
fn reverse_search(&mut self, query: &str) {
    // 从当前位置开始反向搜索
    let start = self.position.unwrap_or(self.buffer.len());
    for i in (0..start).rev() {
        if self.buffer[i].contains(query) {
            self.position = Some(i);
            self.composer.input_buffer = self.buffer[i].clone();
            return;
        }
    }
    // 未找到：显示提示音效
}
```

**搜索模式按键：**
- `Ctrl+R`：继续搜索下一个匹配项
- 字符键：扩展搜索查询
- `Backspace`：删除搜索查询字符
- `Enter`：接受当前搜索结果
- `Esc`：取消搜索，回到编辑模式

---

## 4. Slash 命令系统

### 4.1 命令注册

```rust
struct CommandRegistry {
    commands: HashMap<&'static str, SlashCommand>,
}

struct SlashCommand {
    name: &'static str,
    description: &'static str,
    handler: fn(&mut App, &str) -> CommandResult,
    params: Vec<CommandParam>,
}

struct CommandParam {
    name: &'static str,
    description: &'static str,
    required: bool,
    completion: fn(&str) -> Vec<String>,
}
```

### 4.2 可用命令列表

| 命令 | 描述 | 参数 | 示例 |
|------|------|------|------|
| `/help` | 显示帮助信息 | 无 | `/help` |
| `/reset` | 重置当前会话 | 无 | `/reset` |
| `/compact` | 压缩对话历史 | [max_lines] | `/compact 50` |
| `/summarize` | 总结当前对话 | 无 | `/summarize` |
| `/models` | 列出可用模型 | 无 | `/models` |
| `/model` | 切换模型 | [model_name] | `/model gpt-4o` |
| `/tools` | 列出可用工具 | 无 | `/tools` |
| `/resume` | 恢复会话 | [session_id] | `/resume abc123` |
| `/undo` | 撤销上次操作 | 无 | `/undo` |
| `/retry` | 重试上次操作 | 无 | `/retry` |
| `/history` | 显示输入历史 | [count] | `/history 10` |
| `/exit` | 退出 TUI | 无 | `/exit` |

### 4.3 命令补全

**触发条件：**
- 用户输入 `/` 字符时自动触发
- 命令名称输入时实时过滤

**补全界面：**
```
┌──────────────────────────────────────────────────────┐
│ > /mo_                                               │
│ ┌─ 命令补全 ────────────────────────────────────────┐│
│ │ /model     切换 AI 模型                             ││
│ │ /models    列出可用模型                             ││
│ └────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────┘
```

**补全算法：**
```rust
fn complete_command(&self, prefix: &str) -> Vec<&'static str> {
    self.commands
        .keys()
        .filter(|cmd| cmd.starts_with(prefix))
        .cloned()
        .collect()
}
```

### 4.4 参数提示

**参数补全：**
```
┌──────────────────────────────────────────────────────┐
│ > /model _                                           │
│ ┌─ 参数提示 ────────────────────────────────────────┐│
│ │ model_name: 模型名称 (gpt-4o, gpt-4o-mini, o1)     ││
│ │ 可用选项:                                          ││
│ │   • gpt-4o                                         ││
│ │   • gpt-4o-mini                                   ││
│ │   • o1-mini                                       ││
│ └────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────┘
```

**参数验证：**
```rust
fn validate_params(&self, command: &str, args: &str) -> Result<(), CommandError> {
    let cmd = self.commands.get(command)
        .ok_or(CommandError::UnknownCommand)?;
    
    for param in &cmd.params {
        if param.required && !args.contains(&format!("{}:", param.name)) {
            return Err(CommandError::MissingParam(param.name));
        }
    }
    
    Ok(())
}
```

### 4.5 命令处理流程

```
用户输入 "/"
  → ChatComposer 进入命令模式
  → 命令补全弹出 CommandPopup
  → 用户选择/输入命令
  → 参数验证
  → 执行命令处理器
  → 返回 CommandResult
  → 处理结果（显示反馈/修改状态）
  → 回到正常输入模式
```

---

## 5. 粘贴处理

### 5.1 Bracketed Paste 模式

```rust
// 启用 bracketed paste
fn enable_bracketed_paste() {
    print!("\x1b[?2004h");
    std::io::stdout().flush();
}

// 禁用 bracketed paste
fn disable_bracketed_paste() {
    print!("\x1b[?2004l");
    std::io::stdout().flush();
}
```

### 5.2 粘贴内容解析

```rust
fn parse_paste_event(input: &str) -> Result<String, PasteError> {
    // Bracketed paste 格式: \x1b[200~ ... \x1b[201~
    if input.starts_with("\x1b[200~") && input.ends_with("\x1b[201~") {
        let content = &input[7..input.len()-6];
        Ok(content.to_string())
    } else {
        // 正常粘贴（可能来自终端模拟器）
        Ok(input.to_string())
    }
}
```

### 5.3 多行文本处理

```rust
fn handle_multiline_paste(&mut self, content: String) {
    let lines: Vec<&str> = content.lines().collect();
    
    if lines.len() > 1 {
        // 多行粘贴：询问用户意图
        self.show_paste_confirmation(&content);
    } else {
        // 单行粘贴：直接插入
        self.insert_at_cursor(&content);
    }
}
```

### 5.4 大文本分块处理

```rust
const CHUNK_SIZE: usize = 10_000;  // 10KB 分块

fn handle_large_paste(&mut self, content: String) {
    if content.len() > CHUNK_SIZE {
        // 显示处理进度
        self.show_paste_progress(0, content.len());
        
        // 分块处理
        for (i, chunk) in content.as_bytes().chunks(CHUNK_SIZE).enumerate() {
            let chunk_str = String::from_utf8_lossy(chunk).to_string();
            self.insert_at_cursor(&chunk_str);
            self.show_paste_progress((i + 1) * CHUNK_SIZE, content.len());
        }
    } else {
        self.insert_at_cursor(&content);
    }
}
```

### 5.5 粘贴进度显示

```
┌──────────────────────────────────────────────────────┐
│ 正在粘贴文本... [████████░░░░░░░] 45% (12,450 / 27,800) │
│                                                     │
│ > 帮我优化这个函数_                                  │
└──────────────────────────────────────────────────────┘
```

---

## 6. 外部编辑器集成

### 6.1 触发方式

| 触发方式 | 配置键 | 默认值 | 说明 |
|----------|--------|--------|------|
| `Ctrl+E` | `editor_keybinding` | `Ctrl+E` | 打开外部编辑器 |
| `Ctrl+O` | `editor_keybinding` | `Ctrl+E` | 备用快捷键 |

### 6.2 编辑器检测流程

```rust
fn detect_editor() -> String {
    // 1. 检查环境变量
    if let Ok(editor) = std::env::var("EDITOR") {
        return editor;
    }
    
    // 2. 检查 VISUAL 变量
    if let Ok(editor) = std::env::var("VISUAL") {
        return editor;
    }
    
    // 3. 检测系统默认
    if cfg!(target_os = "macos") {
        return "vim".to_string();  // macOS 默认
    } else if cfg!(target_os = "linux") {
        return "nano".to_string();  // Linux 默认
    } else {
        return "notepad".to_string();  // Windows 默认
    }
}
```

### 6.3 外部编辑器流程

```
用户按 Ctrl+E
  │
  ├── 保存当前输入状态
  ├── 创建临时文件
  ├── 将当前缓冲区内容写入临时文件
  │
  ├── 暂停 TUI 事件轮询
  ├── 恢复终端正常模式
  │
  ├── 启动外部编辑器进程
  │   └── std::process::Command::new(&editor)
  │         .arg(&temp_file_path)
  │         .spawn()
  │
  ├── 等待编辑器退出
  │   └── child.wait()
  │
  ├── 读取临时文件内容
  ├── 更新输入缓冲区
  │
  ├── 清理临时文件
  ├── 恢复终端 raw mode
  ├── 恢复 TUI 事件轮询
  │
  └── 重新渲染，显示编辑后的内容
```

### 6.4 临时文件管理

```rust
fn create_temp_file() -> std::io::Result<(PathBuf, File)> {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("loom-tui-input-{}.txt", 
                           std::process::id());
    let file_path = temp_dir.join(&file_name);
    let file = File::create(&file_path)?;
    
    Ok((file_path, file))
}

fn cleanup_temp_file(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}
```

### 6.5 编辑器崩溃处理

```rust
fn handle_editor_crash(&mut self, temp_path: &PathBuf) {
    // 1. 检查临时文件是否可读
    if let Ok(content) = std::fs::read_to_string(temp_path) {
        // 2. 文件完整：恢复内容
        self.input_buffer = content;
        self.show_notification("编辑器意外关闭，已恢复编辑内容");
    } else {
        // 3. 文件损坏：提示用户
        self.show_notification("编辑器意外关闭，部分内容可能丢失");
        // 4. 保留临时文件供检查
        let backup_path = temp_path.with_extension("txt.backup");
        let _ = std::fs::copy(temp_path, &backup_path);
    }
}
```

---

## 7. 对抗性验证

### 7.1 边缘情况处理

#### 空输入提交
```rust
fn validate_submit(&self) -> Result<(), ValidationError> {
    if self.input_buffer.trim().is_empty() {
        return Err(ValidationError::EmptyInput);
    }
    Ok(())
}
```

#### 超大输入处理
```rust
const MAX_INPUT_SIZE: usize = 100_000;  // 100KB

fn handle_large_input(&mut self, input: String) {
    if input.len() > MAX_INPUT_SIZE {
        self.show_warning(
            &format!("输入超过 {}KB，已截断。使用外部编辑器处理大文本。", 
                     MAX_INPUT_SIZE / 1024)
        );
        self.input_buffer = input.chars().take(MAX_INPUT_SIZE).collect();
    } else {
        self.input_buffer = input;
    }
}
```

#### Unicode 字符处理
```rust
fn insert_unicode_char(&mut self, c: char) {
    // 正确处理多字节字符
    let mut bytes = [0u8; 4];
    c.encode_utf8(&mut bytes);
    
    // 在字节索引处插入
    let pos = self.cursor_position;
    self.input_buffer.insert_str(pos, &c.to_string());
    self.cursor_position += c.len_utf8();
}
```

#### 控制字符过滤
```rust
fn filter_control_chars(input: &str) -> String {
    input.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
```

### 7.2 失败模式处理

#### 历史缓冲区溢出
```rust
fn handle_history_overflow(&mut self) {
    // 1. 通知用户
    self.show_warning("历史记录已满，删除最旧的记录");
    
    // 2. 删除最旧的 10% 记录
    let remove_count = self.history.len() / 10;
    for _ in 0..remove_count {
        self.history.pop_front();
    }
    
    // 3. 强制保存
    self.save_to_disk();
}
```

#### 编辑器崩溃恢复
```rust
fn recover_from_editor_crash(&mut self, temp_path: &PathBuf) -> Result<(), RecoveryError> {
    // 1. 检查文件状态
    let metadata = std::fs::metadata(temp_path)?;
    
    // 2. 验证文件完整性
    if metadata.len() > 0 {
        let content = std::fs::read_to_string(temp_path)?;
        if !content.trim().is_empty() {
            self.input_buffer = content;
            return Ok(());
        }
    }
    
    // 3. 文件损坏：尝试恢复
    Err(RecoveryError::CorruptFile)
}
```

#### 粘贴中断处理
```rust
fn handle_interrupted_paste(&mut self, partial_content: String) {
    // 1. 显示中断警告
    self.show_warning("粘贴过程中断，部分内容可能丢失");
    
    // 2. 显示已粘贴内容
    if !partial_content.is_empty() {
        self.insert_at_cursor(&partial_content);
        self.show_notification(&format!("已恢复 {} 字符", partial_content.len()));
    }
    
    // 3. 提供恢复选项
    self.show_recovery_dialog();
}
```

### 7.3 安全考量

#### 命令注入防护
```rust
fn sanitize_slash_command(&self, input: &str) -> Result<String, SecurityError> {
    if !input.starts_with('/') {
        return Ok(input.to_string());
    }
    
    // 验证命令是否在注册表中
    let cmd_name = input.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    
    if !self.commands.contains_key(cmd_name) {
        return Err(SecurityError::UnknownCommand(cmd_name.to_string()));
    }
    
    // 过滤危险参数
    if input.contains("&&") || input.contains("||") || input.contains(";") {
        return Err(SecurityError::CommandInjection);
    }
    
    Ok(input.to_string())
}
```

#### 历史记录泄露防护
```rust
fn sanitize_for_history(&self, input: &str) -> String {
    // 1. 移除敏感信息
    let sanitized = input
        .lines()
        .filter(|line| !self.contains_sensitive_info(line))
        .collect::<Vec<_>>()
        .join("\n");
    
    // 2. 截断过长内容
    if sanitized.len() > 1000 {
        format!("{}...[截断]", &sanitized[..1000])
    } else {
        sanitized
    }
}

fn contains_sensitive_info(&self, line: &str) -> bool {
    let sensitive_patterns = [
        "password", "token", "api_key", "secret", 
        "credit_card", "ssn", "private_key"
    ];
    
    sensitive_patterns.iter()
        .any(|pattern| line.to_lowercase().contains(pattern))
}
```

#### 路径遍历防护
```rust
fn safe_path_join(base: &Path, user_input: &str) -> Result<PathBuf, SecurityError> {
    let path = PathBuf::from(user_input);
    
    // 检查路径遍历
    if path.has_root() || path.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(SecurityError::PathTraversal(user_input.to_string()));
    }
    
    // 验证最终路径在 base 内
    let full_path = base.join(&path);
    if !full_path.starts_with(base.canonicalize()?) {
        return Err(SecurityError::PathTraversal(user_input.to_string()));
    }
    
    Ok(full_path)
}
```

### 7.4 设计权衡

#### 行编辑 vs 全屏编辑
| 维度 | 行编辑 | 全屏编辑 |
|------|--------|----------|
| **上下文保持** | 高 | 低 |
| **实现复杂度** | 低 | 高 |
| **大文本处理** | 差 | 好 |
| **内联体验** | 好 | 差 |
| **用户习惯** | 符合终端 | 符合 GUI |

**决策：** 采用行编辑为主，外部编辑器为辅的混合模式。

#### 同步历史 vs 异步历史
| 维度 | 同步历史 | 异步历史 |
|------|----------|----------|
| **数据一致性** | 高 | 低 |
| **响应速度** | 慢 | 快 |
| **实现复杂度** | 低 | 高 |
| **崩溃影响** | 小 | 大 |

**决策：** 采用异步批量保存策略（每 10 次操作保存一次）。

#### 内存历史 vs 磁盘历史
| 维度 | 内存历史 | 磁盘历史 |
|------|----------|----------|
| **访问速度** | 快 | 慢 |
| **容量限制** | 小 | 大 |
| **持久化** | 无 | 有 |
| **隐私安全** | 好 | 差 |

**决策：** 采用混合策略（内存 1000 条 + 磁盘 10000 条）。

---

## 8. 集成示例

### 8.1 与 Agent 后端的集成

```rust
// 提交输入到 Agent 后端
impl ChatComposer {
    fn submit_to_agent(&self) -> Result<AgentRequest, SubmitError> {
        // 1. 验证输入
        self.validate_submit()?;
        
        // 2. 处理 Slash 命令
        if let Some(cmd_result) = self.process_slash_command(&self.input_buffer) {
            return Ok(AgentRequest::Command(cmd_result));
        }
        
        // 3. 构造 Agent 请求
        let user_content = if self.is_multiline_input(&self.input_buffer) {
            UserContent::Text(self.input_buffer.clone())
        } else {
            UserContent::Text(self.input_buffer.clone())
        };
        
        // 4. 添加到历史
        self.history.add(self.input_buffer.clone());
        
        Ok(AgentRequest::Chat(user_content))
    }
}
```

### 8.2 与状态机的集成

```rust
// ChatComposer 状态转换
impl ChatComposer {
    fn transition_state(&mut self, event: InputEvent) -> Result<(), StateError> {
        match self.state {
            ChatState::Empty => {
                match event {
                    InputEvent::Char(c) => self.transition_to_editing(c),
                    InputEvent::Paste(s) => self.transition_to_editing_with_paste(s),
                    InputEvent::Slash => self.transition_to_command_mode(),
                    _ => Ok(()),
                }
            }
            ChatState::Editing => {
                match event {
                    InputEvent::Submit => self.transition_to_submitting(),
                    InputEvent::Cancel => self.transition_to_empty(),
                    InputEvent::Command => self.transition_to_popup(),
                    _ => Ok(()),
                }
            }
            ChatState::PopupActive => {
                match event {
                    InputEvent::PopupComplete => self.transition_to_editing(),
                    InputEvent::PopupCancel => self.transition_to_editing(),
                    _ => Ok(()),
                }
            }
            ChatState::Submitting => {
                match event {
                    InputEvent::SubmitComplete => self.transition_to_empty(),
                    InputEvent::SubmitFailed => self.transition_to_editing(),
                    _ => Ok(()),
                }
            }
        }
    }
}
```

---

## 9. 性能优化

### 9.1 渲染优化

```rust
// 局部渲染优化
impl Renderable for ChatComposer {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // 只重绘变化的区域
        let dirty_rect = self.calculate_dirty_rect(area);
        self.render_partial(dirty_rect, buf);
    }
}
```

### 9.2 历史搜索优化

```rust
// 索引优化搜索
struct IndexedHistory {
    history: Vec<String>,
    index: HashMap<String, Vec<usize>>,  // 倒排索引
}

impl IndexedHistory {
    fn build_index(&mut self) {
        for (i, entry) in self.history.iter().enumerate() {
            for word in entry.split_whitespace() {
                self.index.entry(word.to_lowercase())
                    .or_insert_with(Vec::new)
                    .push(i);
            }
        }
    }
    
    fn search(&self, query: &str) -> Vec<usize> {
        self.index.get(&query.to_lowercase())
            .map(|indices| indices.clone())
            .unwrap_or_default()
    }
}
```

---

## 10. 测试策略

### 10.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cursor_movement_unicode() {
        let mut composer = ChatComposer::new();
        composer.insert_str("你好世界");
        composer.move_cursor_left();  // 移动到"界"字
        assert_eq!(composer.get_cursor_char(), Some('界'));
    }
    
    #[test]
    fn test_undo_redo_chain() {
        let mut composer = ChatComposer::new();
        composer.insert_str("Hello");
        composer.insert_str(" World");
        composer.undo();
        assert_eq!(composer.get_buffer(), "Hello");
        composer.redo();
        assert_eq!(composer.get_buffer(), "Hello World");
    }
}
```

### 10.2 集成测试

```rust
#[test]
fn test_full_workflow() {
    let mut app = App::new();
    
    // 模拟用户输入
    app.handle_key(KeyEvent::from(KeyCode::Char('h')));
    app.handle_key(KeyEvent::from(KeyCode::Char('e')));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    
    // 验证状态转换
    assert_eq!(app.get_state(), AppState::Submitting);
}
```

---

## 总结

ChatComposer 是 Loom TUI 输入系统的核心，提供完整的终端内嵌式编辑体验。通过栈式视图管理、丰富的编辑能力、智能历史系统和安全的命令处理，确保用户能够高效、安全地与 AI 助手交互。

关键设计原则：
1. **体验优先**：内联视图、快捷键、自动补全提升用户体验
2. **安全可控**：输入验证、历史脱敏、命令注入防护确保安全
3. **性能稳健**：增量渲染、异步历史、索引搜索优化性能
4. **容错恢复**：异常处理、状态回滚、崩溃恢复确保稳定性

ChatComposer 为其他交互组件（如 ApprovalOverlay、ListSelectionView）提供了基础架构，是整个 Loom TUI 交互系统的重要支柱。