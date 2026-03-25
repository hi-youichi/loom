# PowerShell Tool 支持方案

## 1. 新增文件

### `loom/src/tools/powershell/mod.rs`
- 完整的 PowerShell tool 实现
- 支持 `pwsh` (PowerShell Core) 和 `powershell` (Windows PowerShell)
- 自动检测可用版本（优先尝试 pwsh，失败回退到 powershell）
- 支持取消操作、超时、工作目录切换
- 高级参数：环境变量、执行策略、使用旧版 PowerShell

## 2. 修改文件

| 文件 | 修改内容 |
|------|---------|
| `loom/src/tools/mod.rs` | 添加 `pub mod powershell` 和 `PowerShellTool` 导出 |
| `loom/src/agent/react/build/tool_source.rs` | 在工具聚合源中注册 PowerShellTool |

## 3. 注册逻辑

```rust
// 在 build_tool_source() 函数中两处注册：
#[cfg(windows)]
aggregate.register_async(Box::new(PowerShellTool::new())).await;
```

**`#[cfg(windows)]` 说明**：
- 仅在 Windows 平台编译此代码
- Unix/macOS 系统不会注册 PowerShell tool
- AI 在非 Windows 环境不会看到 `powershell` 工具选项

## 4. Tool Schema

```json
{
  "name": "powershell",
  "description": "在 Windows 上执行 PowerShell 命令...",
  "parameters": {
    "type": "object",
    "properties": {
      "command": {
        "type": "string",
        "description": "要执行的 PowerShell 命令"
      },
      "workdir": {
        "type": "string",
        "description": "命令执行的工作目录（可选）"
      },
      "timeout_ms": {
        "type": "integer",
        "description": "超时时间（毫秒），默认30秒"
      },
      "env": {
        "type": "object",
        "description": "环境变量键值对（可选）"
      },
      "execution_policy": {
        "type": "string",
        "enum": ["Bypass", "RemoteSigned", "AllSigned", "Restricted", "Undefined"],
        "description": "PowerShell 执行策略（可选）"
      },
      "use_legacy_powershell": {
        "type": "boolean",
        "description": "使用旧版 powershell.exe 而非 pwsh（可选，默认false）"
      }
    },
    "required": ["command"]
  }
}
```

## 5. 与 BashTool 对比

| 特性 | BashTool | PowerShellTool |
|------|---------|---------------|
| 名称 | `bash` | `powershell` |
| 平台 | 跨平台 | Windows only |
| Shell | Unix: `sh`, Windows: `cmd` | `pwsh` / `powershell` |
| 适用场景 | 通用命令、git、npm、docker | WMI、Registry、.NET、COM |
| 取消机制 | 进程 kill | 进程 kill |
| 工作目录 | 支持 | 支持 |
| 环境变量 | 支持 | 支持 |
| 特殊功能 | - | 执行策略、旧版兼容 |

## 6. 使用示例

```rust
// AI 自动选择合适工具

// Windows 系统管理任务：
ToolCall {
    name: "powershell",
    arguments: r#"{"command":"Get-Process | Sort CPU -Desc | Select -First 5"}"#,
}

// WMI 查询：
ToolCall {
    name: "powershell",
    arguments: r#"{"command":"Get-WmiObject Win32_OperatingSystem | Select Caption, Version"}"#,
}

// 注册表操作：
ToolCall {
    name: "powershell",
    arguments: r#"{"command":"Get-ItemProperty HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion | Select ProductName"}"#,
}

// 跨平台兼容任务（非 Windows）：
ToolCall {
    name: "bash",
    arguments: r#"{"command":"git status"}"#,
}
```

## 7. 测试方案

### 7.1 测试文件

**`loom/tests/powershell_tool.rs`**

### 7.2 分层测试架构

测试采用两层设计，确保在非 Windows 平台也能验证基础结构：

**第一层：通用测试（所有平台）**
- 验证 Tool 名称和常量
- 验证 JSON Schema 结构
- 验证工具描述包含 Windows 关键字

**第二层：Windows 特有测试（`#[cfg(windows)]`）**
- 实际命令执行
- 工作目录、环境变量、执行策略
- 真实场景：WMI 查询、注册表读取
- 自动检测逻辑
- 错误处理

### 7.3 测试清单

| 测试类型 | 测试函数名 | 平台 | 说明 |
|---------|-----------|------|------|
| 常量 | `powershell_tool_name_is_correct` | All | 验证 `TOOL_POWERSHELL = "powershell"` |
| Schema | `powershell_tool_spec_has_correct_properties` | All | 验证 name、description、input_schema |
| 描述 | `powershell_tool_description_mentions_windows` | All | 描述包含 "Windows"、"PowerShell"、"WMI" |
| 执行 | `powershell_tool_call_get_location` | Win | 基础命令：Get-Location |
| 目录 | `powershell_tool_call_with_workdir` | Win | workdir 参数生效 |
| 环境 | `powershell_tool_call_with_env_vars` | Win | env 参数传递环境变量 |
| 策略 | `powershell_tool_call_with_execution_policy` | Win | execution_policy 参数生效 |
| 场景 | `powershell_tool_call_wmi_query` | Win | 真实 WMI 查询 |
| 场景 | `powershell_tool_call_registry_read` | Win | 真实注册表读取 |
| 检测 | `powershell_tool_auto_detects_pwsh_or_powershell` | Win | 自动检测可用版本 |
| 错误 | `powershell_tool_call_invalid_command_returns_error` | Win | 无效命令返回错误 |

### 7.4 CI 配置

```yaml
# .github/workflows/test.yml
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Run tests
        run: cargo test
        # powershell_tool 测试：
        # - 非 Windows：只运行通用层测试
        # - Windows：运行全部测试（通用 + Windows）
```

## 8. 配置选项

### 8.1 默认行为

- **Windows 平台**：自动注册 PowerShellTool
- **非 Windows**：不注册，AI 看不到该工具

### 8.2 环境变量控制（可选扩展）

```rust
// 可通过环境变量完全禁用
if env::var("LOOM_DISABLE_POWERSHELL").is_err() {
    aggregate.register_async(Box::new(PowerShellTool::new())).await;
}
```

### 8.3 手动注册（自定义场景）

```rust
use loom::tools::{PowerShellTool, PowerShellToolOptions};

// 自定义配置
let ps_tool = PowerShellTool::with_options(PowerShellToolOptions {
    prefer_legacy: true,  // 优先使用旧版 powershell.exe
    default_timeout_ms: 60000,
});
aggregate.register_async(Box::new(ps_tool)).await;
```

## 9. 故障排查

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| `powershell` 工具不可用 | 在非 Windows 平台 | 使用 `bash` 或其他跨平台工具 |
| 脚本执行被阻止 | 执行策略限制 | 设置 `execution_policy: "Bypass"` |
| 命令在 pwsh 中失败 | 语法差异 | 设置 `use_legacy_powershell: true` |
| 环境变量未生效 | 作用域问题 | 使用 `env` 参数显式传递 |
| 工作目录切换失败 | 路径不存在 | 确保 workdir 存在且可访问 |

## 10. 下一步建议

1. **编译验证**：运行 `cargo check --all-targets`
2. **本地测试**：在 Windows 环境运行 `cargo test powershell_tool`
3. **CI 验证**：确保 GitHub Actions 包含 windows-latest runner
4. **文档更新**：在 user guide 中添加 PowerShell 使用示例
5. **可选扩展**：考虑添加 `ReactBuildConfig.enable_powershell` 开关