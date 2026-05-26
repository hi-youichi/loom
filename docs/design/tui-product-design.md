# Loom TUI 产品设计方案

## 一、定位与目标

**目标用户**：开发者和技术团队，使用 Loom 多 Agent 系统进行任务协作。

**核心价值**：将现有的 AI Company 多 Agent 架构通过 TUI 可视化，提供实时状态监控、协作交互、和流程控制。

**设计哲学**：参考 7 种经典 TUI 布局模式中的 **Persistent Multi-Panel**（持久多面板）——空间一致性让用户建立肌肉记忆，固定位置代替导航。

---

## 二、架构选型

### 2.1 推荐框架：Ratatui + 自定义 MVU

**理由**：
- 生态最成熟（3700+ crates，20k+ stars），维护活跃
- 零 C 依赖，纯 Rust，适合 Loom 项目
- 支持 `crossterm` 后端，Windows 兼容性最佳
- 丰富的 widget 生态（charts, tables, sparklines）

**备选**：
- `textual-rs`（React 风格，适合复杂状态管理）
- `bubble-tea-rs`（Elm 架构，适合消息驱动的 Agent 交互）

### 2.2 架构模式：MVU（Model-View-Update）

```rust
// 核心循环
Model (状态) → View (渲染) → Update (消息处理) → Model
```

**优势**：
- 单向数据流，状态可预测
- 易于测试（状态是纯数据）
- 组件可组合（类似 React）

---

## 三、布局设计

### 3.1 主布局：四区域分屏

```
┌─────────────────────────────────────────────────────────────┐
│  [Logo] Loom TUI           Session: dev  │  Ctrl+H 帮助      │
├─────────────┬───────────────────────────────┬───────────────┤
│             │                               │               │
│   团队面板    │       会话/输出区域            │   任务面板     │
│   (Agents)   │       (Main View)            │   (Tasks)     │
│   200 cols   │       flexible               │   300 cols    │
│             │                               │               │
│  ○ CEO      │  [Agent] thinking...          │  ▸ In Progress│
│  ○ Architect│  [Agent] calling tools...      │    • 子任务1  │
│  ○ Engineer │                               │    • 子任务2  │
│  ○ QA       │  ─────────────────────────    │               │
│             │  [User] >                     │  ✓ Completed  │
│             │                               │    • 子任务3  │
├─────────────┴───────────────────────────────┴───────────────┤
│  Tokens: 12.5k │  Cost: $0.42 │  [streaming ●] │  模型: deepseek│
└─────────────────────────────────────────────────────────────┘
```

### 3.2 面板职责

| 面板 | 宽度 | 内容 | 优先级 |
|------|------|------|--------|
| **团队面板** | 20-25 cols | Agent 状态、在线/忙碌指示 | P1 |
| **主区域** | flexible | 会话流、工具调用、输出 | P0 |
| **任务面板** | 30-35 cols | Task 看板、进度 | P0 |
| **状态栏** | 1 row | 统计、快捷键 | P1 |

### 3.3 响应式策略

| 终端宽度 | 布局 |
|---------|------|
| < 80 cols | 隐藏任务面板，仅主区域 |
| 80-120 cols | 紧凑模式，面板收窄 |
| 120-160 cols | 标准模式 |
| > 160 cols | 宽裕模式，展开所有面板 |

---

## 四、功能模块

### 4.1 团队面板（Team Panel）

**功能**：
- 显示所有 Agent 及其当前状态（空闲/思考/执行工具）
- 单击切换到对应 Agent 的视角
- 实时心跳指示器（每 5s 更新）

**视觉**：
- `○` 空闲（绿色）
- `◐` 思考中（黄色）
- `●` 执行工具（红色）
- `✕` 离线/错误（灰色）

### 4.2 会话主区域（Main View）

**功能**：
- 流式输出渲染（token 逐字显示）
- 思考过程折叠/展开（`<think>` 标签区域）
- 工具调用卡片（工具名、参数摘要、结果）
- Markdown 渲染（代码高亮、链接）

