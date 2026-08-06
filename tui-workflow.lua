--------------------------------------------
-- Goal:  实现 Loom TUI Phase 2-4 全部代码
-- Arch:  8-phase pipeline
--   Phase 0: Setup (Cargo.toml + mod.rs)
--   Phase A: 渲染基础 (implement→test→review ×3)
--   Phase B: 渲染扩展 (implement→test→review ×3)
--   Phase C: 渲染管线 (implement→review ×1)
--   Phase D: 交互基础 (implement→test→review ×2)
--   Phase E: 交互组件 (implement→test→review ×3)
--   Phase F: 系统集成 (implement→test→review ×3)
--   Phase G: 最终集成 (integrate→build ×2)
--   Phase H: 对抗验证 (review×4 → fix×1)
--------------------------------------------

meta = {
  reasoning = "按依赖关系分8阶段: setup → 渲染(基础→扩展→管线) → 交互(基础→组件) → 集成 → 验证。每模块走 implement→test→review 三步，最终对抗性验证",
  phases = {
    { label = "setup",          description = "Cargo.toml + mod.rs 依赖声明" },
    { label = "render-base",    description = "Renderable trait, 状态指示, 差异渲染", dynamic = true },
    { label = "render-ext",     description = "历史Cell, 流式输出, Spinner", dynamic = true },
    { label = "render-pipe",    description = "升级渲染管线 terminal+viewport" },
    { label = "interact-base",  description = "PaneView trait, 状态机", dynamic = true },
    { label = "interact-comp",  description = "输入框, 审批弹窗, 选择列表", dynamic = true },
    { label = "integrate",      description = "Agent适配, JobControl, 通知", dynamic = true },
    { label = "final",          description = "App集成 + 编译验证" },
    { label = "adversarial",    description = "对抗性验证: 4角度审查 + 修复", dynamic = true },
  },
}

-- ════════════════════════════════════════════════════════
-- Schema 定义
-- ════════════════════════════════════════════════════════

local FILE_CREATED = {
  type = "object",
  properties = {
    changed = { type = "boolean" },
    files = { type = "array", items = { type = "string" } },
    notes = { type = "string" }
  },
  required = { "changed", "files" }
}

local VERIFY = {
  type = "object",
  properties = {
    passed = { type = "boolean" },
    errors = { type = "array", items = { type = "string" } },
    warnings = { type = "array", items = { type = "string" } },
    summary = { type = "string" }
  },
  required = { "passed" }
}

local REVIEW_SCHEMA = {
  type = "object",
  properties = {
    passed = { type = "boolean" },
    score = { type = "number" },
    issues = {
      type = "array",
      items = {
        type = "object",
        properties = {
          file = { type = "string" },
          severity = { type = "string" },
          description = { type = "string" },
          fix = { type = "string" }
        },
        required = { "file", "severity", "description" }
      }
    },
    summary = { type = "string" }
  },
  required = { "passed", "issues" }
}

-- ════════════════════════════════════════════════════════
-- 公共上下文
-- ════════════════════════════════════════════════════════

local BASE = "/Users/apple/dev/worktrees/loom/tui"
local TUI_DIR = BASE .. "/apps/cli/src/tui"
local DOCS = BASE .. "/docs/tui"
local CLI_SRC = BASE .. "/apps/cli/src"

local function ctx(extra)
  return "项目路径: " .. BASE .. "\n"
      .. "TUI 目录: " .. TUI_DIR .. "\n"
      .. "CLI 源码: " .. CLI_SRC .. "\n"
      .. "设计文档: " .. DOCS .. "\n"
      .. "Phase 1 已完成的文件: mod.rs, app.rs, event.rs, terminal.rs, viewport.rs, history.rs\n"
      .. "现有依赖: crossterm 0.29 (已启用), tokio, tokio-stream, libc\n"
      .. extra
end

-- ════════════════════════════════════════════════════════
-- 辅助函数: 模块三步流水线 (implement → test → review)
-- ════════════════════════════════════════════════════════

