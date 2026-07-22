# OpenChamber ↔ Loom 功能验收手册

> 通过模拟真实用户操作验证功能完整性，不涉及接口级检查。
> 所有启动命令基于 PowerShell 5.1（PS 5.1 不支持 `&&`，使用 `;`）。

## 0. 启动最新代码（前置条件）

> 验收前必须使用**最新代码**重新构建并启动两端，否则验证的是旧实现。
> 端口约定：前端 3000，后端 18081。

### 0.1 同步代码

**后端 (loom-server)**:

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
git fetch origin; git pull --ff-only
git log -1 --oneline     # 确认 HEAD 是最新 commit
```

**前端 (openchamber-feat-dev)**:

```powershell
cd C:\Users\heycj\dev\openchamber-feat-dev
git fetch origin; git pull --ff-only
git log -1 --oneline     # 确认 HEAD 是最新 commit
```

### 0.2 重新构建后端（确保无缓存脏数据）

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
cargo build -p loom-server --release
```

> `--release` 让 `/api/*` 性能与最终产物一致；调试用 `cargo run -p loom-server`。
> 若只想复用上次 build：`cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081`，
> Cargo 会自动增量编译。

### 0.3 启动 loom-server（端口 18081）

**方式 A — Release 二进制（推荐，最贴近生产）**:

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
$env:RUST_LOG = "info,loom_server=debug"
Start-Process pwsh -ArgumentList "-NoLogo","-Command","& {
  cargo run -p loom-server --release -- serve --host 127.0.0.1 --port 18081
}" -WindowStyle Minimized
```

**方式 B — 直接前台跑（看实时日志）**:

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
$env:RUST_LOG = "info,loom_server=debug"
cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081
```

**确认启动**:

```powershell
Start-Sleep -Seconds 3
Invoke-RestMethod -Uri "http://127.0.0.1:18081/health"
Invoke-RestMethod -Uri "http://127.0.0.1:18081/provider" | ConvertTo-Json -Depth 3
```

期望：返回 `{providers, default}` 或 `{all, connected, default}`，非空。

### 0.4 启动 OpenChamber 前端（端口 3000）

**前置**: 已启动 loom-server（0.3）。

```powershell
cd C:\Users\heycj\dev\openchamber-feat-dev
bun install                   # 仅在 lockfile 变更后需要
$env:OPENCODE_HOST   = "http://127.0.0.1:18081"
$env:OPENCODE_SKIP_START = "true"   # 跳过自启 opencode 子进程
bun run packages/web/dev
```

> `OPENCODE_HOST` 让 openchamber 进入外部模式 — 不 spawn `opencode serve`，
> 所有 `/api/*` 代理到 loom-server:18081（环境变量位于 `packages/web/src/env-config.js`，
> 控制逻辑在 `packages/web/src/server/lifecycle.js`）。
> `OPENCODE_SKIP_START="true"` 是配套保险，防止任何兜底 spawn。

**确认启动**:

- Vite 终端出现 `Local: http://127.0.0.1:3000/`
- 浏览器打开 `http://127.0.0.1:3000/` 不再自动 spawn opencode 子进程
- DevTools → Network → 任一 `/api/...` 请求 URL 显示 `127.0.0.1:18081`

### 0.5 健康检查清单（验收前必跑）

```powershell
# 1) 后端进程 + 端口监听
Get-Process | Where-Object { $_.ProcessName -like "*loom*" -or $_.CommandLine -like "*18081*" }
Test-NetConnection 127.0.0.1 -Port 18081

# 2) 前端进程 + 端口监听
Get-Process bun,pwsh | Where-Object { $_.CommandLine -like "*web/dev*" }
Test-NetConnection 127.0.0.1 -Port 3000

# 3) 端到端打通
Invoke-RestMethod -Uri "http://127.0.0.1:18081/config/providers"
```

期望：两端进程在跑、两个端口都连通、后端 provider 端点返回非空。