**交互**：
- `↑/↓` 在历史消息间导航
- `Enter` 发送消息
- `Ctrl+E` 展开/折叠思考过程
- `Space` 暂停/恢复流式输出

### 4.3 任务面板（Task Panel）

**功能**：
- 实时 Task 列表（按状态分组）
- 子任务依赖可视化
- 进度条（已完成/总数）
- 单击跳转到对应 Task 的详情

**布局**：
```
▸ In Progress (3)
  ├─ • 设计TUI方案       [▓▓▓░░░] 60%
  ├─ • 实现MVP          [░░░░░░░] 0%
  └─ ● 子任务3          [▓▓▓▓▓▓] 运行中

✓ Completed (5)
  ├─ • 需求分析
  └─ • 架构设计

○ Pending (2)
  ├─ • 测试验证
  └─ • 文档编写
```

### 4.4 工具执行层

**工具调用卡片**：
```
┌─ bash ─────────────────────────────┐
│  > cargo build --release            │
├─────────────────────────────────────┤
│  ✓ Completed (4.2s)                │
│  编译成功，生成 23 个 crate         │
└─────────────────────────────────────┘
```

**工具状态**：
- 运行中：动画边框（黄色脉冲）
- 成功：绿色勾号 + 耗时
- 失败：红色叉号 + 错误摘要（可展开）

---

## 五、交互设计

### 5.1 快捷键体系（Vim 风格）

| 快捷键 | 动作 |
|--------|------|
| `j/k` | 上/下导航 |
| `Enter` | 确认/发送 |
| `Esc` | 取消/返回 |
| `q` | 退出面板 |
| `/` | 搜索 |
| `?` | 帮助 |
| `Ctrl+H` | 帮助 |
| `Ctrl+R` | 重绘 |
| `Ctrl+C` | 中断 |
| `Tab` | 切换面板焦点 |

**层级快捷键**（按面板分组）：
- 团队：`a` 激活 Agent，`d` 详情
- 任务：`n` 新建，`e` 编辑，`c` 完成

### 5.2 命令模式

```
:help          显示帮助
:session       切换会话
:task list     列出任务
:agent invoke  手动触发 Agent
:theme dark    切换主题
```

---

## 六、主题与色彩

### 6.1 色彩语义系统

**色板**（基于终端 22 槽位）：
```rust
struct Theme {
    // 品牌色
    accent: Color,        // 主操作色
    accent_dim: Color,    // 次要强调

    // 文本
    text: Color,          // 正文
    text_dim: Color,     // 弱化文本
    text_bright: Color,  // 强调

    // 状态
    success: Color,       // 成功
    error: Color,        // 错误
    warning: Color,      // 警告
    info: Color,         // 信息

    // 结构
    border: Color,       // 边框
    surface: Color,     // 卡片背景
    background: Color,   // 画布
}
```

### 6.2 兼容性与降级

**必须支持**：
- `NO_COLOR=1`：禁用所有颜色
- 终端 16 色模式：自动降级到 ANSI 16
- 终端 256 色模式：降级到 ANSI 256

**主题方案**：
- 默认跟随终端主题（深色/浅色）
- 支持手动切换：`--theme dark|light`
- 预设主题：Catppuccin, Dracula, Nord, Solarized

---

## 七、Agent 交互流

### 7.1 完整流程

```
用户输入
    ↓
[CEO] 接收 → 分析 → 判断
    ↓
创建/分配 Task
    ↓
[Agent] 思考 → 调用工具 → 输出
    ↓
TUI 渲染：
  ├─ 思考动画（沙漏/旋转）
  ├─ 工具调用卡片（展开/折叠）
  ├─ 流式输出
  └─ Task 状态更新
    ↓
用户确认/干预
    ↓
继续执行或中断
```

### 7.2 权限流

```
Tool Call
    ↓
[检查权限] ──需要确认──→ TUI 弹窗
    │                      ↓
    │                   用户确认
    │                      ↓
    └──直接执行 ←──────────┘
```

