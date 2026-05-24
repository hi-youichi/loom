# CLI 工具显示 — 交互体验问题与目标

## 灵感来源

基于对以下项目的调研：Gemini CLI（compact tool output）、Aider（SEARCH/REPLACE diff）、delta/bat（语法高亮 diff）、Evil Martians（进度显示 3 模式）、AI 等待 UX 研究。

**核心借鉴**：
- Gemini CLI：默认紧凑模式、单行摘要 + 可展开详情、结构化工具结果、工具名不截断、智能分组
- delta：word-level diff 高亮、语法着色、行编号装饰
- Aider：unified diff 格式、inline diff 反馈
- 进度指示器：spinner（\<2s）、X of Y（步骤计数）、elapsed time 始终可见
- AI 等待 UX：streaming 作为 progress appearance、tool call 可见性、step ordinal count

## 设计原则

1. **默认紧凑**：单行摘要，按需展开（Ctrl+O 全局切换）
2. **结构化结果**：每个工具返回结构化摘要，而非原始文本
3. **elapsed time 始终可见**：DONE 行必显耗时
4. **工具名不截断**：名称固定宽度，描述弹性截断
5. **视觉分层**：CALL → PREVIEW/DIFF → DONE 三阶段递进
6. **一致性措辞**：DONE 行结果摘要使用统一格式（见下方动词表）

## 动词表（DONE 行结果摘要）

| 语义 | 动词 | 示例 |
|------|------|------|
| 创建 | `created` | `→ created` |
| 删除 | `deleted` | `→ deleted` |
| 移动 | `moved` | `→ moved` |
| 替换 | `replaced N` | `→ replaced 1` |
| 写入 | `N bytes written` | `→ 28 bytes written` |
| 读取 | `N lines` | `→ 30 lines` |
| 搜索（文件） | `N files` | `→ 15 files` |
| 搜索（内容） | `N matches in M files` | `→ 12 matches in 3 files` |
| 搜索（网络） | `N results` | `→ 8 results` |
| HTTP | `status, size` | `→ 200, 4.2KB` |
| 任务 | `#id` / `#id status` | `→ #550e` |
| 加载 | `name loaded` | `→ deploy loaded` |
| 列出 | `N agents` / `N tasks` | `→ 4 agents` |
| 完成子任务 | `agent completed` | `→ explore completed` |

## 当前体验问题

1. **参数是原始 JSON** — `{"command":"grep -r \"TODO\""}` 噪音极大，用户真正需要的信息被 JSON 语法淹没
2. **DONE 行重复完整参数** — 浪费垂直空间，多工具时输出非常冗长
3. **无工具执行耗时** — 无法判断性能瓶颈
4. **无工具结果预览** — 需要等待完整输出才能了解进度
5. **多工具无编号/进度** — 并行工具调用堆叠在一起，难以跟踪
6. **CALL 和 DONE 之间无视觉分组** — 多轮次时输出混乱

## 目标体验

### 智能参数摘要（借鉴 Gemini CLI DenseToolMessage）

| 工具 | 当前 | 目标 |
|------|------|------|
| bash | `{"command":"grep -r \"TODO\" src/**/*.rs"}` | `grep -r "TODO" src/**/*.rs` |
| read | `{"path":"src/main.rs","offset":10,"limit":50}` | `src/main.rs [10:50]` |
| edit | `{"path":"src/main.rs","oldString":"fn main()","newString":"fn hello()"}` | `src/main.rs: "fn main()" → "fn hello()"` |
| write | `{"path":"src/new.rs","content":"fn main() {}..."}` | `src/new.rs (28 bytes)` |
| glob | `{"pattern":"**/*.rs"}` | `**/*.rs` |

### 精简完成行

- DONE 只显示工具名 + 状态 + 耗时，不重复参数（借鉴 Gemini CLI）
- 结果摘要：`bash ✓ (0.8s) → 3 lines`

### 视觉分组（借鉴 Gemini CLI ToolGroupMessage）

- 连续紧凑工具密集排列，标准工具（bash）独立边框盒
- 空行区分不同轮次

## 工具体验说明

每个工具最多包含四个维度：
- **CALL** — 用户看到工具正在做什么（单行摘要）
- **PREVIEW / DIFF** — 内联预览或 diff 视图（可折叠）
- **DONE** — 结果状态 + 耗时 + 结构化摘要
- **FAIL** — 失败时的错误信息

### 通用格式规范