### 0.6 关闭顺序（验收结束）

```powershell
# 1) 前端 Ctrl+C；2) 后端 Ctrl+C；或直接结束进程
Get-Process | Where-Object { $_.CommandLine -like "*18081*" } | Stop-Process
Get-Process bun | Where-Object { $_.CommandLine -like "*web/dev*" } | Stop-Process
```

## 前提

- OpenChamber 前端运行在 `http://127.0.0.1:3000`（按 0.4 启动）
- Loom 后端运行在 `http://127.0.0.1:18081`（按 0.3 启动）
- 使用最新代码（按 0.1-0.2 同步并重建）

## 工具操作

| 操作 | 工具 | 说明 |
|------|------|------|
| 打开页面 | `navigate_page` | 导航到指定 URL |
| 查看页面元素 | `take_snapshot` | 获取当前页面 DOM 树和 uid |
| 点击按钮 | `click` | 用 uid 点击元素 |
| 输入文字 | `fill` / `type_text` | 向输入框填入内容 |
| 截图 | `take_screenshot` | 视觉确认页面状态 |
| 等待内容出现 | `wait_for` | 等待特定文字出现在页面上 |
| 查看控制台 | `list_console_messages` | 检查是否有报错 |
| 查看网络请求 | `list_network_requests` | 确认请求发出 |

基本操作流程：
1. `navigate_page` 打开目标页面
2. `take_snapshot` 获取元素 uid
3. `click` / `fill` 操作元素
4. `take_snapshot` 或 `take_screenshot` 确认结果
5. `list_console_messages` 检查无报错

---

## 1. 启动与首页

### 步骤

```
1. navigate_page → http://127.0.0.1:3000/
2. wait_for → ["OpenChamber"]
3. take_snapshot
4. take_screenshot
```

### 验证点

- 页面标题显示 "OpenChamber"
- 左侧边栏可见（Session 列表、Settings、Git 等入口）
- 无错误通知弹出
- 控制台无红色错误（`list_console_messages` → types: ["error"]）

---

## 2. Provider 管理

### 2.1 查看 Provider 列表

```
1. navigate_page → http://127.0.0.1:3000/?settings=providers
2. wait_for → ["Providers"]
3. take_snapshot
4. take_screenshot
```

**验证点**:
- 列表显示 "Total N"（N > 0）
- 每个 provider 有名称、状态（connected/disconnected）
- "Connect Provider" 按钮可见

### 2.2 添加 Provider

```
1. take_snapshot → 找到 "Connect Provider" 按钮的 uid
2. click → uid: <Connect Provider 按钮>
3. wait_for → ["API Key"]  （或 "provider" 选择列表）
4. take_snapshot
5. take_screenshot
```

**验证点**:
- 弹出对话框/面板
- 显示可用 provider 列表（非空）
- 有 API Key 输入框
- 无 "Failed to load provider auth methods" 错误通知

### 2.3 输入 API Key 连接

```
1. take_snapshot → 找到 provider 下拉框 uid 和 API Key 输入框 uid
2. fill → uid: <provider 选择>, value: <provider 名称>
3. fill → uid: <API Key 输入框>, value: "test-key-xxx"
4. click → uid: <确认/Connect 按钮>
5. wait_for → ["connected"]  或  ["success"]
6. take_snapshot
7. take_screenshot
```

**验证点**:
- provider 状态变为 connected
- 列表更新
- 无报错

### 2.4 断开 Provider

```
1. take_snapshot → 找到已连接 provider 的 "Disconnect" 按钮 uid
2. click → uid: <Disconnect 按钮>
3. wait_for → ["disconnected"]  或确认弹窗出现
4. 如有确认弹窗: click → uid: <确认>
5. take_snapshot
```

**验证点**:
- provider 状态变为 disconnected
- 列表更新

---

## 3. 对话（Session）

