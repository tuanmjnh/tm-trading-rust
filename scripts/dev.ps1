<#
.SYNOPSIS
    Starts the full TM Trading development stack (Backend Management API + Desktop UI).
.DESCRIPTION
    Launches the backend Management API in the background (or side-by-side terminal)
    and then launches the Desktop UI with optional live auto-reload monitoring.
.PARAMETER WithBackend
    Auto-starts tm-trading-management-api if not already running on port 8787. Defaults to $true.
.PARAMETER Watch
    Watches file changes in apps/desktop and auto-restarts the Desktop UI on edit. Defaults to $false.
.PARAMETER Locale
    UI language ('en' or 'vi'). Defaults to 'vi'.
.EXAMPLE
    .\scripts\dev.ps1
.PARAMETER Clean
    Performs a clean build by removing target/ directory before launching. Defaults to $false.
.EXAMPLE
    .\scripts\dev.ps1
.EXAMPLE
    .\scripts\dev.ps1 -Clean
.EXAMPLE
    .\scripts\dev.ps1 -Watch -Locale vi
#>
[CmdletBinding()]
param(
    [switch]$WithBackend = $true,
    [switch]$Watch = $false,
    [switch]$Clean = $false,
    [string]$Locale = "vi",
    [string]$ApiUrl = "http://127.0.0.1:8787",
    [string]$ApiToken = "local-dev-operator-token"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

if ($Clean) {
    Write-Host "[...] Clean flag detected. Cleaning Cargo build cache..." -ForegroundColor Yellow
    & cargo clean
    Write-Host "[OK] Clean completed." -ForegroundColor Green
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  TM TRADING - DESKTOP & API DEV ENVIRONMENT RUNNER      " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# Set environment variables for Desktop and Management API
$env:TM_TRADING_LOCALE = $Locale
$env:TM_TRADING_API_URL = $ApiUrl
$env:TM_TRADING_LOCAL_API_TOKEN = $ApiToken
$env:TM_TRADING_AGENT_TOKEN = "local-dev-agent-token"
$env:TM_TRADING_LOG_FORMAT = "compact"
$env:RUST_LOG = "info,tm_trading_desktop=info"

# Shared local MongoDB database environment
$env:TM_DATABASE_URI = "mongodb://127.0.0.1:27017"
$env:TM_DATABASE_NAME = "tm-trading-rust"
$env:TM_STORAGE_ALLOW_INSECURE_DEV = "1"
$env:TM_TRADING_STORE = "mongodb"

# Worker write permissions and default telemetry targets
$env:TM_TRADING_MARKET_SOURCE_OF_TRUTH = "1"
$env:TM_TRADING_MARKET_SYMBOLS = "BTCUSDT,ETHUSDT,SOLUSDT"
$env:TM_TRADING_RESEARCH_OWNER_SCOPE = "default"
$env:TM_TRADING_RESEARCH_SOURCE_OF_TRUTH = "1"
$env:TM_TRADING_RESEARCH_SYMBOLS = "BTCUSDT,ETHUSDT,SOLUSDT"
$env:TM_TRADING_RESEARCH_METHODS = "vsa,price-action,trend,wonderwhales,sweep"
$env:TM_TRADING_NEWS_SOURCE_OF_TRUTH = "1"
$env:TM_TRADING_MARKET_WORKER_ENABLED = "1"
$env:TM_TRADING_RESEARCH_WORKER_ENABLED = "1"
$env:TM_TRADING_NEWS_WORKER_ENABLED = "1"
$env:TM_TRADING_AI_WORKER_ENABLED = "1"
$env:TM_TRADING_NOTIFICATION_WORKER_ENABLED = "1"

# 1. Check & Start Management API if requested
$apiProcess = $null
$agentProcess = $null
if ($WithBackend) {
    $portOpen = $false
    try {
        $tcp = New-Object System.Net.Sockets.TcpClient
        $tcp.Connect("127.0.0.1", 8787)
        $tcp.Close()
        $portOpen = $true
    } catch {
        $portOpen = $false
    }

    if ($portOpen) {
        Write-Host "[OK] Management API is already running on 127.0.0.1:8787" -ForegroundColor Green
    } else {
        Write-Host "[...] Starting Management API on 127.0.0.1:8787 in background..." -ForegroundColor Yellow
        $apiPsi = New-Object System.Diagnostics.ProcessStartInfo
        $apiPsi.FileName = "cargo"
        $apiPsi.Arguments = "run -p tm-trading-management-api"
        $apiPsi.WorkingDirectory = $RepoRoot
        $apiPsi.UseShellExecute = $true
        $apiProcess = [System.Diagnostics.Process]::Start($apiPsi)

        Write-Host "[...] Waiting for Management API to become ready..." -ForegroundColor Yellow
        $ready = $false
        for ($i = 0; $i -lt 30; $i++) {
            Start-Sleep -Milliseconds 800
            try {
                $tcp = New-Object System.Net.Sockets.TcpClient
                $tcp.Connect("127.0.0.1", 8787)
                $tcp.Close()
                $ready = $true
                break
            } catch { }
        }
        if ($ready) {
            Write-Host "[OK] Management API is live and accepting connections!" -ForegroundColor Green
        } else {
            Write-Host "[WARN] API did not answer on 8787 yet. Desktop UI will retry automatically." -ForegroundColor DarkYellow
        }
    }

    # Start Host Agent with workers enabled so Services table shows live worker instances
    Write-Host "[...] Starting Host Agent with live worker fleet..." -ForegroundColor Yellow
    $env:TM_TRADING_MARKET_WORKER_ENABLED = "1"
    $env:TM_TRADING_RESEARCH_WORKER_ENABLED = "1"
    $env:TM_TRADING_RESEARCH_METHODS = "vsa,price-action,trend,wonderwhales,sweep"
    $env:TM_TRADING_NEWS_WORKER_ENABLED = "1"
    $env:TM_TRADING_AI_WORKER_ENABLED = "1"
    $env:TM_TRADING_NOTIFICATION_WORKER_ENABLED = "1"

    $agentPsi = New-Object System.Diagnostics.ProcessStartInfo
    $agentPsi.FileName = "cargo"
    $agentPsi.Arguments = "run -p tm-trading-agent -- --api-url http://127.0.0.1:8787 --start sample-worker"
    $agentPsi.WorkingDirectory = $RepoRoot
    $agentPsi.UseShellExecute = $true
    $agentProcess = [System.Diagnostics.Process]::Start($agentPsi)
    Write-Host "[OK] Host Agent started!" -ForegroundColor Green
}

# 2. Check if dx (Dioxus CLI) or cargo-watch is available
$hasDx = (Get-Command dx -ErrorAction SilentlyContinue) -ne $null
$hasCargoWatch = (Get-Command cargo-watch -ErrorAction SilentlyContinue) -ne $null

Write-Host "----------------------------------------------------------" -ForegroundColor DarkGray
Write-Host "  Desktop UI Mode: Locale=$Locale | Watch=$Watch" -ForegroundColor White
Write-Host "----------------------------------------------------------" -ForegroundColor DarkGray

try {
    if ($hasDx) {
        Write-Host "[DX] Using Dioxus CLI with native Hot-Reload!" -ForegroundColor Green
        Set-Location "$RepoRoot\apps\desktop"
        dx serve --platform desktop
    } elseif ($hasCargoWatch -and $Watch) {
        Write-Host "[WATCH] Using cargo-watch for auto-recompilation on save..." -ForegroundColor Green
        cargo watch -x "run -p tm-trading-desktop" -w "apps/desktop"
    } elseif ($Watch) {
        Write-Host "[WATCH-LOOP] File-watcher active. UI will restart upon saving files..." -ForegroundColor Green
        $desktopDir = Join-Path $RepoRoot "apps/desktop"
        $watcher = New-Object System.IO.FileSystemWatcher
        $watcher.Path = $desktopDir
        $watcher.IncludeSubdirectories = $true
        $watcher.EnableRaisingEvents = $true
        $watcher.Filter = "*.*"

        while ($true) {
            Write-Host "[RUN] Starting Desktop UI..." -ForegroundColor Cyan
            $desktopProcess = Start-Process -FilePath "cargo" -ArgumentList "run -p tm-trading-desktop" -PassThru -NoNewWindow
            
            # Wait for file change event or process exit
            $changed = $false
            while (-not $desktopProcess.HasExited) {
                $changeEvent = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::Changed -bor [System.IO.WatcherChangeTypes]::Created, 1000)
                if ($changeEvent.TimedOut -eq $false) {
                    $ext = [System.IO.Path]::GetExtension($changeEvent.Name)
                    if ($ext -in @(".rs", ".css", ".toml")) {
                        Write-Host "[CHANGE DETECTED] $($changeEvent.Name) modified! Reloading Desktop UI..." -ForegroundColor Magenta
                        $changed = $true
                        if (-not $desktopProcess.HasExited) {
                            Stop-Process -Id $desktopProcess.Id -Force -ErrorAction SilentlyContinue
                        }
                        Start-Sleep -Milliseconds 500
                        break
                    }
                }
            }

            if (-not $changed) {
                # User closed the UI manually
                Write-Host "[STOP] Desktop UI was closed by user." -ForegroundColor Yellow
                break
            }
        }
    } else {
        Write-Host "[RUN] Launching Desktop UI directly (cargo run -p tm-trading-desktop)..." -ForegroundColor Cyan
        cargo run -p tm-trading-desktop
    }
} finally {
    if ($agentProcess -and -not $agentProcess.HasExited) {
        Write-Host "[CLEANUP] Stopping background Host Agent process..." -ForegroundColor DarkGray
        Stop-Process -Id $agentProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if ($apiProcess -and -not $apiProcess.HasExited) {
        Write-Host "[CLEANUP] Stopping background Management API process..." -ForegroundColor DarkGray
        Stop-Process -Id $apiProcess.Id -Force -ErrorAction SilentlyContinue
    }
    Set-Location $RepoRoot
}