```
_CALL  <tool>: <summary>
_PREV  <content>          ← 仅部分工具
_DONE  <tool> ✓ (X.Xs) → <result summary>
_DONE  <tool> ✗ (X.Xs) → <error message>
```

- `_CALL`/`_DONE` 前缀固定宽度 6 字符（含空格）
- `<tool>:` 工具名固定宽度 12 字符，左对齐
- PREVIEW 行号栏宽度按最大行号动态计算，`│` 右侧为内容区
- 摘要截断到终端宽度 - 20 字符
- 耗时格式：见下方「耗时显示规范」

### 文件操作

#### `bash`
- *做什么*：执行 shell 命令
- **CALL**
  - *摘要*：`原始命令`
  - *示例*：`_CALL  bash: grep -rn "format_tool" loom/src/**/*.rs`
  - *注意*：命令截断到终端宽度；多行命令取第一行 + `…`
- **DONE**
  - *成功*：`_DONE  bash ✓ (0.8s) → exit 0, 3 lines`
  - *失败*：`_DONE  bash ✗ (2.1s) → exit 1: permission denied`
  - *长输出*：compact 模式只显示最后 N 行摘要（借鉴 Gemini CLI compactShellOutput）
    ```
    _DONE  bash ✓ (12.3s) → exit 0, 847 lines
           ⋮ last 5 lines shown, full output in .loom/shell/...
    ```

#### `read`
- *做什么*：读取文件内容
- **CALL**
  - *摘要*：`path [offset:limit]`
  - *示例*：`_CALL  read: src/main.rs [80:110]`
- **PREVIEW**（借鉴 bat 风格）
  - *作用*：让用户快速判断是否读到目标内容
  - *显示*：
    ```
    _PREV  src/main.rs [80:110]
           80 │ pub fn format_tool_call(tool_name: &str, args_json: &str) -> String {
           81 │     let summary = extract_tool_summary(tool_name, args_json);
           82 │     let msg = format!("{}: {}", yellow(tool_name), summary);
           83 │     format_panel_line("CALL", &msg)
           84 │ }
              ⋮ (26 more lines)
    ```
  - *规则*：默认最多 5 行，超出行用 `⋮ (N more lines)` 折叠；行号左对齐 + `│` 分隔；内容超宽截断
  - *compact 模式*：隐藏 PREVIEW，DONE 行显示 `N lines`
- **DONE**
  - *示例*：`_DONE  read ✓ (0.1s) → 30 lines`

#### `edit`
- *做什么*：精确替换文件中的一段文本
- **CALL**
  - *摘要*：`path: "old" → "new"`
  - *示例*：`_CALL  edit: panel_format.rs: "args_summary: &str" → "args_json: &str"`
  - *截断*：old/new 各截断 30 字符
- **DIFF**（借鉴 delta + Aider unified diff）
  - *作用*：用户直观看到改了什么
  - *显示*：
    ```
    _DIFF  panel_format.rs:42
           fn format_tool_call(tool_name: &str,
    -      args_summary: &str) -> String {
    +      args_json: &str) -> String {
    ```
  - *规则*：
    - 文件名 + 行号定位
    - `-` 红色，`+` 绿色，上下文行默认色
    - 只显示变更行及前后 1 行
    - Word-level diff 高亮（借鉴 delta 的 Levenshtein 算法）：变更部分用粗体/下划线标记
    - 无颜色 fallback：`"old" → "new"` 单行
  - *compact 模式*：隐藏 DIFF，DONE 行显示替换数
- **DONE**
  - *成功*：`_DONE  edit ✓ (0.0s) → replaced 1`
  - *失败*：`_DONE  edit ✗ (0.0s) → oldString not found`

#### `multiedit`
- *做什么*：对同一文件进行多处替换
- **CALL**
  - *摘要*：`path: N edits`
  - *示例*：`_CALL  multiedit: panel_format.rs: 3 edits`
- **DIFF**
  - *显示*：
    ```
    _DIFF  panel_format.rs (3 edits)
           @@ line 42
           fn format_tool_call(tool_name: &str,
    -      args_summary: &str) -> String {
    +      args_json: &str) -> String {
           @@ line 78
    -      tool_name, args_json);
    +      tool_name, args_json, 60);
           @@ line 95
    -      _ => truncate_args(args_json, 60),
    +      _ => extract_first_meaningful_field(&val, 60),
    ```
  - *规则*：`@@ line N` 分隔；按行号排序；>5 处显示前 5 + `... N more`
