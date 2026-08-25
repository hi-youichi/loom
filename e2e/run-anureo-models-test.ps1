# e2e/run-anureo-models-test.ps1
# 运行 anureo Backend Models UI 验证测试
#
# 用法：
#   .\e2e\run-anureo-models-test.ps1

$ErrorActionPreference = "Stop"

$ANUREO_PORT = 18081
$ANUREO_PORT = 3200
$ANUREO_DIR = "C:\Users\heycj\dev\worktrees\anureo\cli-server-backend"
$ANUREO_DIR = "C:\Users\heycj\dev\anureo-feat-dev"

Write-Host "=== anureo Backend Models UI Test ===" -ForegroundColor Cyan

# 检查 anureo-server 是否在运行
try {
    $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$ANUREO_PORT/global/health" -TimeoutSec 2 -ErrorAction Stop
    Write-Host "[OK] anureo-server running on port $ANUREO_PORT" -ForegroundColor Green
} catch {
    Write-Host "[!] anureo-server not running. Starting..." -ForegroundColor Yellow
    Start-Process pwsh -ArgumentList "-NoExit", "-Command", "cd $ANUREO_DIR; cargo run -p anureo-server -- serve --host 127.0.0.1 --port $ANUREO_PORT"
    Write-Host "Waiting for anureo-server to start..."
    Start-Sleep -Seconds 10
}

# 检查 anureo 是否在运行
try {
    $resp = Invoke-WebRequest -Uri "http://localhost:$ANUREO_PORT" -TimeoutSec 2 -ErrorAction Stop
    Write-Host "[OK] anureo running on port $ANUREO_PORT" -ForegroundColor Green
} catch {
    Write-Host "[!] anureo not running. Starting..." -ForegroundColor Yellow
    Start-Process pwsh -ArgumentList "-NoExit", "-Command", "cd $ANUREO_DIR; `$env:ANUREO_PORT='$ANUREO_PORT'; `$env:OPENCODE_HOST='http://127.0.0.1:$ANUREO_PORT'; bun run dev:server"
    Write-Host "Waiting for anureo to start..."
    Start-Sleep -Seconds 8
}

# 运行 Playwright 测试
Write-Host "`n=== Running Playwright Tests ===" -ForegroundColor Cyan
cd $PSScriptRoot

$env:ANUREO_SERVER_URL = "http://127.0.0.1:$ANUREO_PORT"
$env:ANUREO_URL = "http://localhost:$ANUREO_PORT"

# 禁用自动启动 webServer（我们手动启动了）
$env:E2E_NO_AUTOSTART = "1"

npx playwright test anureo-models-ui.spec.ts --reporter=list

Write-Host "`n=== Test Complete ===" -ForegroundColor Cyan
