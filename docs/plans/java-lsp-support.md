# Java LSP 支持实施方案

## 1. 项目背景

### 1.1 现状分析

Loom 项目当前已实现完整的 LSP (Language Server Protocol) 集成，支持多种编程语言的智能代码分析功能。经过代码审查发现，系统在以下层面已经部分支持 Java：

**现有支持情况：**
- ✅ **文件扩展名识别**: `loom/src/lsp/types.rs:329` 已包含 `"java" => "java"` 映射
- ✅ **前端文件图标**: `web/src/components/file-tree/` 已支持 `.java` 文件显示
- ❌ **默认语言服务器配置**: `config/src/lsp_config.rs` 缺少 Java 服务器条目
- ❌ **自动安装器配置**: `loom/src/lsp/installer.rs` 缺少 Java 安装定义

**技术架构：**
```
LSP Tool (loom/src/tools/lsp.rs)
    ↓
LSP Manager (loom/src/lsp/manager.rs)
    ↓
LSP Client (loom/src/lsp/client.rs)
    ↓
Language Server Process (jdtls, rust-analyzer, etc.)
```

### 1.2 需求分析

为 Java 提供完整的 LSP 支持，需要实现：
1. 代码补全
2. 语法错误诊断
3. 跳转到定义
4. 查找引用
5. 悬停信息显示
6. 文档符号导航
7. 项目构建支持 (Maven/Gradle)

## 2. 技术选型

### 2.1 Java 语言服务器对比

| 特性 | Eclipse JDT LS | VS Code Java | Metals | Java Language Server |
|------|----------------|--------------|--------|---------------------|
| **成熟度** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |
| **功能完整度** | 最高 | 基于 JDT LS | Scala 为主 | 基础功能 |
| **社区活跃度** | 高 | 高 | 中 | 低 |
| **启动性能** | 中等 | 中等 | 慢 | 快 |
| **Maven/Gradle** | ✅ 完整支持 | ✅ | ✅ | ⚠️ 部分支持 |
| **调试支持** | ✅ | ✅ | ❌ | ❌ |
| **跨平台** | ✅ | ✅ | ✅ | ✅ |

**推荐选择：Eclipse JDT Language Server**

**理由：**
- VS Code 官方 Java 扩展的底层引擎
- 功能最全面，包含重构、调试等高级特性
- 活跃的社区维护和持续更新
- 完善的 Maven/Gradle 项目集成
- 良好的跨平台支持

### 2.2 安装方式分析

**jdtls 安装挑战：**
- 无统一的包管理器安装方式
- 不同平台需要不同的安装方法
- 首次启动需要下载依赖，启动时间较长

**各平台安装方案：**

| 平台 | 推荐方式 | 备用方案 |
|------|---------|----------|
| **macOS** | `brew install jdtls` | 手动下载 |
| **Linux** | `pip install jdtls` | 手动下载解压 |
| **Windows** | `pip install jdtls` | Chocolatey 安装 |

## 3. 架构设计

### 3.1 系统架构

```
用户请求
    ↓
LSP Tool (代理接口)
    ↓
LSP Manager (多语言管理)
    ↓
Language Detection (*.java → "java")
    ↓
LSP Client (通信管理)
    ↓
jdtls Process (stdio 模式)
    ↓
Java 项目分析
    ├─ Maven 依赖解析
    ├─ Gradle 构建支持  
    ├─ 源码索引
    └─ 类型推断
```

### 3.2 配置架构

```toml
# 用户配置示例
[[servers]]
language = "java"
command = "jdtls"
args = []
file_patterns = ["*.java"]
initialization_options = { settings = { java = { ... } } }
startup_timeout_ms = 30000

[servers.auto_install]
enabled = true
command = "brew install jdtls"  # 平台相关
verify_command = "jdtls --version"
```

### 3.3 启动流程

1. **文件检测**: `manager.rs` 检测 `.java` 文件
2. **语言识别**: `types.rs` 映射到 `"java"` 语言 ID
3. **配置加载**: 从 `LspServerConfig` 加载 jdtls 配置
4. **进程启动**: `client.rs` 启动 jdtls 进程 (stdio 模式)
5. **初始化握手**: 发送 `initialize` 请求，传递项目信息
6. **就绪确认**: 等待服务器发送 `initialized` 通知
7. **功能调用**: 开始处理补全、诊断等请求

## 4. 实施方案

### 4.1 核心代码改动

#### 4.1.1 默认服务器配置 (`config/src/lsp_config.rs`)

**位置**: `get_default_servers()` 函数，约第 212 行后