- **DONE**
  - *示例*：`_DONE  multiedit ✓ (0.0s) → 3/3 replaced`

#### `write_file` / `fs_write_text_file`
- *做什么*：写入文件（创建或覆盖）
- **CALL**
  - *摘要*：`path (N bytes)`
  - *示例*：`_CALL  write_file: src/new.rs (28 bytes)`
  - *注意*：绝不能显示 content 原文
- **DONE**
  - *示例*：`_DONE  write_file ✓ (0.0s) → 28 bytes written`

#### `glob`
- *做什么*：按文件名模式匹配查找文件
- **CALL**
  - *摘要*：`pattern`
  - *示例*：`_CALL  glob: **/*.rs`
- **PREVIEW**
  - *作用*：让用户快速浏览匹配到的文件
  - *显示*：
    ```
    _PREV  glob: **/*.rs (15 files)
           src/main.rs
           src/lib.rs
           src/utils.rs
           src/parser/mod.rs
           src/parser/ast.rs
           ⋮ (10 more files)
    ```
  - *规则*：默认最多显示 10 个文件路径；超出行用 `⋮ (N more files)` 折叠；路径截断到终端宽度 - 12 字符
  - *compact 模式*：隐藏 PREVIEW，DONE 显示 `N files`
- **DONE**
  - *示例*：`_DONE  glob ✓ (0.2s) → 15 files`
  - *空结果*：`_DONE  glob ✓ (0.1s) → 0 files`

#### `grep`
- *做什么*：按正则搜索文件内容
- **CALL**
  - *摘要*：`pattern [include]`
  - *示例*：`_CALL  grep: format_tool [*.rs]`
- **PREVIEW**
  - *作用*：让用户快速浏览匹配结果
  - *显示*：
    ```
    _PREV  grep: format_tool (12 matches in 3 files)
           src/panel_format.rs:42:  pub fn format_tool_call(tool_name: &str, ...
           src/panel_format.rs:78:  let summary = extract_tool_summary(...
           src/panel_format.rs:95:  format_tool_done(tool_name, &msg)
           src/main.rs:12:          use panel_format::format_tool_call;
           ⋮ (8 more matches)
    ```
  - *规则*：默认最多显示 5 条匹配；格式为 `file:line: content`；content 截断到终端宽度 - 30 字符
  - *compact 模式*：隐藏 PREVIEW，DONE 显示 `N matches in M files`
- **DONE**
  - *示例*：`_DONE  grep ✓ (0.3s) → 12 matches in 3 files`

#### `create_dir`
- *做什么*：创建目录
- **CALL**：`_CALL  create_dir: src/new_module`
- **DONE**：`_DONE  create_dir ✓ (0.0s) → created`
- **FAIL**：`_DONE  create_dir ✗ (0.0s) → permission denied`

#### `delete_file`
- *做什么*：删除文件或空目录
- **CALL**：`_CALL  delete_file: src/old.rs`
- **DONE**：`_DONE  delete_file ✓ (0.0s) → deleted`
- **FAIL**：`_DONE  delete_file ✗ (0.0s) → file not found`

#### `move_file`
- *做什么*：移动或重命名文件
- **CALL**：`_CALL  move_file: src/old.rs → src/new.rs`
- **DONE**：`_DONE  move_file ✓ (0.0s) → moved`
- **FAIL**：`_DONE  move_file ✗ (0.0s) → source not found`

#### `apply_patch`
- *做什么*：应用多文件补丁
- **CALL**：`_CALL  apply_patch: patch (42 lines)`
- **DONE**：`_DONE  apply_patch ✓ (0.1s) → 3 files changed`
- **FAIL**：`_DONE  apply_patch ✗ (0.1s) → hunk failed at line 42`

### 性能与诊断

#### `lsp`
- *做什么*：语言服务协议操作（补全、诊断、跳转定义等）
- **CALL**：`_CALL  lsp: completion src/main.rs:42`
- **DONE**：`_DONE  lsp ✓ (0.2s) → 5 completions`
- **FAIL**：`_DONE  lsp ✗ (0.5s) → server not running`

### 网络

#### `web_fetcher`
- *做什么*：请求 URL 获取内容
- **CALL**：`_CALL  web_fetcher: GET https://api.example.com/data`
- **DONE**
  - *成功*：`_DONE  web_fetcher ✓ (1.2s) → 200, 4.2KB`
  - *失败*：`_DONE  web_fetcher ✗ (3.0s) → 404 Not Found`