**弹窗示例**：
```
┌─ 权限请求 ─────────────────────────────┐
│  agent: engineer                        │
│  工具: bash                            │
│  命令: rm -rf /tmp/build/*             │
│                                        │
│  ⚠️ 危险操作：删除文件                   │
│                                        │
│  [y] 允许一次  [a] 始终允许  [n] 拒绝   │
└────────────────────────────────────────┘
```

---

## 八、技术实现

### 8.1 项目结构

```
loom-tui/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口
│   ├── lib.rs               # 导出
│   ├── model/
│   │   ├── app.rs           # App 状态
│   │   ├── agent.rs         # Agent 状态
│   │   ├── task.rs          # Task 状态
│   │   └── message.rs       # 消息状态
│   ├── view/
│   │   ├── app.rs           # 主视图
│   │   ├── team_panel.rs   # 团队面板
│   │   ├── main_view.rs    # 主区域
│   │   ├── task_panel.rs   # 任务面板
│   │   ├── status_bar.rs   # 状态栏
│   │   └── widgets/        # 组件
│   ├── update/
│   │   ├── mod.rs          # 更新逻辑
│   │   ├── input.rs        # 输入处理
│   │   └── events.rs       # 事件处理
│   └── theme/
│       └── mod.rs          # 主题系统
└── README.md
```

### 8.2 核心依赖

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.29"
tokio = { version = "1", features = ["sync", "rt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
crossterm = { version = "0.29", features = ["dev"] }
```

### 8.3 IPC 设计

```
┌─────────────┐     WebSocket      ┌─────────────┐
│   loom-tui  │ ←───────────────→ │  loom-core  │
│   (TUI)    │                    │   (Agent)  │
└─────────────┘                    └─────────────┘
     │
     ├── 状态同步（Agent 状态、Task 状态）
     ├── 命令发送（用户输入）
     └── 事件接收（流式输出、工具调用）
```

---

## 九、路线图

### Phase 1: MVP（2 周）

- [ ] 基础 TUI 框架搭建（Ratatui + crossterm）
- [ ] 四面板布局实现
- [ ] 静态 Task 列表渲染
- [ ] 基础主题系统
- [ ] 快捷键绑定

### Phase 2: 交互（2 周）

- [ ] 流式输出渲染
- [ ] 工具调用卡片
- [ ] 权限弹窗
- [ ] 命令模式
- [ ] 响应式布局

### Phase 3: 集成（3 周）

- [ ] 与 Loom Agent 通信（WebSocket）
- [ ] 实时状态同步
- [ ] Task CRUD 集成
- [ ] 多 Agent 状态显示
- [ ] 会话管理

### Phase 4: 打磨（2 周）

- [ ] 主题切换
- [ ] 动画优化
- [ ] 性能优化
- [ ] 文档与测试

---

## 十、参考项目

| 项目 | 特点 |
|------|------|
| [lazygit](https://github.com/jesseduffield/lazygit) | 多面板 + Vim 风格 |
| [k9s](https://github.com/derailed/k9s) | Kubernetes 仪表板 |
| [btop](https://github.com/aristocratos/btop) | 系统监控 widget 布局 |
| [gact-tui](https://github.com/iowarp/gact-tui) | Agent 循环 TUI |
| [saorsa-tui](https://github.com/saorsa-labs/saorsa-tui) | Rust AI Agent + CSS 样式 |
| [patchfeld](https://github.com/jimmymills/patchfeld) | Claude 多会话编排 |

---

## 十一、附录：TUI 设计检查清单

### 布局
- [ ] 面板位置固定，用户可预测
- [ ] 响应式降级（< 80 cols 仍可用）
- [ ] 边框风格一致

### 交互
- [ ] 所有操作可通过键盘完成
- [ ] 快捷键不冲突
- [ ] 帮助面板覆盖所有命令

### 色彩
- [ ] 遵循 NO_COLOR 规范
- [ ] 语义颜色一致（成功=绿，错误=红）
- [ ] 前景/背景对比度 ≥ 4.5:1

### 状态
- [ ] 运行中状态有视觉反馈
- [ ] 错误状态清晰可辨
- [ ] 加载状态有 spinner/进度