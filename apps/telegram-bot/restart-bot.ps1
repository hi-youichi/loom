#!/usr/bin/env pwsh
# Telegram Bot 安全重启脚本（裸进程模式）
#
# 安全重启流程：
#   1. 优雅停止旧进程（SIGTERM → 等待 → SIGKILL）
#   2. 等待资源释放
#   3. 启动新进程
#   4. 健康检查验证
#
# 用法：
#   ./restart-bot.ps1                    # 重启默认 bot
#   ./restart-bot.ps1 -Build             # 先 cargo build 再重启

param(
    [switch]$Build
)

set-strictmode -version latest
$ErrorActionPreference = "Stop"

# ── 配置 ──
$ProcessName = "telegram-bot"
$ProjectRoot = Join-Path (Join-Path $PSScriptRoot "..") ".."
$BotBinary = Join-Path $ProjectRoot "target" "release" "telegram-bot"
$GracefulShutdownTimeoutSec = 15
$HealthCheckTimeoutSec = 30

# ── 辅助函数 ──

function Write-Step($msg) {
    Write-Host "`n$msg" -ForegroundColor Cyan
}

function Write-Ok($msg) {
    Write-Host "  ✓ $msg" -ForegroundColor Green
}

function Write-Warn($msg) {
    Write-Host "  ⚠ $msg" -ForegroundColor Yellow
}

function Write-Fail($msg) {
    Write-Host "  ✗ $msg" -ForegroundColor Red
}

function Test-ProcessRunning {
    return [bool](Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
}

function Stop-Graceful {
    $procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
    if (-not $procs) {
        Write-Ok "没有运行中的进程"
        return
    }

    foreach ($proc in $procs) {
        Write-Host "  发送停止信号到 PID $($proc.Id)..." -ForegroundColor Gray
        Stop-Process -Id $proc.Id -ErrorAction SilentlyContinue
    }

    # 等待进程退出
    $deadline = (Get-Date).AddSeconds($GracefulShutdownTimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $remaining = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
        if (-not $remaining) { break }
        Start-Sleep -Milliseconds 500
    }

    # 强制清理残留
    $remaining = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
    if ($remaining) {
        Write-Warn "进程未在 ${GracefulShutdownTimeoutSec}s 内退出，强制终止"
        foreach ($proc in $remaining) {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        }
        Start-Sleep -Seconds 1
    }
    Write-Ok "进程已停止"
}

# ── 主流程 ──

Write-Host "====================================" -ForegroundColor Cyan
Write-Host "  Telegram Bot 安全重启" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan

# 1. 停止旧进程
Write-Step "[1/3] 停止旧进程..."
Stop-Graceful

# 2. 构建并启动
Write-Step "[2/3] 启动新进程..."

if ($Build -or -not (Test-Path $BotBinary)) {
    Write-Host "  cargo build --release ..." -ForegroundColor Gray
    cargo build --release -p telegram-bot --bin telegram-bot 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "cargo build 失败"
        exit 1
    }
}

$psi = @{
    FilePath         = $BotBinary
    WorkingDirectory = $ProjectRoot
    WindowStyle      = "Hidden"
}
Start-Process @psi
Write-Ok "进程已启动"

# 3. 验证
Write-Step "[3/3] 验证..."
Start-Sleep -Seconds 3

if (Test-ProcessRunning) {
    $proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
    Write-Ok "进程运行中 (PID $($proc.Id))"
} else {
    Write-Fail "进程未运行，请检查日志"
    exit 1
}

Write-Host "`n====================================" -ForegroundColor Green
Write-Host "  ✓ 重启完成" -ForegroundColor Green
Write-Host "====================================" -ForegroundColor Green