#### `websearch`
- *做什么*：网络搜索
- **CALL**：`_CALL  websearch: Rust async best practices 2025`
- **PREVIEW**（借鉴 exa-cli compact 模式）
  - *作用*：让用户快速浏览搜索结果
  - *显示*：
    ```
    _PREV  websearch (8 results)
           1. "Rust Async Best Practices 2025" — blog.example.com
              "A comprehensive guide to async patterns..."
           2. "Async Rust Performance" — reddit.com/r/rust
              "Discussion on tokio vs async-std..."
           ⋮ (6 more results)
    ```
  - *规则*：最多 5 条，每条显示标题 + 域名 + 摘要首行（截断 60 字符）
  - *依赖*：需要 websearch API 返回结构化结果（title/url/snippet），若 API 只返回纯文本则降级为隐藏 PREVIEW
  - *compact 模式*：隐藏 PREVIEW
- **DONE**：`_DONE  websearch ✓ (2.1s) → 8 results`

### 任务管理

#### `task_create`
- **CALL**：`_CALL  task_create: Fix login bug`
- **DONE**：`_DONE  task_create ✓ (0.1s) → #550e`
- **FAIL**：`_DONE  task_create ✗ (0.0s) → missing name`

#### `task_update`
- **CALL**：`_CALL  task_update: #550e → completed`
- **DONE**：`_DONE  task_update ✓ (0.0s) → #550e updated`
- **FAIL**：`_DONE  task_update ✗ (0.0s) → task not found`

#### `task_list`
- **CALL**：`_CALL  task_list: pending: login`
- **PREVIEW**
  - *作用*：让用户快速浏览匹配的任务
  - *显示*：
    ```
    _PREV  task_list (3 tasks)
           ○ #a1b2 Fix login redirect bug       [pending]
           ○ #c3d4 Update login API endpoint    [pending]
           ○ #e5f6 Add login unit tests         [pending]
    ```
  - *规则*：显示 ID 前缀 + 名称 + 状态；名称截断到终端宽度 - 25 字符
  - *compact 模式*：隐藏 PREVIEW
- **DONE**：`_DONE  task_list ✓ (0.1s) → 3 tasks`
- **FAIL**：`_DONE  task_list ✗ (0.0s) → invalid filter`

#### `task_show`
- **CALL**：`_CALL  task_show: #550e`
- **DONE**：`_DONE  task_show ✓ (0.0s) → #550e in_progress`
- **FAIL**：`_DONE  task_show ✗ (0.0s) → ambiguous prefix`

#### `task_delete`
- **CALL**：`_CALL  task_delete: #550e`
- **DONE**：`_DONE  task_delete ✓ (0.0s) → #550e deleted`
- **FAIL**：`_DONE  task_delete ✗ (0.0s) → task not found`

### Agent 与 Skill

#### `invoke_agent`
- *做什么*：委派任务给子 agent
- **CALL**：`_CALL  invoke_agent: explore: find format_tool usage`
- *注意*：耗时可能较长，建议在 spinner 中显示 agent 名称
  ```
  ⠋ explore: scanning codebase...
  ```
- **DONE**：`_DONE  invoke_agent ✓ (12.3s) → explore completed`
- **FAIL**：`_DONE  invoke_agent ✗ (8.1s) → agent error: timeout`

#### `list_agents`
- **CALL**：`_CALL  list_agents`
- **DONE**：`_DONE  list_agents ✓ (0.1s) → 4 agents`
- **FAIL**：`_DONE  list_agents ✗ (0.0s) → no agents configured`

#### `skill`
- **CALL**：`_CALL  skill: deploy`
- **DONE**：`_DONE  skill ✓ (0.0s) → deploy loaded`
- **FAIL**：`_DONE  skill ✗ (0.0s) → skill not found`

#### `help`
- **CALL**：`_CALL  help`
- **DONE**：`_DONE  help ✓`

### 其他

#### `todo_write`
- *做什么*：写入待办事项列表
- **CALL**：`_CALL  todo_write: 5 todos`
- **PREVIEW**
  - *作用*：让用户确认待办事项是否正确
  - *显示*：
    ```
    _PREV  todo_write (5 todos)
           ✓ 1. Run the build                    [completed]
           ● 2. Fix type errors in panel_format   [in_progress]
           ○ 3. Update unit tests                 [pending]
           ○ 4. Run lint check                    [pending]
           ○ 5. Commit changes                    [pending]
    ```
  - *规则*：`✓`=completed, `●`=in_progress, `○`=pending, `⊘`=cancelled；每项截断到终端宽度 - 25 字符
  - *compact 模式*：隐藏 PREVIEW
