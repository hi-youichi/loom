# 标题生成功能测试脚本
# 使用 anureo CLI 测试 session 标题生成

$ErrorActionPreference = "Stop"

# 配置
$Port = 3031
$HomeDir = ".anureo-home"
$PidFile = ".anureo-home/anureo-server.pid"
$LogFile = ".anureo-home/anureo-server.log"

Write-Host "=== 标题生成功能测试 ===" -ForegroundColor Cyan
Write-Host ""

# 检查环境变量
Write-Host "1. 检查环境变量..." -ForegroundColor Yellow
if (-not $env:OPENAI_API_KEY) {
    Write-Error "OPENAI_API_KEY 环境变量未设置"
    exit 1
}
if (-not $env:OPENAI_BASE_URL) {
    Write-Error "OPENAI_BASE_URL 环境变量未设置"
    exit 1
}
Write-Host "   ✓ OPENAI_API_KEY: $($env:OPENAI_API_KEY.Substring(0, 10))..."
Write-Host "   ✓ OPENAI_BASE_URL: $env:OPENAI_BASE_URL"
Write-Host ""

# 创建 .anureo-home 目录（如果不存在）
if (-not (Test-Path $HomeDir)) {
    New-Item -ItemType Directory -Path $HomeDir -Force | Out-Null
    Write-Host "   ✓ 创建 $HomeDir 目录" -ForegroundColor Green
}

# 清理旧的 PID 文件
if (Test-Path $PidFile) {
    Remove-Item $PidFile -Force
    Write-Host "   ✓ 清理旧的 PID 文件" -ForegroundColor Green
}
Write-Host ""

# 启动 anureo server
Write-Host "2. 启动 anureo server..." -ForegroundColor Yellow
Write-Host "   端口: $Port"
Write-Host "   Home: $HomeDir"
Write-Host "   PID 文件: $PidFile"
Write-Host "   日志文件: $LogFile"
Write-Host ""

$serverProcess = Start-Process -FilePath "cargo" -ArgumentList "run", "-p", "cli", "--", "server", "--port", $Port, "--home", $HomeDir, "--pid-file", $PidFile -NoNewWindow -PassThru -RedirectStandardOutput $LogFile -RedirectStandardError $LogFile

# 等待 server 启动
Write-Host "   等待 server 启动..." -NoNewline
$timeout = 30
$elapsed = 0
while ($elapsed -lt $timeout) {
    if (Test-Path $PidFile) {
        Write-Host " ✓" -ForegroundColor Green
        break
    }
    Start-Sleep -Seconds 1
    $elapsed++
    Write-Host "." -NoNewline
}

if ($elapsed -ge $timeout) {
    Write-Host " ✗ 超时" -ForegroundColor Red
    Write-Host "   查看日志: Get-Content $LogFile -Tail 50"
    Stop-Process -Id $serverProcess.Id -Force -ErrorAction SilentlyContinue
    exit 1
}
Write-Host ""

# 显示 server 状态
Write-Host "3. Server 状态检查..." -ForegroundColor Yellow
$pidContent = Get-Content $PidFile
Write-Host "   PID: $pidContent"
Write-Host "   进程存在: $((Get-Process -Id $pidContent -ErrorAction SilentlyContinue) -ne $null)"
Write-Host ""

# 测试说明
Write-Host "4. 测试步骤（需要手动执行）:" -ForegroundColor Yellow
Write-Host ""
Write-Host "   步骤 1: 在新终端中启动 ACP agent"
Write-Host "   ```powershell"
Write-Host "   cargo run -p anureo-cli -- acp"
Write-Host "   ```"
Write-Host ""
Write-Host "   步骤 2: 发送第一轮测试消息，例如："
Write-Host "   '帮我写一个 Hello World 程序'"
Write-Host ""
Write-Host "   步骤 3: 观察 agent 响应完成"
Write-Host ""
Write-Host "   步骤 4: 检查日志中的标题生成信息"
Write-Host "   ```powershell"
Write-Host "   Get-Content $LogFile -Tail 100 | Select-String 'title|Title'"
Write-Host "   ```"
Write-Host ""
Write-Host "   步骤 5: 使用以下命令检查 session 信息（需要安装 sqlite3）"
Write-Host "   ```powershell"
Write-Host "   # 查找 agents.db 文件"
Write-Host "   Get-ChildItem $HomeDir -Filter '*.db'"
Write-Host "   #"
Write-Host "   # 查看最近的 session 和标题"
Write-Host "   # sqlite3 <db-file> 'SELECT session_id, title, created_at FROM acp_sessions ORDER BY created_at DESC LIMIT 5;'"
Write-Host "   ```"
Write-Host ""

# 监控日志
Write-Host "5. 实时监控日志（按 Ctrl+C 停止）..." -ForegroundColor Yellow
Write-Host ""
Write-Host "   日志文件: $LogFile"
Write-Host ""
Write-Host "   按 Ctrl+C 停止监控并关闭 server"
Write-Host ""

try {
    Get-Content $LogFile -Wait -Tail 50 | ForEach-Object {
        $line = $_
        if ($line -match "title|Title") {
            Write-Host $line -ForegroundColor Cyan
        } elseif ($line -match "ERROR|WARN") {
            Write-Host $line -ForegroundColor Red
        } else {
            Write-Host $line
        }
    }
} finally {
    Write-Host ""
    Write-Host "正在关闭 server..." -ForegroundColor Yellow
    Stop-Process -Id $pidContent -Force -ErrorAction SilentlyContinue
    Write-Host "✓ Server 已关闭" -ForegroundColor Green
}
