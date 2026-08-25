param(
  [Parameter(Mandatory=$true)]
  [string[]]$ExtensionIds
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$hostDir = Join-Path $scriptDir "host"
$hostName = "com.anureo.browser"
$wrapperPath = Join-Path $hostDir "native-host-wrapper.bat"

$nodePath = (Get-Command node -ErrorAction Stop).Source
$hostDirAbsolute = (Resolve-Path $hostDir).Path
$nativeHostJs = Join-Path $hostDirAbsolute "native-host.js"

Set-Content -Path $wrapperPath -Value "@echo off`n`"$nodePath`" `"$nativeHostJs`""

$origins = ($ExtensionIds | ForEach-Object { "chrome-extension://$_/" }) -join "`n    "

$manifestJson = @"
{
  "name": "$hostName",
  "description": "anureo Browser Extension Native Messaging Host",
  "path": "$($wrapperPath.Replace('\','\\'))",
  "type": "stdio",
  "allowed_origins": [
    $($origins -replace '(?m)^(.+)$', '"$1"' -replace "`n", ",`n    ")
  ]
}
"@

$manifestPath = Join-Path $hostDir "$hostName.json"
Set-Content -Path $manifestPath -Value $manifestJson -Encoding UTF8
Write-Host "Created manifest: $manifestPath"

function Register-NativeHost {
  param([string]$BrowserName, [string]$RegKeyPath)

  $parentPath = Split-Path $RegKeyPath -Parent
  if (-not (Test-Path $parentPath)) {
    New-Item -Path $parentPath -Force | Out-Null
  }
  if (-not (Test-Path $RegKeyPath)) {
    New-Item -Path $RegKeyPath -Force | Out-Null
  }
  Set-ItemProperty -Path $RegKeyPath -Name "(Default)" -Value $manifestPath
  Write-Host "Registered for $BrowserName"
}

Register-NativeHost "Google Chrome" "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$hostName"
Register-NativeHost "Microsoft Edge" "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$hostName"
Register-NativeHost "Brave Browser" "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\$hostName"

Write-Host ""
Write-Host "Done! Next steps:"
Write-Host "  1. Restart your browser (close all windows)"
Write-Host "  2. Start anureo - it will auto-connect via .anureo/mcp.json"