local function run_module_pipeline(modules, doc_ref)
  -- modules: { {id, file, task} }
  -- 返回 { impl_results, test_results, review_results }

  -- Step 1: 并行实现
  local impl_results = parallel(modules, function(m)
    return {
      name = "impl-" .. m.id,
      description = "Create " .. m.file,
      prompt = ctx(
        "任务: 创建文件 " .. TUI_DIR .. "/" .. m.file .. "\n\n"
        .. m.task .. "\n\n"
        .. "设计文档参考: " .. doc_ref .. "\n\n"
        .. "要求:\n"
        .. "- 使用 ratatui 0.29 API (不是 0.28)\n"
        .. "- crossterm 版本为 0.29\n"
        .. "- 实现完整的功能代码，包含必要的 use 导入\n"
        .. "- 文件不超过 500 行\n"
        .. "- 用 write_file 工具写入文件\n"
        .. "- 暂不写测试，后续有专门测试 agent\n"
      ),
      schema = FILE_CREATED,
      working_folder = BASE,
    }
  end)

  -- Step 2: 专写测试
  local test_results = parallel(modules, function(m)
    return {
      name = "test-" .. m.id,
      description = "Write tests for " .. m.file,
      prompt = ctx(
        "任务: 为 " .. TUI_DIR .. "/" .. m.file .. " 编写全面的单元测试\n\n"
        .. "约束:\n"
        .. "- 先 read 该文件，理解所有公开 API\n"
        .. "- 只在文件末尾追加 #[cfg(test)] mod tests，不修改已有实现代码\n"
        .. "- 必须覆盖:\n"
        .. "  1. trait 方法签名正确性 (render/desired_height/cursor_pos/cursor_style)\n"
        .. "  2. 边界条件 (空输入, 零宽度, 最大高度, 空字符串)\n"
        .. "  3. 错误路径 (无效输入, 溢出, panic 场景)\n"
        .. "  4. 状态转换正确性 (如果有状态机/枚举转换)\n"
        .. "  5. 事件处理逻辑 (如果有 handle_key_event)\n"
        .. "- 对照设计文档 " .. doc_ref .. " 验收标准\n"
        .. "- 用 edit 工具追加测试代码到文件末尾\n"
      ),
      schema = FILE_CREATED,
      working_folder = BASE,
    }
  end)

  -- Step 3: 审查
  local review_results = parallel(modules, function(m)
    return {
      name = "review-" .. m.id,
      description = "Review " .. m.file,
      prompt = ctx(
        "任务: 审查 " .. TUI_DIR .. "/" .. m.file .. " 的实现质量\n\n"
        .. "审查清单:\n"
        .. "1. read 源文件，对照设计文档 " .. doc_ref .. " 检查 API 一致性\n"
        .. "2. 运行 cargo test --features tui --lib 验证测试通过 (在 " .. BASE .. " 目录)\n"
        .. "3. 检查 trait 实现完整性 (Renderable 的4个方法 / PaneView 的4个方法)\n"
        .. "4. 检查 unwrap() 滥用、panic 风险、unsafe 代码\n"
        .. "5. 如果发现 bug 或编译错误，用 edit 工具修复\n"
        .. "6. 修复后重新运行 cargo test --features tui --lib 确认通过\n"
        .. "7. 返回审查结果\n"
      ),
      schema = REVIEW_SCHEMA,
      working_folder = BASE,
    }
  end)

  return impl_results, test_results, review_results
end

-- ════════════════════════════════════════════════════════
-- MAIN
-- ════════════════════════════════════════════════════════

