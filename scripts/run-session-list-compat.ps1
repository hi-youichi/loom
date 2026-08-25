param(
  [Parameter(Mandatory = $true)]
  [string]$NewanureoBinary,
  [string]$OldanureoBinary,
  [string]$OutputDir = (Join-Path (Get-Location) "session-list-compat-results"),
  [string]$TestName = "e2e_session_list"
)

$ErrorActionPreference = "Stop"

function Resolve-ExistingFile([string]$Path, [string]$Label) {
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
  if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "$Label is not a file: $resolved"
  }
  return $resolved
}

function Get-BinaryIdentity([string]$Path) {
  $file = Get-Item -LiteralPath $Path -ErrorAction Stop
  $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
  return [ordered]@{
    sha256 = $hash
    sizeBytes = [int64]$file.Length
  }
}

$newBinary = Resolve-ExistingFile $NewanureoBinary "New anureo binary"
$oldBinary = if ($OldanureoBinary) {
  Resolve-ExistingFile $OldanureoBinary "Old anureo binary"
} else { $null }

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDir).Path
$runs = @(@{ name = "new-anureo"; binary = $newBinary })
if ($oldBinary) { $runs += @{ name = "old-anureo"; binary = $oldBinary } }

$previousBinary = $env:ANUREO_ACP_BINARY
$previousLegacyExpectation = $env:ANUREO_SESSION_LIST_EXPECT_LEGACY
$results = [System.Collections.Generic.List[object]]::new()
$seenBinaryHashes = [System.Collections.Generic.HashSet[string]]::new()
try {
  foreach ($run in $runs) {
    $logPath = Join-Path $resolvedOutput "$($run.name).log"
    $started = [DateTime]::UtcNow
    $env:ANUREO_ACP_BINARY = $run.binary
    if ($run.name -eq "old-anureo") {
      $env:ANUREO_SESSION_LIST_EXPECT_LEGACY = "1"
    } else {
      Remove-Item Env:ANUREO_SESSION_LIST_EXPECT_LEGACY -ErrorAction SilentlyContinue
    }
    Write-Host "[compat] $($run.name): $($run.binary)"
    Write-Host "[compat] output: $logPath"

    $captured = @(
      & cargo test -p anureo-acp --test $TestName -- --nocapture 2>&1 |
        Tee-Object -FilePath $logPath
    )
    $exitCode = $LASTEXITCODE
    $joinedOutput = $captured -join "`n"
    $testMatch = [regex]::Match($joinedOutput, "running\s+(\d+)\s+tests?")
    $testCount = if ($testMatch.Success) { [int]$testMatch.Groups[1].Value } else { 0 }
    $identity = Get-BinaryIdentity $run.binary
    $sameBinaryAsEarlierRun = $seenBinaryHashes.Contains($identity.sha256)
    [void]$seenBinaryHashes.Add($identity.sha256)
    $results.Add([ordered]@{
        name = $run.name
        expectedMode = if ($run.name -eq "old-anureo") { "legacy" } else { "canonical" }
        binary = $run.binary
        binarySha256 = $identity.sha256
        binarySizeBytes = $identity.sizeBytes
        sameBinaryAsEarlierRun = $sameBinaryAsEarlierRun
        startedUtc = $started.ToString("o")
        finishedUtc = [DateTime]::UtcNow.ToString("o")
        exitCode = $exitCode
        testCount = $testCount
        log = $logPath
        osDescription = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
        osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        processArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        powershell = $PSVersionTable.PSVersion.ToString()
      })
    if ($exitCode -ne 0) {
      throw "compatibility run failed for $($run.name) with exit code $exitCode"
    }
    if ($testCount -lt 1) {
      throw "compatibility run executed zero tests for $($run.name); inspect $logPath"
    }
  }
}
finally {
  if ($null -eq $previousBinary) {
    Remove-Item Env:ANUREO_ACP_BINARY -ErrorAction SilentlyContinue
  } else {
    $env:ANUREO_ACP_BINARY = $previousBinary
  }
  if ($null -eq $previousLegacyExpectation) {
    Remove-Item Env:ANUREO_SESSION_LIST_EXPECT_LEGACY -ErrorAction SilentlyContinue
  } else {
    $env:ANUREO_SESSION_LIST_EXPECT_LEGACY = $previousLegacyExpectation
  }
  $manifest = Join-Path $resolvedOutput "manifest.json"
  $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifest -Encoding utf8
  Write-Host "[compat] manifest: $manifest"
}

if (($results | Where-Object { $_.exitCode -ne 0 }).Count -gt 0) { exit 1 }
exit 0
