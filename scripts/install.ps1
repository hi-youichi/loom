param(
    [switch]$Help,
    [string]$Version = $env:LOOM_VERSION,
    [string]$InstallDir = $env:LOOM_INSTALL_DIR,
    [string]$Repository = $env:LOOM_REPO
)

$ErrorActionPreference = 'Stop'

if ($Help) {
    @'
Install Loom from GitHub Releases.

Environment variables:
  LOOM_VERSION       Release tag without the leading v (default: latest)
  LOOM_INSTALL_DIR   Installation directory (default: %LOCALAPPDATA%\loom\bin)
  LOOM_REPO          GitHub repository (default: hi-youichi/loom)
'@
    exit 0
}

if ([string]::IsNullOrWhiteSpace($Version)) { $Version = 'latest' }
if ([string]::IsNullOrWhiteSpace($Repository)) { $Repository = 'hi-youichi/loom' }
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA 'loom\bin'
}

$target = 'x86_64-pc-windows-msvc'
if ($Version -eq 'latest') {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest"
    $tag = $release.tag_name
    $Version = $tag.TrimStart('v')
} else {
    $Version = $Version.TrimStart('v')
    $tag = "v$Version"
}

$archive = "loom-$Version-$target.zip"
$url = "https://github.com/$Repository/releases/download/$tag/$archive"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("loom-install-" + [guid]::NewGuid())
$archivePath = Join-Path $tempDir $archive

try {
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    Write-Host "Downloading Loom $Version for $target..."
    Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing
    Expand-Archive -Path $archivePath -DestinationPath $tempDir -Force

    $binary = Join-Path $tempDir 'loom.exe'
    if (-not (Test-Path -LiteralPath $binary)) {
        throw "release archive does not contain loom.exe"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -LiteralPath $binary -Destination (Join-Path $InstallDir 'loom.exe') -Force
    Write-Host "Loom installed to $(Join-Path $InstallDir 'loom.exe')"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $pathEntries = @($userPath -split ';' | Where-Object { $_ })
    if ($pathEntries -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', (($pathEntries + $InstallDir) -join ';'), 'User')
        Write-Host "Added $InstallDir to the user PATH. Open a new terminal to use loom."
    }
} finally {
    if (Test-Path -LiteralPath $tempDir) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force
    }
}
