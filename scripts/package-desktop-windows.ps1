#Requires -Version 5.1
<#
.SYNOPSIS
    Packages TM Trading Rust Desktop & Services distribution for Windows x64.
.DESCRIPTION
    Builds the release binaries with cargo, collects required executables,
    configuration templates, and launch scripts, packages them into a clean
    zip archive, and calculates the SHA-256 integrity checksum.
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
Write-Host "  TM Trading Rust - Windows Desktop Packaging Tool" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Build release binaries if requested
if (-not $SkipBuild) {
    Write-Host "[1/5] Compiling release binaries (cargo build --workspace --release)..." -ForegroundColor Yellow
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo build failed with exit code $LASTEXITCODE."
    }
} else {
    Write-Host "[1/5] Skipping build step (-SkipBuild specified)..." -ForegroundColor Yellow
}

# 2. Extract version from Cargo.toml
$cargoToml = Get-Content (Join-Path $repoRoot "Cargo.toml") -Raw
if ($cargoToml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
    $version = $matches[1]
} else {
    $version = "0.1.0"
}
Write-Host "  Target distribution version: v$version" -ForegroundColor Green

# 3. Setup distribution staging directory
$distDir = Join-Path $repoRoot "dist"
$stageDir = Join-Path $distDir "tm-trading-desktop-windows-x64"
$zipName = "tm-trading-desktop-windows-x64-v$version.zip"
$zipPath = Join-Path $distDir $zipName
$shaPath = Join-Path $distDir "$zipName.sha256"

if (Test-Path $stageDir) {
    Remove-Item -Recurse -Force $stageDir
}
New-Item -ItemType Directory -Force (Join-Path $stageDir "bin") | Out-Null
New-Item -ItemType Directory -Force (Join-Path $stageDir "data") | Out-Null

Write-Host "[2/5] Collecting binaries into staging directory..." -ForegroundColor Yellow
$binaries = @(
    "tm-trading-desktop.exe",
    "tm-trading-management-api.exe",
    "tm-trading-agent.exe",
    "tm-trading-market-worker.exe",
    "tm-trading-news-worker.exe",
    "tm-trading-ai-worker.exe",
    "tm-trading-notification-worker.exe",
    "tm-trading-research-worker.exe",
    "tm-trading-sample-worker.exe"
)

$targetRelease = Join-Path $repoRoot "target\release"
foreach ($bin in $binaries) {
    $src = Join-Path $targetRelease $bin
    if (Test-Path $src) {
        Copy-Item -Path $src -Destination (Join-Path $stageDir "bin") -Force
        Write-Host "  + Included: $bin" -ForegroundColor Gray
    } else {
        Write-Warning "Binary not found in release folder: $bin"
    }
}

# 4. Create configuration template & launch scripts
Write-Host "[3/5] Adding launch scripts and configuration templates..." -ForegroundColor Yellow

$envExample = @"
# TM Trading Desktop & Local Services Configuration
# Loopback management API endpoint:
TM_TRADING_API_URL=http://127.0.0.1:8787

# Authentication bearer token for local user operations:
TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token

# Agent bearer token for supervised worker lifecycle:
TM_TRADING_AGENT_API_TOKEN=tm-local-agent-token

# Desktop locale: 'en' for English, 'vi' for Tiếng Việt:
TM_TRADING_LOCALE=vi

# Logging format ('compact' or 'json'):
TM_LOG_FORMAT=compact

# Redb database directory:
TM_TRADING_DATA_DIR=data
"@
Set-Content -Path (Join-Path $stageDir ".env.example") -Value $envExample -Encoding UTF8

$readmeTxt = @"
========================================================================
  TM Trading Desktop Control Center & Services (Windows x64)
  Version: v$version
========================================================================

SYSTEM REQUIREMENTS:
  1. Windows 10 (Build 1809+) or Windows 11 64-bit.
  2. Microsoft Edge WebView2 Evergreen Runtime.
     If missing, download bootstrapper:
     https://go.microsoft.com/fwlink/p/?LinkId=2124703

QUICK START:
  1. Copy '.env.example' to '.env' and adjust tokens if desired.
  2. Double click 'start-all-services.cmd' to launch Management API and Desktop.
  3. Or double click 'start-desktop-only.cmd' to connect to an existing server.

SECURITY NOTICE:
  - This application operates strictly read-only and information-first.
  - Live order execution and broker credentials storage are intentionally omitted.
  - Management API binds strictly to loopback (127.0.0.1).

TROUBLESHOOTING & SUPPORT:
  - Diagnostics and support bundle exports are available directly inside
    the Desktop Control Center -> 'System Diagnostics & Environment' panel.
========================================================================
"@
Set-Content -Path (Join-Path $stageDir "README.txt") -Value $readmeTxt -Encoding UTF8

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

# 5. Archive to ZIP and calculate SHA-256
Write-Host "[4/5] Creating distribution archive ($zipName)..." -ForegroundColor Yellow
if (Test-Path $zipPath) {
    Remove-Item -Force $zipPath
}
Compress-Archive -Path "$stageDir\*" -DestinationPath $zipPath -CompressionLevel Optimal

Write-Host "[5/5] Calculating SHA-256 checksum..." -ForegroundColor Yellow
$hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash
Set-Content -Path $shaPath -Value "$hash  $zipName" -Encoding ASCII

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  Packaging Complete!" -ForegroundColor Green
Write-Host "  Archive:  $zipPath" -ForegroundColor White
Write-Host "  SHA-256:  $hash" -ForegroundColor White
Write-Host "  Checksum: $shaPath" -ForegroundColor White
Write-Host "==========================================================" -ForegroundColor Cyan