- **DONE**：`_DONE  todo_write ✓ (0.0s) → 5 todos saved`
- **FAIL**：`_DONE  todo_write ✗ (0.0s) → invalid todos format`

#### `todo_read`
- *做什么*：读取待办列表
- **CALL**：`_CALL  todo_read`
- **PREVIEW**
  - *作用*：直接展示当前待办状态，无需等待后续输出
  - *显示*：
    ```
    _PREV  todo_read (3 pending, 1 in_progress, 2 completed)
           ● 2. Fix type errors in panel_format   [in_progress]
           ○ 3. Update unit tests                 [pending]
           ○ 4. Run lint check                    [pending]
           ○ 5. Commit changes                    [pending]
           ⋮ (2 completed hidden)
    ```
  - *规则*：只显示 pending + in_progress 项，completed/collapsed 折叠为 `⋮ (N completed hidden)`
  - *compact 模式*：隐藏 PREVIEW，DONE 显示摘要
- **DONE**：`_DONE  todo_read ✓ (0.0s) → 3 pending, 2 completed`
- **FAIL**：`_DONE  todo_read ✗ (0.0s) → no session active`

### 无参数工具汇总

`list_agents`、`help`、`todo_read`

## 全局交互机制

### 紧凑模式（借鉴 Gemini CLI compactToolOutput）

- compact 模式
- compact 模式下：隐藏所有 PREVIEW/DIFF；CALL 只显示单行摘要；DONE 只显示工具名 + 状态 + 耗时 + 结果计数
- 以下工具有 compact 折叠规则：
  - `read`：隐藏 PREVIEW，DONE 显示 `N lines`
  - `edit`/`multiedit`：隐藏 DIFF，DONE 显示 `replaced N`
  - `grep`：隐藏匹配列表，DONE 显示 `N matches in M files`
  - `glob`：隐藏 PREVIEW，DONE 显示 `N files`
  - `grep`：隐藏 PREVIEW，DONE 显示 `N matches in M files`
  - `websearch`：隐藏 PREVIEW，DONE 显示 `N results`
  - `todo_write`/`todo_read`：隐藏 PREVIEW，DONE 显示摘要
  - `task_list`：隐藏 PREVIEW，DONE 显示 `N tasks`
  - `bash`（长输出）：隐藏完整输出，显示最后 5 行摘要
- `Ctrl+O` 全局展开/折叠所有工具输出
- 可配置：`ui.compactToolOutput: true/false`

### 多工具分组（借鉴 Gemini CLI ToolGroupMessage）

- 连续紧凑工具（read、glob、grep、edit）密集排列
- 标准工具（bash、invoke_agent）独立边框盒
- 每轮工具调用间空行分隔

### 并行工具调用（batch）

- 并行工具的 CALL 行连续显示，带序号：
  ```
  _CALL  [1/3] read: src/main.rs
  _CALL  [2/3] read: src/lib.rs
  _CALL  [3/3] read: src/utils.rs
  ```
- 并行执行中，显示已完成和仍在运行的：
  ```
  _DONE  [2/3] read ✓ (0.1s) → 30 lines
  ⠋ [1/3] [3/3] still running...
  ```
- DONE 行按完成顺序显示，先完成的先出现
- compact 模式下全部单行显示

### 实时进度

- `bash` 长命令（>2s）显示实时输出流，持续刷新最后 3 行：
  ```
  ⠋ bash: cargo build --release (12.3s)
         Compiling loom v0.1.0
         Compiling deps... 42/87
         Building [=================>           ] 48%
  ```
  - spinner 后显示 elapsed time，每秒更新
  - 输出行用终端 ANSI 控制覆盖刷新，不留历史
- `websearch` 等待时 spinner 显示 `⠋ searching...`
- `invoke_agent` spinner 显示 `⠋ explore: scanning codebase...`

### 中断显示

- 用户按 Ctrl+C 中断工具时，显示中断确认：
  ```
  _INTR  bash ✗ (3.2s) → interrupted by user
  ```
- 中断不等于失败，用 `_INTR` 前缀区分 `_DONE  ✗`
- 已完成的工具不受影响，只中断当前正在运行的

### 步骤进度（借鉴 Evil Martians X-of-Y 模式 + AI 等待 UX 研究）