**新增配置**:
```rust
// Java
LspServerConfig {
    language: "java".to_string(),
    command: "jdtls".to_string(),
    args: vec![],
    file_patterns: vec!["*.java".to_string()],
    initialization_options: Some(serde_json::json!({
        "settings": {
            "java": {
                "configuration": {
                    "updateBuildConfiguration": "interactive"
                },
                "maven": {
                    "downloadSources": true,
                    "updateSnapshots": true
                },
                "gradle": {
                    "enabled": true,
                    "wrapper": { "enabled": true }
                },
                "autobuild": {
                    "enabled": true
                },
                "completion": {
                    "enabled": true,
                    "overwrite": true,
                    "guessMethodArguments": true
                },
                "format": {
                    "enabled": true,
                    "comments": { "enabled": true }
                }
            }
        }
    })),
    root_uri: None,
    env: std::collections::HashMap::new(),
    startup_timeout_ms: 30_000, // Java 服务器启动较慢
    auto_install: Some(AutoInstallConfig {
        enabled: true,
        command: "brew install jdtls".to_string(), // 默认 macOS
        verify_command: Some("jdtls --version".to_string()),
    }),
},
```

#### 4.1.2 安装器定义 (`loom/src/lsp/installer.rs`)

**位置**: `LspInstaller::new()` 方法，约第 111 行后

**新增定义**:
```rust
// Java
ServerDefinition {
    language: "java".to_string(),
    server_name: "eclipse-jdtls".to_string(),
    executable: "jdtls".to_string(),
    check_args: vec!["--version".to_string()],
    install_commands: vec![
        "brew install jdtls".to_string(),          // macOS
        "pip install jdtls".to_string(),           // 跨平台 Python 包
        "choco install jdtls".to_string(),         // Windows
    ],
    package_managers: vec!["brew".to_string(), "pip".to_string(), "choco".to_string()],
},
```

#### 4.1.3 测试用例扩展 (`loom/src/lsp/tests.rs`)

**位置**: `test_detect_language` 函数，约第 19 行

**新增测试**:
```rust
let test_cases = vec![
    ("src/main.rs", "rust"),
    ("src/lib.ts", "typescript"),
    ("app.jsx", "javascript"),
    ("script.py", "python"),
    ("main.go", "go"),
    ("App.java", "java"),  // 新增
];
```

### 4.2 平台适配增强

#### 4.2.1 条件安装支持

为了更好地支持跨平台安装，建议增强 `AutoInstallConfig` 结构：

**文件**: `config/src/lsp_config.rs`

```rust
pub struct AutoInstallConfig {
    pub enabled: bool,
    pub command: String,                              // 默认命令
    pub verify_command: Option<String>,
    pub platform_commands: Option<HashMap<String, String>>, // 新增平台特定命令
}
```

**使用示例**:
```rust
auto_install: Some(AutoInstallConfig {
    enabled: true,
    command: "pip install jdtls".to_string(),           // 默认
    verify_command: Some("jdtls --version".to_string()),
    platform_commands: Some({
        let mut map = HashMap::new();
        map.insert("macos".to_string(), "brew install jdtls".to_string());
        map.insert("linux".to_string(), "pip install jdtls".to_string());
        map.insert("windows".to_string(), "choco install jdtls".to_string());
        map
    }),
}),
```

### 4.3 配置文件示例

**用户自定义配置**: `~/.loom/lsp.toml`

```toml
# Java 语言服务器配置
[[servers]]
language = "java"
command = "jdtls"
args = ["-data", "/path/to/workspace/data"]
file_patterns = ["*.java"]
initialization_options = { settings = { java = { 
    "configuration" = { "updateBuildConfiguration" = "interactive" },
    "maven" = { "downloadSources" = true },
    "gradle" = { "enabled" = true }
}}}

[servers.auto_install]
enabled = true
command = "brew install jdtls"
verify_command = "jdtls --version"
```

## 5. 测试计划

### 5.1 单元测试

| 测试用例 | 描述 | 优先级 |
|---------|------|--------|
| `test_java_language_detection` | 验证 .java 文件正确识别为 Java | 高 |
| `test_java_config_loading` | 验证 Java 服务器配置正确加载 | 高 |
| `test_java_installer_creation` | 验证 Java 安装器定义正确 | 中 |
| `test_java_timeout_value` | 验证启动超时设置为 30s | 中 |

### 5.2 集成测试

| 测试场景 | 描述 | 预期结果 |
|---------|------|----------|
| Java 项目启动 | 在 Maven/Gradle 项目中启动 jdtls | 服务器成功启动并完成初始化 |
| 代码补全 | 在 Java 文件中触发补全 | 返回相关的类型、方法建议 |
| 语法诊断 | 编写包含语法错误的 Java 代码 | 返回准确的错误位置和描述 |
| 跳转定义 | 在方法调用处跳转到定义 | 正确导航到方法定义位置 |
| Maven 项目识别 | 在 pom.xml 项目中打开 Java 文件 | 正确解析 Maven 依赖和结构 |

