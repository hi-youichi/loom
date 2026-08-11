[CmdletBinding()]
param(
    [string]$BaseUrl = "",
    [int]$Port = 18080,
    [switch]$NoBoot,
    [string]$Authorization = "Basic dXNlcjp0ZXN0"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($env:LOOM_PROTOCOL_NO_BOOT -eq "1") {
    $NoBoot = $true
}
if (-not $BaseUrl) {
    $BaseUrl = "http://127.0.0.1:$Port"
}

$server = $null
$stdoutLog = Join-Path ([IO.Path]::GetTempPath()) "loom-server-protocol-$PID.stdout.log"
$stderrLog = Join-Path ([IO.Path]::GetTempPath()) "loom-server-protocol-$PID.stderr.log"
$sessionId = $null

function Invoke-ProtocolRequest {
    param(
        [ValidateSet("GET", "POST", "PATCH", "PUT", "DELETE")]
        [string]$Method,
        [string]$Path,
        [object]$Body = $null,
        [int[]]$Expected = @(200, 204)
    )

    $arguments = @("-sS", "-o", "-", "-w", "`n%{http_code}", "-X", $Method)
    if ($Authorization) {
        $arguments += @("-H", "Authorization: $Authorization")
    }
    if ($null -ne $Body) {
        $arguments += @("-H", "Content-Type: application/json", "--data", ($Body | ConvertTo-Json -Compress -Depth 20))
    }
    $arguments += "$BaseUrl$Path"

    $raw = (& curl.exe @arguments 2>&1) -join "`n"
    if ($LASTEXITCODE -ne 0) {
        throw "$Method $Path transport failure (curl exit $LASTEXITCODE): $raw"
    }
    $lines = $raw -split "`r?`n"
    $status = [int]$lines[-1]
    $responseBody = ($lines[0..([Math]::Max(0, $lines.Length - 2))] -join "`n").Trim()
    if ($status -notin $Expected) {
        throw "$Method $Path returned HTTP $status; body=$responseBody"
    }

    $json = $null
    if ($responseBody) {
        try { $json = $responseBody | ConvertFrom-Json } catch { }
    }
    [pscustomobject]@{ Status = $status; Body = $responseBody; Json = $json }
}

function Wait-ForServer {
    $deadline = [DateTime]::UtcNow.AddSeconds(180)
    do {
        if ($server -and $server.HasExited) {
            $stderr = if (Test-Path $stderrLog) { Get-Content $stderrLog -Raw } else { "" }
            throw "loom-server exited before becoming healthy: $stderr"
        }
        try {
            $health = Invoke-ProtocolRequest -Method GET -Path "/api/health" -Expected @(200)
            if ($health.Json.ok -eq $true) { return }
        } catch {
            Start-Sleep -Milliseconds 500
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "loom-server did not become healthy within 180 seconds"
}

function Assert-SseEnvelope {
    param([string]$Path, [ValidateSet("v1", "v2")] [string]$Kind)

    $arguments = @("-sN", "--max-time", "3")
    if ($Authorization) { $arguments += @("-H", "Authorization: $Authorization") }
    $arguments += "$BaseUrl$Path"
    $output = (& curl.exe @arguments 2>$null) -join "`n"
    # curl exit 28 is expected because an SSE connection is intentionally open.
    if ($LASTEXITCODE -notin @(0, 28)) {
        throw "SSE $Path transport failure (curl exit $LASTEXITCODE)"
    }
    $dataLine = ($output -split "`r?`n" | Where-Object { $_ -like "data:*" } | Select-Object -First 1)
    if (-not $dataLine) { throw "SSE $Path emitted no data frame" }
    $event = $dataLine.Substring(5).Trim() | ConvertFrom-Json
    if ($Kind -eq "v1") {
        if (-not $event.directory -or -not $event.payload.type) {
            throw "SSE $Path did not emit the v1 {directory,payload} envelope"
        }
    } elseif (-not $event.payload.type -or -not $event.payload.id) {
        throw "SSE $Path did not emit the v2 payload envelope"
    }
}

try {
    if (-not $NoBoot) {
        $server = Start-Process -FilePath "cargo" -ArgumentList @(
            "run", "-p", "loom-server", "--", "serve", "--host", "127.0.0.1", "--port", "$Port"
        ) -WorkingDirectory (Get-Location) -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog -PassThru
    }
    Wait-ForServer

    # P0 bootstrap and health gates (v1 + rollout-v2).
    foreach ($path in @(
        "/config", "/config/providers", "/provider", "/agent", "/path", "/project/current",
        "/command", "/mcp", "/mcp/status", "/lsp", "/formatter", "/session/status",
        "/provider/auth", "/experimental/resource/list", "/vcs",
        "/experimental/capabilities", "/api/health", "/api/location", "/api/path",
        "/api/app/agent", "/api/app/model", "/api/app/provider", "/global/health"
    )) {
        [void](Invoke-ProtocolRequest -Method GET -Path $path)
    }

    Assert-SseEnvelope -Path "/global/event" -Kind v1
    Assert-SseEnvelope -Path "/api/event" -Kind v2

    # Minimal stateful session interaction, deliberately avoiding an LLM call.
    $created = Invoke-ProtocolRequest -Method POST -Path "/session" -Body @{ title = "protocol gate" }
    $sessionId = [string]$created.Json.id
    if (-not $sessionId.StartsWith("sess_")) { throw "session create returned an invalid id" }
    [void](Invoke-ProtocolRequest -Method PATCH -Path "/session/$sessionId" -Body @{ title = "protocol gate updated" })
    [void](Invoke-ProtocolRequest -Method GET -Path "/session/$sessionId")
    [void](Invoke-ProtocolRequest -Method GET -Path "/session/$sessionId/message")
    $shell = Invoke-ProtocolRequest -Method POST -Path "/session/$sessionId/shell" -Body @{ command = "echo loom-shell" }
    if ($shell.Body -notlike "*loom-shell*") { throw "session shell did not return command output" }
    [void](Invoke-ProtocolRequest -Method POST -Path "/session/$sessionId/abort" -Body @{})
    [void](Invoke-ProtocolRequest -Method GET -Path "/api/session/$sessionId/event")

    # P2 stub surface must resolve with its current SDK method.
    [void](Invoke-ProtocolRequest -Method GET -Path "/permission")
    [void](Invoke-ProtocolRequest -Method GET -Path "/question")
    [void](Invoke-ProtocolRequest -Method PATCH -Path "/mcp" -Body @{})
    [void](Invoke-ProtocolRequest -Method POST -Path "/mcp/protocol/connect" -Body @{})
    [void](Invoke-ProtocolRequest -Method GET -Path "/pty")
    [void](Invoke-ProtocolRequest -Method GET -Path "/file/status")
    [void](Invoke-ProtocolRequest -Method GET -Path "/find?pattern=src")
    [void](Invoke-ProtocolRequest -Method GET -Path "/experimental/resource/protocol")
    [void](Invoke-ProtocolRequest -Method POST -Path "/provider/auth" -Body @{ providerID = "protocol" })
    [void](Invoke-ProtocolRequest -Method POST -Path "/api/instance" -Body @{})
    [void](Invoke-ProtocolRequest -Method PUT -Path "/api/location/workspace" -Body @{})
    [void](Invoke-ProtocolRequest -Method POST -Path "/api/mcp" -Body @{})
    [void](Invoke-ProtocolRequest -Method GET -Path "/api/experimental/app")

    [void](Invoke-ProtocolRequest -Method GET -Path "/global/version")
    [void](Invoke-ProtocolRequest -Method DELETE -Path "/session/$sessionId" -Expected @(204))
    $sessionId = $null
    Write-Host "Protocol gate passed: bootstrap, SSE v1/v2, session CRUD, auth pass-through, and P2 routes."
}
finally {
    if ($sessionId) {
        try { [void](Invoke-ProtocolRequest -Method DELETE -Path "/session/$sessionId" -Expected @(204, 404)) } catch { }
    }
    if ($server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
        $server.WaitForExit()
    }
    Remove-Item $stdoutLog, $stderrLog -Force -ErrorAction SilentlyContinue
}