- 多轮对话显示步骤计数：`Step 3/7` 或 `Step 3`
- Spinner 中显示当前活跃工具名
- 超过 10s 的操作显示 elapsed time 更新

### 耗时显示规范

| 耗时 | 格式 | 示例 |
|------|------|------|
| < 100ms | `(0.0s)` | `(0.0s)` |
| 100ms–1s | `(0.Xs)` | `(0.3s)` |
| 1s–60s | `(X.Xs)` | `(12.3s)` |
| >= 60s | `(Xm XXs)` | `(2m 15s)` |

### 色彩规范

| 用途 | 颜色 | ANSI | 示例 |
|------|------|------|------|
| 成功 | 绿色 | `\e[32m` | `✓` |
| 失败 | 红色 | `\e[31m` | `✗` |
| 警告/工具名 | 黄色 | `\e[33m` | `bash:` |
| 次要信息 | 灰色 | `\e[90m` | `(0.0s)`、`⋮` |
| Diff 删除 | 红色 | `\e[31m` | `- old code` |
| Diff 新增 | 绿色 | `\e[32m` | `+ new code` |
| Diff 上下文 | 默认色 | — | `context line` |
| 中断 | 黄色 | `\e[33m` | `_INTR` |
| 行号/状态符号 | 青色 | `\e[36m` | `●`、`○` |

- 无颜色终端（`NO_COLOR` 或非 TTY）：所有前缀用纯文本标记（`✓`/`✗`/`-`/`+`），无 ANSI
- 色彩遵循 [NO_COLOR](https://no-color.org/) 规范

### 终端宽度适配

| 终端宽度 | 策略 |
|----------|------|
| < 80 列 | 极简模式：隐藏所有 PREVIEW/DIFF，摘要截断到 50 字符 |
| 80–120 列 | 标准模式：PREVIEW 默认折叠，摘要截断到终端宽度 - 20 |
| > 120 列 | 宽屏模式：PREVIEW 默认展开，摘要截断到 100 字符 |

- 运行时自动检测终端宽度变化并适配
- 管道/重定向输出时忽略宽度限制，输出完整内容

## 完整会话示例

以下是一个真实的端到端会话，展示所有机制协同工作：

```
⠋ Thinking...

_CALL  bash: grep -rn "format_tool" loom/src/**/*.rs
_USAGE  2.35s | 1.2K↓ 800↑ @ 850 t/s

_DONE  bash ✓ (0.8s) → exit 0, 3 lines

_CALL  [1/2] read: loom/src/panel_format.rs [80:110]
_CALL  [2/2] read: loom/src/panel_format.rs [120:150]
_PREV  [1/2] loom/src/panel_format.rs [80:110]
       80 │ pub fn format_tool_call(tool_name: &str, args_summary: &str) -> String {
       81 │     let summary = extract_tool_summary(tool_name, args_summary);
       82 │     let msg = format!("{}: {}", yellow(tool_name), summary);
       83 │     format_panel_line("CALL", &msg)
          ⋮ (26 more lines)
_DONE  [1/2] read ✓ (0.1s) → 30 lines
_DONE  [2/2] read ✓ (0.1s) → 30 lines

_CALL  edit: panel_format.rs: "args_summary: &str" → "args_json: &str"
_DIFF  panel_format.rs:42
       fn format_tool_call(tool_name: &str,
-      args_summary: &str) -> String {
+      args_json: &str) -> String {
_DONE  edit ✓ (0.0s) → replaced 1

_CALL  bash: cargo build 2>&1
⠋ bash: cargo build (4.2s)
       Compiling loom v0.1.0
       Building [===================>          ] 72%
_DONE  bash ✓ (8.1s) → exit 0

_CALL  todo_write: 3 todos
_PREV  todo_write (3 todos)
       ✓ 1. grep format_tool usage          [completed]
       ● 2. Fix parameter name               [in_progress]
       ○ 3. Run build                        [pending]
_DONE  todo_write ✓ (0.0s) → 3 todos saved
```

## 分阶段交付

- **Phase 1**（纯展示层）：智能参数摘要 + 精简 DONE 行（不重复参数）
- **Phase 2**（事件扩展）：耗时追踪 + PREVIEW/DIFF + compact 模式开关
- **Phase 3**（锦上添花）：多工具分组 + 步骤进度 + Ctrl+O 全局切换 + 实时进度流
- **Phase 4**（完善）：终端宽度自适应 + 中断显示 + 色彩规范 + 完整会话验证
