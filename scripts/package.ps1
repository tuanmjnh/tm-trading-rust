#Requires -Version 5.1
<#
.SYNOPSIS
    Universal Multi-Platform Packaging Tool for TM Trading Rust.
    Supports Windows (PowerShell 5.1/7+), Linux (pwsh), and macOS (pwsh).
.DESCRIPTION
    Builds workspace release binaries with cargo, collects executables,
    configuration templates, and launch scripts, packages them into an
    optimized archive (.zip on Windows, .tar.gz on Linux/macOS), and calculates
    the SHA-256 integrity checksum.
.PARAMETER SkipBuild
    Skip cargo build --workspace --release if binaries are already freshly compiled.
#>

[CmdletBinding()]
param (
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  TM Trading Rust - Universal Multi-Platform Packager" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Detect Operating System and Architecture
$isWin = $false
$isMac = $false
$isLinux = $false

if ($PSVersionTable.PSEdition -eq "Core") {
    $isWin = $IsWindows
    $isMac = $IsMacOS
    $isLinux = $IsLinux
} else {
    $isWin = ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
$archNormalized = switch ($arch) {
    "x64" { "x64" }
    "arm64" { "arm64" }
    default { $arch }
}

if ($isWin) {
    $platform = "windows-$archNormalized"
    $exeExt = ".exe"
    $archiveExt = "zip"
} elseif ($isMac) {
    $platform = "macos-$archNormalized"
    $exeExt = ""
    $archiveExt = "tar.gz"
} else {
    $platform = "linux-$archNormalized"
    $exeExt = ""
    $archiveExt = "tar.gz"
}

Write-Host "  Detected Platform: $platform" -ForegroundColor Green

# 2. Extract version from Cargo.toml
$cargoToml = Get-Content (Join-Path $repoRoot "Cargo.toml") -Raw
if ($cargoToml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
    $version = $matches[1]
} else {
    $version = "0.1.0"
}
Write-Host "  Target Release Version: v$version" -ForegroundColor Green

# 3. Compile release binaries if requested
if (-not $SkipBuild) {
    Write-Host "[1/5] Compiling release binaries (cargo build --workspace --release)..." -ForegroundColor Yellow
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo build failed with exit code $LASTEXITCODE."
    }
} else {
    Write-Host "[1/5] Skipping build step (-SkipBuild specified)..." -ForegroundColor Yellow
}

# 4. Setup staging directory
$distDir = Join-Path $repoRoot "dist"
$stageDir = Join-Path $distDir "tm-trading-$platform"
$archiveName = "tm-trading-$platform-v$version.$archiveExt"
$archivePath = Join-Path $distDir $archiveName
$shaPath = Join-Path $distDir "$archiveName.sha256"

if (Test-Path $stageDir) {
    Remove-Item -Recurse -Force $stageDir
}
New-Item -ItemType Directory -Force (Join-Path $stageDir "bin") | Out-Null
New-Item -ItemType Directory -Force (Join-Path $stageDir "data") | Out-Null

# 5. Collect executables
Write-Host "[2/5] Collecting release binaries..." -ForegroundColor Yellow
$binaries = @(
    "tm-trading-desktop",
    "tm-trading-management-api",
    "tm-trading-agent",
    "tm-trading-market-worker",
    "tm-trading-news-worker",
    "tm-trading-ai-worker",
    "tm-trading-notification-worker",
    "tm-trading-research-worker",
    "tm-trading-sample-worker"
)

$targetRelease = Join-Path $repoRoot "target\release"
foreach ($bin in $binaries) {
    $binFile = "$bin$exeExt"
    $src = Join-Path $targetRelease $binFile
    if (Test-Path $src) {
        Copy-Item -Path $src -Destination (Join-Path $stageDir "bin") -Force
        Write-Host "  + Included: $binFile" -ForegroundColor Gray
    } else {
        Write-Warning "Binary not found in release folder: $binFile"
    }
}

# 6. Add configuration template & launch scripts
Write-Host "[3/5] Adding launch scripts and configuration templates..." -ForegroundColor Yellow

$envExample = @"
# TM Trading Desktop & Local Services Configuration
TM_TRADING_API_URL=http://127.0.0.1:8787
TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token
TM_TRADING_AGENT_API_TOKEN=tm-local-agent-token
TM_TRADING_LOCALE=vi
TM_LOG_FORMAT=compact
TM_TRADING_DATA_DIR=data
"@
Set-Content -Path (Join-Path $stageDir ".env.example") -Value $envExample -Encoding UTF8

$readmeTxt = @"
========================================================================
  TM Trading Desktop Control Center & Services ($platform)
  Version: v$version
========================================================================

SYSTEM PREREQUISITES:
  - Windows: Windows 10/11 x64 with Microsoft Edge WebView2 Evergreen Runtime.
  - Linux: webkit2gtk-4.1 / gtk3 packages installed.
  - macOS: macOS 11+ (Big Sur or newer).

QUICK START:
  1. Copy '.env.example' to '.env' and adjust tokens if desired.
  2. Launch services and desktop GUI using the provided start script.

SECURITY NOTICE:
  - This application operates strictly read-only and information-first.
  - Live order execution and broker credentials storage are intentionally omitted.
  - Management API binds strictly to loopback (127.0.0.1).
========================================================================
"@
Set-Content -Path (Join-Path $stageDir "README.txt") -Value $readmeTxt -Encoding UTF8

if ($isWin) {
    $startAllCmd = @"
@echo off
title TM Trading Desktop & Services
setlocal
cd /d "%~dp0"
if exist ".env" (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        set "%%A=%%B"
    )
)
if "%TM_TRADING_API_URL%"=="" set "TM_TRADING_API_URL=http://127.0.0.1:8787"
if "%TM_TRADING_LOCAL_API_TOKEN%"=="" set "TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token"

echo Starting Management API Server in background...
start "TM Trading Management API" /min "%~dp0bin\tm-trading-management-api.exe"
timeout /t 2 /nobreak >nul
echo Launching TM Trading Control Center...
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
"@
    Set-Content -Path (Join-Path $stageDir "start-all-services.cmd") -Value $startAllCmd -Encoding ASCII

    $startDesktopCmd = @"
@echo off
title TM Trading Control Center
setlocal
cd /d "%~dp0"
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
"@
    Set-Content -Path (Join-Path $stageDir "start-desktop-only.cmd") -Value $startDesktopCmd -Encoding ASCII
} else {
    $startAllSh = @"
#!/usr/bin/env bash
set -e
DIR="`$(cd "`$(dirname "`"${BASH_SOURCE[0]}`")" && pwd)"
cd "`$DIR"
if [ -f ".env" ]; then
  export `$(grep -v '^#' .env | xargs)
fi
export TM_TRADING_API_URL="`${TM_TRADING_API_URL:-http://127.0.0.1:8787}"
export TM_TRADING_LOCAL_API_TOKEN="`${TM_TRADING_LOCAL_API_TOKEN:-tm-local-dev-token}"
echo "Starting Management API server in background..."
"`$DIR/bin/tm-trading-management-api" > "`$DIR/api.log" 2>&1 &
sleep 2
echo "Launching TM Trading Desktop..."
"`$DIR/bin/tm-trading-desktop"
"@
    Set-Content -Path (Join-Path $stageDir "start-all-services.sh") -Value $startAllSh -Encoding UTF8

    $startDesktopSh = @"
#!/usr/bin/env bash
set -e
DIR="`$(cd "`$(dirname "`"${BASH_SOURCE[0]}`")" && pwd)"
"`$DIR/bin/tm-trading-desktop"
"@
    Set-Content -Path (Join-Path $stageDir "start-desktop-only.sh") -Value $startDesktopSh -Encoding UTF8
}

# 7. Create Archive and Calculate SHA-256
Write-Host "[4/5] Creating distribution archive ($archiveName)..." -ForegroundColor Yellow
if (Test-Path $archivePath) {
    Remove-Item -Force $archivePath
}

if ($archiveExt -eq "zip") {
    Compress-Archive -Path "$stageDir\*" -DestinationPath $archivePath -CompressionLevel Optimal
} else {
    tar -czf $archivePath -C $distDir "tm-trading-$platform"
}

Write-Host "[5/5] Calculating SHA-256 checksum..." -ForegroundColor Yellow
$hash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash
Set-Content -Path $shaPath -Value "$hash  $archiveName" -Encoding ASCII

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  Packaging Complete!" -ForegroundColor Green
Write-Host "  Platform: $platform" -ForegroundColor White
Write-Host "  Archive:  $archivePath" -ForegroundColor White
Write-Host "  SHA-256:  $hash" -ForegroundColor White
Write-Host "  Checksum: $shaPath" -ForegroundColor White
Write-Host "==========================================================" -ForegroundColor Cyan