### 3.1 新建对话

```
1. navigate_page → http://127.0.0.1:3000/
2. take_snapshot
3. 找到 "New Session" / "New Chat" / "+" 按钮
4. click → uid: <新建按钮>
5. take_snapshot
```

**验证点**:
- 创建新 session，显示空对话界面
- 有消息输入框
- 有模型/provider 选择器（显示当前使用的 model）

### 3.2 发送消息

```
1. take_snapshot → 找到输入框 uid
2. fill → uid: <输入框>, value: "Hello, what is 2+2?"
3. click → uid: <发送按钮>  (或 press_key → "Enter")
4. wait_for → ["4"]  或 ["response"]  (等待 AI 回复)
5. take_snapshot
6. take_screenshot
```

**验证点**:
- 用户消息显示在对话区
- AI 回复流式渲染出现（逐字或分块）
- 回复内容合理（包含 "4"）
- 无 "service unavailable" 等错误
- 消息有时间戳

### 3.3 查看对话历史

```
1. navigate_page → http://127.0.0.1:3000/
2. take_snapshot → 找到左侧 session 列表
3. click → uid: <某个历史 session>
4. take_snapshot
5. take_screenshot
```

**验证点**:
- 加载已有消息
- 消息顺序正确（用户/AI 交替）
- 代码块正确渲染

### 3.4 中断对话

```
1. fill → uid: <输入框>, value: "Write a long essay about..."
2. click → uid: <发送按钮>
3. wait_for → ["Stop"]  (等待 Stop 按钮出现)
4. click → uid: <Stop 按钮>
5. take_snapshot
```

**验证点**:
- 回复中断，已生成内容保留
- Stop 按钮恢复为发送按钮
- 无异常

---

## 4. 文件浏览

### 4.1 打开文件树

```
1. navigate_page → http://127.0.0.1:3000/
2. take_snapshot → 找到文件浏览器面板/图标
3. click → uid: <文件浏览器入口>
4. take_snapshot
```

**验证点**:
- 显示当前工作目录的文件树
- 目录可展开/折叠
- 文件有图标区分类型

### 4.2 创建文件

```
1. take_snapshot → 找到 "New File" 按钮或右键菜单
2. click → uid: <新建文件>
3. fill → uid: <文件名输入>, value: "test-verify.txt"
4. press_key → "Enter"
5. wait_for → ["test-verify.txt"]
6. take_snapshot
```

**验证点**:
- 新文件出现在文件树
- 文件可点击打开编辑

### 4.3 编辑文件

```
1. click → uid: <test-verify.txt>
2. fill → uid: <编辑器区域>, value: "hello world"
3. press_key → "Control+S"  (保存)
4. take_snapshot
```

**验证点**:
- 编辑器打开文件内容
- 保存成功（无报错，或显示 "saved"）

### 4.4 删除文件

```
1. take_snapshot → 找到 test-verify.txt 的右键菜单或删除按钮
2. click → uid: <删除/右键>
3. 如有确认弹窗: click → uid: <确认删除>
4. take_snapshot
```

**验证点**:
- 文件从文件树消失

---

## 5. Git 操作

### 5.1 查看 Git 状态

```
1. navigate_page → http://127.0.0.1:3000/
2. take_snapshot → 找到 Git/SOURCE CONTROL 面板入口
3. click → uid: <Git 面板>
4. take_snapshot
5. take_screenshot
```

**验证点**:
- 显示当前分支名
- 显示 modified/staged/untracked 文件列表
- 显示 diff 预览

### 5.2 Stage 文件

```
1. take_snapshot → 找到某文件的 "+" (stage) 按钮
2. click → uid: <stage 按钮>
3. take_snapshot
```

**验证点**:
- 文件从 "Changes" 移到 "Staged Changes"

### 5.3 提交