### 5.3 性能测试

| 指标 | 目标值 | 测量方法 |
|------|--------|----------|
| 首次启动时间 | < 30s | 从进程启动到 `initialized` 通知 |
| 补全响应时间 | < 500ms | 从补全请求到响应返回 |
| 诊断延迟 | < 2s | 文件保存到诊断结果更新 |
| 内存占用 | < 512MB | 稳定运行时的内存使用 |

## 6. 风险评估

### 6.1 技术风险

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| jdtls 启动失败 | 中 | 高 | 提供详细的错误日志和安装指导 |
| 跨平台兼容性问题 | 高 | 中 | 充分测试三大平台，提供备用安装方案 |
| 性能问题 | 中 | 中 | 设置合理的超时时间，提供异步支持 |
| 依赖冲突 | 低 | 中 | 文档说明 Java 版本和环境要求 |

### 6.2 用户体验风险

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| 安装复杂度高 | 高 | 高 | 提供自动安装和详细文档 |
| 首次使用体验差 | 中 | 中 | 添加进度提示和友好的错误信息 |
| 配置复杂 | 中 | 低 | 提供合理的默认配置 |

## 7. 实施时间估算

| 阶段 | 任务 | 预估时间 | 负责人 |
|------|------|----------|--------|
| **阶段一** | 核心代码实现 | | |
| | 默认配置添加 | 2小时 | - |
| | 安装器定义 | 1小时 | - |
| | 测试用例编写 | 1小时 | - |
| **阶段二** | 跨平台适配 | | |
| | 平台条件安装 | 3小时 | - |
| | 三大平台测试 | 4小时 | - |
| **阶段三** | 集成测试 | | |
| | 功能测试 | 4小时 | - |
| | 性能测试 | 2小时 | - |
| **阶段四** | 文档和优化 | | |
| | 用户文档编写 | 2小时 | - |
| | 错误处理优化 | 2小时 | - |
| **总计** | | **21小时** | |

## 8. 验收标准

### 8.1 功能验收

- [ ] Java 文件扩展名正确识别
- [ ] jdtls 服务器能够成功启动
- [ ] 代码补全功能正常工作
- [ ] 语法错误诊断准确显示
- [ ] 跳转定义功能正常
- [ ] Maven/Gradle 项目正确识别

### 8.2 性能验收

- [ ] 首次启动时间 < 30秒
- [ ] 补全响应时间 < 500毫秒
- [ ] 内存占用 < 512MB

### 8.3 用户体验验收

- [ ] 自动安装功能在至少两个平台上正常工作
- [ ] 错误信息清晰友好
- [ ] 配置文档完整易懂

## 9. 后续优化方向

### 9.1 短期优化 (1-2周)

1. **增强错误处理**: 提供更详细的错误诊断和修复建议
2. **进度提示**: 在 jdtls 首次启动时显示加载进度
3. **配置验证**: 启动前验证 Java 环境配置

### 9.2 中期优化 (1-2月)

1. **性能调优**: 缓存机制和增量更新
2. **多项目支持**: 同时管理多个 Java 项目的工作区
3. **调试集成**: 支持Java调试功能

### 9.3 长期优化 (3-6月)

1. **智能安装**: 自动检测最佳安装方式
2. **依赖管理**: 自动处理 Maven/Gradle 依赖
3. **重构支持**: 集成代码重构功能

## 10. 附录

### 10.1 相关文件清单

| 文件路径 | 改动类型 | 行数估算 |
|---------|----------|----------|
| `config/src/lsp_config.rs` | 修改 | +20 |
| `loom/src/lsp/installer.rs` | 修改 | +10 |
| `loom/src/lsp/tests.rs` | 修改 | +2 |
| `docs/guides/java-lsp.md` | 新增 | +200 |

### 10.2 参考资料

- [Eclipse JDT Language Server](https://github.com/eclipse-jdtls/eclipse.jdt.ls)
- [Language Server Protocol Specification](https://microsoft.github.io/language-server-protocol/)
- [VS Code Java Extension](https://code.visualstudio.com/docs/java/java-tutorial)
- [jdtls Installation Guide](https://github.com/eclipse-jdtls/eclipse.jdt.ls#installation)

### 10.3 环境要求

**最低要求：**
- Java 11+
- Maven 3.6+ 或 Gradle 6.0+ (如需项目构建支持)
- 2GB 可用内存

**推荐配置：**
- Java 17+
- Maven 3.8+ 或 Gradle 7.0+
- 4GB 可用内存

---

**文档版本**: v1.0  
**创建日期**: 2025-08-19  
**最后更新**: 2025-08-19  
**维护者**: Loom 开发团队