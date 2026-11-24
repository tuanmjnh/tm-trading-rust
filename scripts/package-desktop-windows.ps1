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
TM_TRADING_AGENT_TOKEN=tm-local-agent-token

# Desktop locale: 'en' for English, 'vi' for Tiếng Việt:
TM_TRADING_LOCALE=vi

# Logging format ('compact' or 'json'):
TM_LOG_FORMAT=compact

# Database configuration (MongoDB local or Atlas):
TM_DATABASE_URI=mongodb://127.0.0.1:27017
TM_DATABASE_NAME=tm-trading-rust
TM_STORAGE_ALLOW_INSECURE_DEV=1
TM_TRADING_STORE=mongodb

# Enable worker fleet on host agent
TM_TRADING_MARKET_WORKER_ENABLED=1
TM_TRADING_RESEARCH_WORKER_ENABLED=1
TM_TRADING_RESEARCH_METHODS=vsa,price-action,trend,wonderwhales,sweep
TM_TRADING_NEWS_WORKER_ENABLED=1
TM_TRADING_AI_WORKER_ENABLED=1
TM_TRADING_NOTIFICATION_WORKER_ENABLED=1
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
  3. MongoDB server (local at mongodb://127.0.0.1:27017 or remote URI).

QUICK START:
  1. Copy '.env.example' to '.env' and adjust tokens/database if desired.
  2. Double click 'start-all-services.cmd' to launch Backend API, Host Agent, and Desktop UI.
  3. Double click 'stop-all-services.cmd' to stop all running services cleanly.
  4. Or double click 'start-desktop-only.cmd' to connect Desktop to an already running server.

SECURITY NOTICE:
  - This application operates strictly read-only and information-first.
  - Live order execution and broker credentials storage are intentionally omitted.
  - Management API binds strictly to loopback (127.0.0.1).

TROUBLESHOOTING & SUPPORT:
  - Logs are output to the console and diagnostics are available in:
    Desktop Control Center -> 'System Diagnostics & Environment' panel.
========================================================================
"@
Set-Content -Path (Join-Path $stageDir "README.txt") -Value $readmeTxt -Encoding UTF8

$loadEnvCmd = @"
@echo off
rem Canonical environment loader for TM Trading
cd /d "%~dp0"
if exist ".env" (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        set "%%A=%%B"
    )
)

if "%TM_TRADING_API_URL%"=="" set "TM_TRADING_API_URL=http://127.0.0.1:8787"
if "%TM_TRADING_LOCAL_API_TOKEN%"=="" set "TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token"
if "%TM_TRADING_AGENT_TOKEN%"=="" set "TM_TRADING_AGENT_TOKEN=tm-local-agent-token"
if "%TM_TRADING_LOCALE%"=="" set "TM_TRADING_LOCALE=vi"
if "%TM_TRADING_LOG_FORMAT%"=="" set "TM_TRADING_LOG_FORMAT=compact"
if "%TM_DATABASE_URI%"=="" set "TM_DATABASE_URI=mongodb://127.0.0.1:27017"
if "%TM_DATABASE_NAME%"=="" set "TM_DATABASE_NAME=tm-trading-rust"
if "%TM_STORAGE_ALLOW_INSECURE_DEV%"=="" set "TM_STORAGE_ALLOW_INSECURE_DEV=1"
if "%TM_TRADING_STORE%"=="" set "TM_TRADING_STORE=mongodb"
if "%TM_TRADING_MARKET_WORKER_ENABLED%"=="" set "TM_TRADING_MARKET_WORKER_ENABLED=1"
if "%TM_TRADING_RESEARCH_WORKER_ENABLED%"=="" set "TM_TRADING_RESEARCH_WORKER_ENABLED=1"
if "%TM_TRADING_RESEARCH_METHODS%"=="" set "TM_TRADING_RESEARCH_METHODS=vsa,price-action,trend,wonderwhales,sweep"
if "%TM_TRADING_NEWS_WORKER_ENABLED%"=="" set "TM_TRADING_NEWS_WORKER_ENABLED=1"
if "%TM_TRADING_AI_WORKER_ENABLED%"=="" set "TM_TRADING_AI_WORKER_ENABLED=1"
if "%TM_TRADING_NOTIFICATION_WORKER_ENABLED%"=="" set "TM_TRADING_NOTIFICATION_WORKER_ENABLED=1"
"@
Set-Content -Path (Join-Path $stageDir "load-env.cmd") -Value $loadEnvCmd -Encoding ASCII

$startAllCmd = @"
@echo off
title TM Trading Desktop & Services
setlocal
cd /d "%~dp0"
call "%~dp0load-env.cmd"

echo [1/3] Starting Management API Server in background...
start "TM Trading Management API" /min "%~dp0bin\tm-trading-management-api.exe"

echo [...] Waiting for Management API to become ready on 8787...
powershell -NoProfile -Command "for (`$i=0; `$i -lt 30; `$i++) { Start-Sleep -Milliseconds 500; try { `$t = New-Object System.Net.Sockets.TcpClient; `$t.Connect('127.0.0.1', 8787); `$t.Close(); exit 0 } catch {} }; exit 1"
if %ERRORLEVEL% equ 0 (
    echo [OK] Management API is live and listening!
) else (
    echo [WARN] Management API did not respond within timeout, continuing launch...
)

echo [2/3] Starting Host Agent with all workers...
start "TM Trading Host Agent" /min "%~dp0bin\tm-trading-agent.exe" --api-url "%TM_TRADING_API_URL%" --start all

timeout /t 1 /nobreak >nul

echo [3/3] Launching TM Trading Control Center...
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
"@
Set-Content -Path (Join-Path $stageDir "start-all-services.cmd") -Value $startAllCmd -Encoding ASCII

$startDesktopCmd = @"
@echo off
title TM Trading Control Center
setlocal
cd /d "%~dp0"
call "%~dp0load-env.cmd"
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
"@
Set-Content -Path (Join-Path $stageDir "start-desktop-only.cmd") -Value $startDesktopCmd -Encoding ASCII

$stopAllCmd = @"
@echo off
title Stopping TM Trading Services
setlocal
echo Stopping all TM Trading processes...
taskkill /F /IM tm-trading-desktop.exe 2>nul
taskkill /F /IM tm-trading-agent.exe 2>nul
taskkill /F /IM tm-trading-management-api.exe 2>nul
taskkill /F /IM tm-trading-market-worker.exe 2>nul
taskkill /F /IM tm-trading-research-worker.exe 2>nul
taskkill /F /IM tm-trading-news-worker.exe 2>nul
taskkill /F /IM tm-trading-ai-worker.exe 2>nul
taskkill /F /IM tm-trading-notification-worker.exe 2>nul
taskkill /F /IM tm-trading-sample-worker.exe 2>nul
echo All TM Trading services stopped.
timeout /t 2 /nobreak >nul
exit /b 0
"@
Set-Content -Path (Join-Path $stageDir "stop-all-services.cmd") -Value $stopAllCmd -Encoding ASCII

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