```
1. take_snapshot → 找到 commit message 输入框
2. fill → uid: <commit 输入框>, value: "test commit from openchamber"
3. click → uid: <Commit 按钮>  (或 press_key → "Control+Enter")
4. wait_for → ["committed"]  或等待输入框清空
5. take_snapshot
```

**验证点**:
- Commit 成功
- Staged 列表清空
- Commit 历史更新

### 5.4 查看 Diff

```
1. click → uid: <某个已修改文件>
2. take_snapshot
3. take_screenshot
```

**验证点**:
- Diff 视图打开（左右对比或行内高亮）
- 新增行（绿色）/删除行（红色）正确显示

---

## 6. Settings 配置

### 6.1 查看全局设置

```
1. navigate_page → http://127.0.0.1:3000/?settings=general
2. wait_for → ["Settings"]  或 ["Theme"]
3. take_snapshot
4. take_screenshot
```

**验证点**:
- 设置页面加载
- Theme 选择器可见
- 当前模型/Provider 显示正确

### 6.2 切换主题

```
1. take_snapshot → 找到主题选择器
2. click → uid: <主题下拉框>
3. click → uid: <某个主题选项>
4. take_screenshot
```

**验证点**:
- 页面主题立即切换
- 颜色/样式变化可见

### 6.3 切换模型

```
1. take_snapshot → 找到 Model 选择器（设置页或顶部栏）
2. click → uid: <Model 下拉框>
3. take_snapshot → 查看可选模型列表
4. click → uid: <某个模型>
5. take_snapshot
```

**验证点**:
- 模型列表非空
- 切换后显示新模型名称
- 新对话使用选中的模型

---

## 7. 错误处理

### 7.1 网络断开恢复

```
1. navigate_page → http://127.0.0.1:3000/
2. emulate → networkConditions: "Offline"
3. fill → uid: <输入框>, value: "test"
4. click → uid: <发送>
5. wait_for → ["error"]  或 ["failed"]  或 ["retry"]
6. take_screenshot
7. emulate → (清除网络限制)
8. wait_for → ["connected"]  或等待恢复正常
9. take_screenshot
```

**验证点**:
- 断网时显示友好错误提示（非白屏/崩溃）
- 恢复网络后自动重连
- SSE 事件流恢复

### 7.2 检查控制台报错

```
1. list_console_messages → types: ["error"]
```

**验证点**:
- 无未处理的异常
- 可接受的 warning（如 deprecation）可以忽略

---

## 结果记录模板

```markdown
### 验收结果 (YYYY-MM-DD)

**后端**: loom-server @ `<commit-hash>` (branch: dev) on 127.0.0.1:18081 — release build
**前端**: openchamber-feat-dev @ `<commit-hash>` (branch: dev) on 127.0.0.1:3000 — bun run packages/web/dev

| 功能 | 操作 | 状态 | 备注 |
|------|------|------|------|
| 首页加载 | 打开 :3000 | ✅/❌ | |
| Provider 列表 | Settings → Providers | ✅/❌ | |
| 添加 Provider | Connect → 输入 Key | ✅/❌ | |
| 新建对话 | New Session | ✅/❌ | |
| 发送消息 | 输入 + 发送 | ✅/❌ | |
| AI 回复 | 等待流式响应 | ✅/❌ | |
| 中断对话 | 点 Stop | ✅/❌ | |
| 文件树 | 打开文件浏览器 | ✅/❌ | |
| 创建文件 | New File | ✅/❌ | |
| Git 状态 | 打开 Git 面板 | ✅/❌ | |
| Stage/Commit | 暂存 + 提交 | ✅/❌ | |
| Diff 视图 | 点击已修改文件 | ✅/❌ | |
| 切换主题 | Settings → Theme | ✅/❌ | |
| 切换模型 | Model 选择器 | ✅/❌ | |
| 断网恢复 | Offline → 恢复 | ✅/❌ | |
| 控制台 | 无未处理异常 | ✅/❌ | |
```
