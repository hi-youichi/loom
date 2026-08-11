# e2e/run-loom-models-test.ps1
# 运行 Loom Backend Models UI 验证测试
#
# 用法：
#   .\e2e\run-loom-models-test.ps1

$ErrorActionPreference = "Stop"

$LOOM_PORT = 18081
$OPENCHAMBER_PORT = 3200
$LOOM_DIR = "C:\Users\heycj\dev\worktrees\loom\cli-server-backend"
$OPENCHAMBER_DIR = "C:\Users\heycj\dev\openchamber-feat-dev"

Write-Host "=== Loom Backend Models UI Test ===" -ForegroundColor Cyan

# 检查 loom-server 是否在运行
try {
    $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$LOOM_PORT/global/health" -TimeoutSec 2 -ErrorAction Stop
    Write-Host "[OK] loom-server running on port $LOOM_PORT" -ForegroundColor Green
} catch {
    Write-Host "[!] loom-server not running. Starting..." -ForegroundColor Yellow
    Start-Process pwsh -ArgumentList "-NoExit", "-Command", "cd $LOOM_DIR; cargo run -p loom-server -- serve --host 127.0.0.1 --port $LOOM_PORT"
    Write-Host "Waiting for loom-server to start..."
    Start-Sleep -Seconds 10
}

# 检查 openchamber 是否在运行
try {
    $resp = Invoke-WebRequest -Uri "http://localhost:$OPENCHAMBER_PORT" -TimeoutSec 2 -ErrorAction Stop
    Write-Host "[OK] openchamber running on port $OPENCHAMBER_PORT" -ForegroundColor Green
} catch {
    Write-Host "[!] openchamber not running. Starting..." -ForegroundColor Yellow
    Start-Process pwsh -ArgumentList "-NoExit", "-Command", "cd $OPENCHAMBER_DIR; `$env:OPENCHAMBER_PORT='$OPENCHAMBER_PORT'; `$env:OPENCODE_HOST='http://127.0.0.1:$LOOM_PORT'; bun run dev:server"
    Write-Host "Waiting for openchamber to start..."
    Start-Sleep -Seconds 8
}

# 运行 Playwright 测试
Write-Host "`n=== Running Playwright Tests ===" -ForegroundColor Cyan
cd $PSScriptRoot

$env:LOOM_SERVER_URL = "http://127.0.0.1:$LOOM_PORT"
$env:OPENCHAMBER_URL = "http://localhost:$OPENCHAMBER_PORT"

# 禁用自动启动 webServer（我们手动启动了）
$env:E2E_NO_AUTOSTART = "1"

npx playwright test loom-models-ui.spec.ts --reporter=list

Write-Host "`n=== Test Complete ===" -ForegroundColor Cyan
