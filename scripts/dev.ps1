# Telegram Bot Development Script
# Features: Build, Stop, Start, Restart, Watch mode

param(
    [Parameter(Position=0)]
    [ValidateSet("build", "start", "stop", "restart", "watch", "status")]
    [string]$Action = "start"
)

$ErrorActionPreference = "Stop"
$ProjectName = "telegram-bot"
$PidFile = ".telegram-bot.pid"
$LogFile = "telegram-bot.log"

function Write-Header {
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host " Telegram Bot Development Helper" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""
}

function Get-BotProcess {
    if (Test-Path $PidFile) {
        $pid = Get-Content $PidFile -ErrorAction SilentlyContinue
        if ($pid) {
            $process = Get-Process -Id $pid -ErrorAction SilentlyContinue
            if ($process) {
                return $process
            }
        }
        # PID file exists but process not found, clean up
        Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
    }
    return $null
}

function Stop-Bot {
    $process = Get-BotProcess
    if ($process) {
        Write-Host "Stopping bot (PID: $($process.Id))..." -ForegroundColor Yellow
        
        # Try graceful shutdown first
        $process.CloseMainWindow() | Out-Null
        
        # Wait up to 5 seconds for graceful shutdown
        $timeout = 5
        while ($timeout -gt 0 -and !$process.HasExited) {
            Start-Sleep -Milliseconds 500
            $timeout -= 0.5
            Write-Host "." -NoNewline
        }
        Write-Host ""
        
        # Force kill if still running
        if (!$process.HasExited) {
            Write-Host "Force killing bot..." -ForegroundColor Red
            $process | Stop-Process -Force
            Start-Sleep -Seconds 1
        }
        
        Write-Host "Bot stopped." -ForegroundColor Green
    } else {
        Write-Host "Bot is not running." -ForegroundColor Gray
    }
    
    # Clean up PID file
    if (Test-Path $PidFile) {
        Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
    }
}

function Build-Bot {
    Write-Host "Building $ProjectName (release mode)..." -ForegroundColor Yellow
    
    $result = cargo build -p $ProjectName --release 2>&1
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0) {
        Write-Host "Build successful!" -ForegroundColor Green
        return $true
    } else {
        Write-Host "Build failed!" -ForegroundColor Red
        Write-Host $result
        return $false
    }
}

function Start-Bot {
    $existing = Get-BotProcess
    if ($existing) {
        Write-Host "Bot is already running (PID: $($existing.Id))" -ForegroundColor Yellow
        Write-Host "Use 'restart' to restart it." -ForegroundColor Gray
        return
    }
    
    # Build first
    if (!(Build-Bot)) {
        return
    }
    
    Write-Host "Starting bot..." -ForegroundColor Yellow
    Write-Host "Log file: $LogFile" -ForegroundColor Gray
    Write-Host ""
    
    $exePath = "target\release\$ProjectName.exe"
    
    # Start process with logging
    $process = Start-Process -FilePath $exePath -RedirectStandardOutput $LogFile -RedirectStandardError "$LogFile.err" -PassThru -WindowStyle Hidden
    
    # Save PID
    $process.Id | Out-File $PidFile -Encoding UTF8
    
    # Wait a moment and check if it's still running
    Start-Sleep -Seconds 2
    
    $checkProcess = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
    if ($checkProcess) {
        Write-Host "Bot started successfully (PID: $($process.Id))" -ForegroundColor Green
        Write-Host "Use '.\scripts\dev.ps1 status' to check status" -ForegroundColor Gray
        Write-Host "Use '.\scripts\dev.ps1 stop' to stop" -ForegroundColor Gray
    } else {
        Write-Host "Bot failed to start! Check logs:" -ForegroundColor Red
        Write-Host "  $LogFile" -ForegroundColor Gray
        Write-Host "  $LogFile.err" -ForegroundColor Gray
    }
}

function Restart-Bot {
    Stop-Bot
    Start-Sleep -Seconds 1
    Start-Bot
}

function Show-Status {
    $process = Get-BotProcess
    if ($process) {
        Write-Host "Bot is running" -ForegroundColor Green
        Write-Host "  PID: $($process.Id)" -ForegroundColor Gray
        Write-Host "  CPU: $($process.CPU.ToString('F2'))s" -ForegroundColor Gray
        Write-Host "  Memory: $([math]::Round($process.WorkingSet64 / 1MB, 2)) MB" -ForegroundColor Gray
        Write-Host "  Started: $($process.StartTime)" -ForegroundColor Gray
        
        if (Test-Path $LogFile) {
            Write-Host ""
            Write-Host "Recent logs:" -ForegroundColor Cyan
            Get-Content $LogFile -Tail 10
        }
    } else {
        Write-Host "Bot is not running" -ForegroundColor Yellow
    }
}

function Watch-Mode {
    Write-Host "Watch mode: Auto-rebuild and restart on file changes" -ForegroundColor Cyan
    Write-Host "Press Ctrl+C to stop" -ForegroundColor Gray
    Write-Host ""
    
    # Initial build and start
    Restart-Bot
    
    # Watch for changes
    $watcher = New-Object System.IO.FileSystemWatcher
    $watcher.Path = (Resolve-Path ".").Path
    $watcher.IncludeSubdirectories = $true
    $watcher.Filter = "*.rs"
    $watcher.EnableRaisingEvents = $true
    
    $lastBuild = [DateTime]::Now
    $debounceSeconds = 3
    
    $onChange = Register-ObjectEvent $watcher -EventName Changed -Action {
        param($sender, $event)
        
        $now = [DateTime]::Now
        $diff = ($now - $Event.MessageData.LastBuild).TotalSeconds
        
        if ($diff -ge $Event.MessageData.DebounceSeconds) {
            $Event.MessageData.LastBuild = $now
            Write-Host "`n[$(Get-Date -Format 'HH:mm:ss')] File changed: $($event.FullPath)" -ForegroundColor Yellow
            
            # Restart (which includes rebuild)
            & "$PSScriptRoot\dev.ps1" restart
        }
    } -MessageData @{ LastBuild = $lastBuild; DebounceSeconds = $debounceSeconds }
    
    Write-Host "Watching for .rs file changes..." -ForegroundColor Green
    
    # Keep running until Ctrl+C
    try {
        while ($true) {
            Start-Sleep -Seconds 1
        }
    } finally {
        Unregister-Event -SourceIdentifier $onChange.Name -ErrorAction SilentlyContinue
        $watcher.Dispose()
        Stop-Bot
    }
}

# Main
Write-Header

switch ($Action) {
    "build" { Build-Bot }
    "start" { Start-Bot }
    "stop" { Stop-Bot }
    "restart" { Restart-Bot }
    "watch" { Watch-Mode }
    "status" { Show-Status }
}