function main()
  local all_results = {}

  -- ════════════════════════════════════════════════════
  -- Phase 0: Setup
  -- ════════════════════════════════════════════════════
  phase("setup")

  local setup = agent({
    name = "setup-deps",
    description = "Add ratatui dependency and update mod.rs",
    prompt = ctx(
      "任务: 更新 Cargo.toml 和 mod.rs，为 Phase 2-4 准备依赖\n\n"
      .. "1. 编辑 " .. BASE .. "/apps/cli/Cargo.toml:\n"
      .. "   - [features] tui 改为: tui = [\"crossterm\", \"crossterm/event-stream\", \"ratatui\"]\n"
      .. "   - [dependencies] 添加: ratatui = { version = \"0.29\", optional = true, features = [\"crossterm\"] }\n"
      .. "   - 注意 crossterm 已存在(version 0.29 optional), 只需加 ratatui\n\n"
      .. "2. 编辑 " .. TUI_DIR .. "/mod.rs:\n"
      .. "   - 添加 render 模块声明: pub mod render;\n"
      .. "   - 其他模块在各 Phase 创建文件时由实现 agent 自行添加\n\n"
      .. "3. 运行 cargo check --features tui 验证编译通过 (在 " .. BASE .. " 目录)\n"
    ),
    schema = FILE_CREATED,
    working_folder = BASE,
  })
  all_results.setup = setup.ok
  if not setup.ok then
    log("Phase 0 setup failed: " .. setup.status, "error")
    report({ error = "Phase 0 setup failed", detail = setup.status })
    return
  end

  -- ════════════════════════════════════════════════════
  -- Phase A: 渲染基础 (render.rs, status.rs, diff.rs)
  -- ════════════════════════════════════════════════════
  phase("render-base")

  local PHASE2_DOC = DOCS .. "/development/phase-2.md"
  local phase_a_modules = {
    {
      id = "render",
      file = "render.rs",
      task = "创建 Renderable trait + 布局组件。\n"
          .. "Renderable trait 方法:\n"
          .. "  render(&self, area: Rect, buf: &mut Buffer)\n"
          .. "  desired_height(&self, width: u16) -> u16\n"
          .. "  cursor_pos(&self, area: Rect) -> Option<(u16, u16)>  (默认 None)\n"
          .. "  cursor_style(&self, area: Rect) -> SetCursorStyle  (默认 DefaultUserShape)\n"
          .. "布局组件:\n"
          .. "  ColumnRenderable (垂直堆叠, Vec<&dyn Renderable>)\n"
          .. "  FlexRenderable (按比例分配空间)\n"
          .. "参考 phase-2.md §2.1"
    },
    {
      id = "status",
      file = "status.rs",
      task = "创建 AI 状态指示器。\n"
          .. "AiStatus 枚举: Idle / Thinking / Executing / WaitingApproval / Error\n"
          .. "StatusBar 结构体 (实现 Renderable):\n"
          .. "  持有 status: AiStatus, spinner_frame: usize, message: Option<String>\n"
          .. "  render() 显示状态文本 + spinner 动画字符\n"
          .. "  desired_height() 返回 1 (单行)\n"
          .. "复用 display/spinner.rs 的帧动画概念 (10帧 dots)\n"
          .. "参考 phase-2.md §2.4"
    },
    {
      id = "diff",
      file = "diff.rs",
      task = "创建文件差异渲染组件。\n"
          .. "DiffLineType 枚举: Context / Added / Removed / Header\n"
          .. "DiffLine 结构体: { line_type: DiffLineType, content: String }\n"
          .. "DiffView 结构体 (实现 Renderable):\n"
          .. "  持有 lines: Vec<DiffLine>, file_path: String\n"
          .. "  parse_diff(input: &str) -> DiffView 方法 (解析 unified diff)\n"
          .. "  render() 按行类型着色: Added=绿色, Removed=红色, Header=蓝色\n"
          .. "参考 phase-2.md §2.5"
    },
  }

  local a_impl, a_test, a_review = run_module_pipeline(
    phase_a_modules, PHASE2_DOC
  )
  all_results.render_base = { impl = a_impl, test = a_test, review = a_review }

  -- ════════════════════════════════════════════════════
  -- Phase B: 渲染扩展 (history_cell.rs, streaming.rs, spinner.rs)
  -- ════════════════════════════════════════════════════
  phase("render-ext")

  local phase_b_modules = {
    {
      id = "history-cell",
      file = "history_cell.rs",
      task = "创建对话历史 Cell 渲染。\n"
          .. "HistoryCell 枚举:\n"
          .. "  UserMessage { content: String, timestamp: ... }\n"
          .. "  AssistantMessage { content: String, timestamp: ... }\n"
          .. "  ToolCall { tool_name: String, args: Value, result: Option<String>, status: ToolStatus }\n"
          .. "  SystemMessage { content: String }\n"
          .. "ToolStatus 枚举: Running / Completed / Failed\n"
          .. "HistoryCell 实现 Renderable trait (每个 variant 不同渲染样式)\n"
          .. "参考 phase-2.md §2.6"
    },
    {
      id = "streaming",
      file = "streaming.rs",
      task = "创建流式输出渲染器。\n"
          .. "StreamingCell 结构体:\n"
          .. "  content: String (累积的流式内容)\n"
          .. "  append_text(&mut self, delta: &str) 增量追加\n"
          .. "  is_empty() -> bool\n"
          .. "  finish(self) -> String (转为最终内容)\n"
          .. "实现 Renderable trait, 渲染当前累积内容\n"
          .. "参考 phase-2.md §2.7"
    },
    {
      id = "spinner",
      file = "spinner.rs",
      task = "创建 TUI Spinner 动画适配器。\n"
          .. "SpinnerWidget 结构体:\n"
          .. "  frame_index: usize, frames: Vec<&'static str>\n"
          .. "  new() 使用 dots 动画帧: \"⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏\"\n"
          .. "  tick(&mut self) 推进下一帧\n"
          .. "  current_frame() -> &str\n"
          .. "实现 Renderable trait (单行渲染当前帧)\n"
          .. "参考 phase-2.md §2.8"
    },
  }

  local b_impl, b_test, b_review = run_module_pipeline(
    phase_b_modules, PHASE2_DOC
  )
  all_results.render_ext = { impl = b_impl, test = b_test, review = b_review }

  -- ════════════════════════════════════════════════════
  -- Phase C: 渲染管线升级 (串行, terminal.rs + viewport.rs)
  -- ════════════════════════════════════════════════════
  phase("render-pipe")

  local phase_c_impl = agent({
    name = "upgrade-terminal",
    description = "Upgrade terminal.rs and viewport.rs with ratatui",
    prompt = ctx(
      "任务: 升级 terminal.rs 和 viewport.rs 集成 ratatui 渲染管线\n\n"
      .. "1. 编辑 " .. TUI_DIR .. "/terminal.rs:\n"
      .. "   - 引入 ratatui::{Terminal, Frame, backend::CrosstermBackend}\n"
      .. "   - TuiTerminal 中添加 ratatui_terminal: Option<Terminal<CrosstermBackend<Stdout>>>\n"
      .. "   - 添加 draw_ratatui(&mut self, height: u16, f: impl FnOnce(&mut Frame)) 方法\n"
      .. "   - 使用 crossterm::execute!(stdout(), EnterAlternateScreen) / LeaveAlternateScreen\n"
      .. "     NO! 使用内联视图模式 — 不用 alt screen, 用 ScrollUp + draw\n"
      .. "   - 保留现有 draw() 方法兼容性\n"
      .. "   - 添加 SynchronizedUpdate 包裹避免闪烁\n\n"
      .. "2. 编辑 " .. TUI_DIR .. "/viewport.rs:\n"
      .. "   - 确保 handle_resize 正确更新 viewport 尺寸\n"
      .. "   - 添加 size() -> (u16, u16) 方法\n"
      .. "   - 确保 viewport 起始位置正确(基于光标探测)\n\n"
      .. "3. 确认 " .. TUI_DIR .. "/mod.rs 导出 render 模块\n"
      .. "4. 运行 cargo check --features tui 验证 (在 " .. BASE .. " 目录)\n"
    ),
    schema = FILE_CREATED,
    working_folder = BASE,
  })

  local phase_c_review = agent({
    name = "review-terminal",
    description = "Review terminal.rs and viewport.rs upgrade",
    prompt = ctx(
      "任务: 审查 terminal.rs 和 viewport.rs 的 ratatui 集成\n\n"
      .. "1. read " .. TUI_DIR .. "/terminal.rs 和 viewport.rs\n"
      .. "2. 检查内联视图正确性 (不使用 alt screen)\n"
      .. "3. 检查 ratatui Terminal 初始化是否正确\n"
      .. "4. 检查 SynchronizedUpdate 是否正确包裹\n"
      .. "5. 运行 cargo check --features tui 和 cargo test --features tui --lib\n"
      .. "6. 如有编译错误，用 edit 工具修复\n"
      .. "参考 phase-2.md §2.2 和 §2.3\n"
    ),
    schema = REVIEW_SCHEMA,
    working_folder = BASE,
  })
  all_results.render_pipe = { impl = phase_c_impl, review = phase_c_review }

  -- ════════════════════════════════════════════════════
  -- Phase D: 交互基础 (pane.rs, state.rs)
  -- ════════════════════════════════════════════════════
  phase("interact-base")

  local PHASE3_DOC = DOCS .. "/development/phase-3.md"
  local phase_d_modules = {
    {
      id = "pane",
      file = "pane.rs",
      task = "创建 PaneView trait + PaneStack 栈式面板管理器。\n"
          .. "Handled 枚举: Handled / NotHandled\n"
          .. "CtrlCAction 枚举: NotHandled / Handled / Cancel\n"
          .. "PaneView trait (extends Renderable):\n"
          .. "  handle_key_event(&mut self, key: KeyEvent) -> Handled\n"
          .. "  is_complete(&self) -> bool  (默认 false)\n"
          .. "  on_ctrl_c(&mut self) -> CtrlCAction  (默认 NotHandled)\n"
          .. "  view_id(&self) -> Option<&'static str>  (默认 None)\n"
          .. "PaneStack 结构体:\n"
          .. "  stack: Vec<Box<dyn PaneView>>, base: Option<Box<dyn PaneView>>\n"
          .. "  new(), set_base(), push(), pop(), active(), depth(), is_active()\n"
          .. "  handle_key_event() — 栈顶优先处理, cleanup_completed() 自动 pop\n"
          .. "PaneStack 实现 Renderable (委托给栈顶/基座)\n"
          .. "use super::render::Renderable;\n"
          .. "参考 phase-3.md §2.1"
    },
    {
      id = "state",
      file = "state.rs",
      task = "创建应用状态机。\n"
          .. "AppState 枚举: Idle / Inputting / Submitting / Processing / AwaitingApproval / Interrupted / Error / Exiting\n"
          .. "AppEvent 枚举: StartInput / Submit / Processing / Completed / RequestApproval / ApprovalDone / Interrupt / Resume / Error / Exit\n"
          .. "AppState::transition(&self, event: AppEvent) -> Result<AppState, StateError>\n"
          .. "  完整状态转换表参考 phase-3.md §2.5\n"
          .. "StateError(pub String)\n"
          .. "参考 phase-3.md §2.5"
    },
  }

  local d_impl, d_test, d_review = run_module_pipeline(
    phase_d_modules, PHASE3_DOC
  )
  all_results.interact_base = { impl = d_impl, test = d_test, review = d_review }

  -- ════════════════════════════════════════════════════
  -- Phase E: 交互组件 (composer.rs, approval.rs, selection.rs)
  -- ════════════════════════════════════════════════════
  phase("interact-comp")

  local phase_e_modules = {
    {
      id = "composer",
      file = "composer.rs",
      task = "创建输入框 Composer。\n"
          .. "Composer 结构体:\n"
          .. "  input: String, cursor: usize\n"
          .. "  history: Vec<String>, history_index: Option<usize>\n"
          .. "  slash_command: bool, placeholder: String\n"
          .. "  new(), content(), submit(), insert_text(), backspace()\n"
          .. "  handle_key(key: KeyEvent) -> ComposerAction\n"
          .. "ComposerAction 枚举: Continue / Submit(String)\n"
          .. "  Enter=提交, Shift+Enter=换行, ↑/↓=历史, Tab=补全(占位)\n"
          .. "实现 Renderable trait (输入框边框 + placeholder + 光标位置)\n"
          .. "参考 phase-3.md §2.2"
    },
    {
      id = "approval",
      file = "approval.rs",
      task = "创建审批弹窗 ApprovalOverlay。\n"
          .. "ApprovalRequest 枚举: Command{command, description} / FileEdit{file_path, diff} / ToolCall{tool_name, args}\n"
          .. "ApprovalResult 枚举: Allow / Deny / AlwaysAllow / ShowDiff\n"
          .. "ApprovalOverlay 结构体 (实现 PaneView):\n"
          .. "  request: ApprovalRequest, result: Option<ApprovalResult>\n"
          .. "  show_diff: bool, error: Option<String>\n"
          .. "  handle_key_event: Y=Allow, N/Esc=Deny, A=AlwaysAllow, D=toggle diff\n"
          .. "  is_complete: result.is_some()\n"
          .. "  on_ctrl_c: 设为 Deny, 返回 Handled\n"
          .. "use super::pane::{PaneView, Handled, CtrlCAction};\n"
          .. "参考 phase-3.md §2.3"
    },
    {
      id = "selection",
      file = "selection.rs",
      task = "创建选择列表 SelectionList。\n"
          .. "SelectionItem 结构体: { label: String, description: Option<String>, value: String }\n"
          .. "SelectionList 结构体 (实现 PaneView):\n"
          .. "  items: Vec<SelectionItem>, selected: usize\n"
          .. "  filter: String, confirmed: bool, result: Option<String>, title: String\n"
          .. "  handle_key_event: ↑/k=上, ↓/j=下, Enter=确认, 字符=过滤, Backspace=删过滤, Esc=取消\n"
          .. "  is_complete: confirmed\n"
          .. "  visible_items() 按 filter 过滤\n"
          .. "参考 phase-3.md §2.4"
    },
  }

  local e_impl, e_test, e_review = run_module_pipeline(
    phase_e_modules, PHASE3_DOC
  )
  all_results.interact_comp = { impl = e_impl, test = e_test, review = e_review }

  -- ════════════════════════════════════════════════════
  -- Phase F: 系统集成 (agent.rs, job_control.rs, notification.rs)
  -- ════════════════════════════════════════════════════
  phase("integrate")

  local PHASE4_DOC = DOCS .. "/development/phase-4.md"
  local AGENT_DOC = DOCS .. "/development/agent-integration.md"
  local phase_f_modules = {
    {
      id = "agent",
      file = "agent.rs",
      task = "创建 Agent 事件适配层。\n"
          .. "AgentEvent 枚举:\n"
          .. "  TextDelta(String), ReasoningDelta(String)\n"
          .. "  ToolCall{call_id, name, arguments}, ToolStart{call_id, name}\n"
          .. "  ToolOutput{call_id, name, content}, ToolEnd{call_id, name, result, is_error, raw_result}\n"
          .. "  Completed, Error(String)\n"
          .. "create_agent_channel() -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) (channel 256)\n"
          .. "create_stream_callback(tx) -> impl FnMut(StreamEvent<Value>) + Send\n"
          .. "  将 StreamEvent 转换为 AgentEvent, 使用 try_send\n"
          .. "参考现有代码: " .. CLI_SRC .. "/run/agent.rs (run_cli_turn 函数)\n"
          .. "参考文档: agent-integration.md §2, phase-4.md §2"
    },
    {
      id = "job-control",
      file = "job_control.rs",
      task = "创建 ^Z 暂停/恢复管理器。\n"
          .. "JobControl 结构体: suspend: Arc<Notify>, resume: Arc<Notify>, suspended: bool\n"
          .. "  new(), suspend_signal(), resume_signal(), is_suspended()\n"
          .. "  suspend() 方法: 恢复终端(DisableBracketedPaste, Show, disable_raw_mode)\n"
          .. "    → libc::raise(SIGTSTP) → 重新初始化(enable_raw_mode, EnableBracketedPaste, Hide)\n"
          .. "setup_signal_handler() 设置 SIGTSTP 为 SIG_IGN\n"
          .. "全部 #[cfg(unix)] 条件编译\n"
          .. "参考 phase-4.md §3"
    },
    {
      id = "notification",
      file = "notification.rs",
      task = "创建桌面通知系统。\n"
          .. "NotificationType 枚举: ReplyComplete / NeedApproval / Error\n"
          .. "NotificationManager 结构体:\n"
          .. "  terminal_focused: Arc<AtomicBool>, enabled: bool\n"
          .. "  new(enabled), focus_state() -> Arc<AtomicBool>\n"
          .. "  notify(&self, notif_type) -> Result\n"
          .. "    如果 enabled=false 或 terminal_focused=true 则跳过\n"
          .. "    macOS: 使用 std::process::Command::new(\"osascript\") 发送通知\n"
          .. "参考 phase-4.md §4"
    },
  }

  local f_impl, f_test, f_review = run_module_pipeline(
    phase_f_modules, PHASE4_DOC
  )
  all_results.integrate = { impl = f_impl, test = f_test, review = f_review }

  -- ════════════════════════════════════════════════════
  -- Phase G: 最终集成 + 编译验证
  -- ════════════════════════════════════════════════════
  phase("final")

  -- G1: 更新 mod.rs + app.rs 集成
  local integrate = agent({
    name = "final-integrate",
    description = "Update mod.rs exports and rewrite app.rs with full integration",
    prompt = ctx(
      "任务: 最终集成 — 更新 mod.rs 导出和 app.rs 主循环\n\n"
      .. "1. 编辑 " .. TUI_DIR .. "/mod.rs:\n"
      .. "   添加所有新模块的 pub mod 声明:\n"
      .. "   pub mod render; pub mod status; pub mod diff;\n"
      .. "   pub mod history_cell; pub mod streaming; pub mod spinner;\n"
      .. "   pub mod pane; pub mod composer; pub mod approval;\n"
      .. "   pub mod selection; pub mod state;\n"
      .. "   pub mod agent; pub mod job_control; pub mod notification;\n"
      .. "   添加合理的 pub use 重导出\n\n"
      .. "2. 编辑 " .. TUI_DIR .. "/app.rs:\n"
      .. "   全面集成所有组件:\n"
      .. "   - 引入 PaneStack, Composer, AppState 状态机\n"
      .. "   - 引入 HistoryCell, StreamingCell, StatusBar\n"
      .. "   - 引入 AgentEvent channel\n"
      .. "   - 字段替换:\n"
      .. "     content_lines → history: Vec<HistoryCell>\n"
      .. "     添加 active_cell: Option<StreamingCell>\n"
      .. "     添加 pane_stack: PaneStack\n"
      .. "     添加 status_bar: StatusBar\n"
      .. "   - handle_key() 委托给 pane_stack.handle_key_event()\n"
      .. "   - 添加 agent_rx 到 tokio::select!\n"
      .. "   - handle_agent_event() 处理 AgentEvent (参考 agent-integration.md §3.2)\n"
      .. "   - render() 使用 ratatui 渲染 (ColumnRenderable 组合 history + active_cell + pane_stack)\n"
      .. "   - 参考 " .. DOCS .. "/development/phase-3.md §4.1\n\n"
      .. "3. 运行 cargo check --features tui (在 " .. BASE .. " 目录)\n"
    ),
    schema = FILE_CREATED,
    working_folder = BASE,
  })
  all_results.final_integrate = integrate.ok

  -- G2: 编译 + 测试验证
  local verify_build = agent({
    name = "final-verify",
    description = "Run cargo build and test with tui feature, fix all errors",
    prompt = ctx(
      "任务: 最终编译和测试验证，修复所有编译错误\n\n"
      .. "1. 在 " .. BASE .. " 目录运行 cargo build --features tui\n"
      .. "2. 记录所有编译错误和警告\n"
      .. "3. 逐个修复编译错误 (使用 edit 工具):\n"
      .. "   - ratatui 0.29 API: SetCursorStyle 在 ratatui::style 模块\n"
      .. "   - trait bound: PaneView: Renderable 要求对象安全\n"
      .. "   - 生命周期: ColumnRenderable 可能需要 owned 版本\n"
      .. "   - unused imports 清理\n"
      .. "4. 运行 cargo test --features tui --lib\n"
      .. "5. 确保传统模式也正常: cargo build (不带 --features)\n"
      .. "6. 返回最终结果\n"
    ),
    schema = VERIFY,
    working_folder = BASE,
  })
  all_results.final_verify = verify_build

  -- ════════════════════════════════════════════════════
  -- Phase H: 对抗性验证 — 4角度并行审查 + 修复
  -- ════════════════════════════════════════════════════
  phase("adversarial")

  local REVIEWERS = {
    {
      id = "api-contract",
      angle = "API契约审查",
      focus = "检查所有 trait 实现的完整性和正确性:\n"
            .. "1. Renderable trait — 每个实现者必须实现 render + desired_height, cursor_pos/cursor_style 有默认值\n"
            .. "2. PaneView trait — extends Renderable, handle_key_event + is_complete + on_ctrl_c + view_id\n"
            .. "3. PaneStack — push/pop/active/cleanup_completed 逻辑, Renderable 委托给栈顶\n"
            .. "4. AppState 状态机 — 所有 (state, event) 组合的转换是否与 phase-3.md §2.5 一致\n"
            .. "5. Composer — Enter 提交, Shift+Enter 换行, 历史导航\n"
            .. "6. AgentEvent — StreamEvent → AgentEvent 转换是否覆盖所有 variant\n"
    },
    {
      id = "architecture",
      angle = "架构合规审查",
      focus = "检查五层架构合规性:\n"
            .. "1. 基础设施层(render.rs)不依赖交互层(pane.rs/composer.rs)\n"
            .. "2. 终端层(terminal.rs/viewport.rs)不依赖渲染层(render.rs)以外的上层\n"
            .. "3. 渲染层(render.rs/status.rs/diff.rs等)不依赖交互层\n"
            .. "4. 交互层(pane.rs/composer.rs/approval.rs)依赖渲染层但不依赖应用层\n"
            .. "5. mod.rs 导出完整, feature flag 条件编译正确\n"
            .. "6. 无循环依赖\n"
    },
    {
      id = "build-test",
      angle = "编译与测试审查",
      focus = "全面编译和测试验证:\n"
            .. "1. cargo build --features tui — 零错误零警告\n"
            .. "2. cargo test --features tui --lib — 所有测试通过\n"
            .. "3. cargo build (无 features) — 传统模式不受影响\n"
            .. "4. cargo clippy --features tui — 检查代码质量\n"
            .. "5. 记录所有 warning 和 failed test\n"
    },
    {
      id = "doc-compliance",
      angle = "设计文档对照审查",
      focus = "逐个模块对比设计文档验收标准:\n"
            .. "1. 对照 phase-2.md: Renderable trait 4方法, ColumnRenderable, FlexRenderable, StatusBar, DiffView, HistoryCell, StreamingCell, SpinnerWidget\n"
            .. "2. 对照 phase-3.md: PaneView trait, PaneStack, Composer+ComposerAction, ApprovalOverlay+ApprovalRequest+ApprovalResult, SelectionList+SelectionItem, AppState+AppEvent\n"
            .. "3. 对照 phase-4.md: AgentEvent+create_agent_channel+create_stream_callback, JobControl, NotificationManager\n"
            .. "4. 对照 agent-integration.md: run_agent_tui_turn, StreamEvent→AgentEvent 映射\n"
            .. "5. 检查是否有遗漏的功能点或接口\n"
    },
  }

  local votes = parallel(REVIEWERS, function(r)
    return {
      name = r.id,
      description = r.angle,
      prompt = ctx(
        "对抗性审查: " .. r.angle .. "\n\n"
        .. "审查范围: " .. TUI_DIR .. "/ 下所有 .rs 文件\n"
        .. "审查重点:\n" .. r.focus .. "\n"
        .. "要求:\n"
        .. "1. 先 ls " .. TUI_DIR .. " 列出所有文件\n"
        .. "2. 逐一 read 每个文件\n"
        .. "3. 严格按照审查重点检查\n"
        .. "4. 发现问题记录到 issues 数组: file/severity(critical|major|minor)/description/fix\n"
        .. "5. 给出总体评分(0-100)和 passed(布尔)\n"
        .. "6. 不要修复问题，只报告\n"
        .. "7. 如果需要运行命令验证, 在 " .. BASE .. " 目录运行\n"
      ),
      schema = REVIEW_SCHEMA,
      working_folder = BASE,
    }
  end)

  -- 统计投票
  local all_issues = {}
  local total_score = 0
  local pass_count = 0
  for i, v in ipairs(votes) do
    if v.ok then
      if v.output.passed then pass_count = pass_count + 1 end
      total_score = total_score + (v.output.score or 0)
      for _, issue in ipairs(v.output.issues or {}) do
        table.insert(all_issues, issue)
      end
    end
  end

  local avg_score = total_score / #REVIEWERS
  local pass_rate = pass_count / #REVIEWERS
  log("对抗验证: " .. pass_count .. "/" .. #REVIEWERS .. " 通过, 平均分 " .. tostring(avg_score))

  all_results.adversarial = {
    pass_rate = pass_rate,
    avg_score = avg_score,
    total_issues = #all_issues,
  }

  -- 如果有 critical/major 问题，触发修复
  local needs_fix = {}
  for _, issue in ipairs(all_issues) do
    if issue.severity == "critical" or issue.severity == "major" then
      table.insert(needs_fix, issue)
    end
  end

  if #needs_fix > 0 then
    phase("fix")
    log("发现 " .. #needs_fix .. " 个 critical/major 问题，启动修复", "warn")
    local fix = agent({
      name = "fix-issues",
      description = "Fix critical/major issues from adversarial review",
      prompt = ctx(
        "修复对抗性审查发现的问题:\n\n"
        .. json.encode(needs_fix) .. "\n\n"
        .. "约束:\n"
        .. "- 按严重程度排序，先修 critical 再修 major\n"
        .. "- 使用 edit 工具修复\n"
        .. "- 每次修复后说明改了什么\n"
        .. "- 全部修复后运行 cargo test --features tui --lib (在 " .. BASE .. " 目录)\n"
        .. "- 返回修复结果\n"
      ),
      schema = VERIFY,
      working_folder = BASE,
    })
    all_results.fix = fix
  end

  -- ════════════════════════════════════════════════════
  -- 最终报告
  -- ════════════════════════════════════════════════════
  report(all_results)
end
