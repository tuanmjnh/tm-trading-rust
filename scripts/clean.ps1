<#
.SYNOPSIS
    Cleans Cargo build cache, target directory, and temporary dev files.
.DESCRIPTION
    Frees up disk space by cleaning build artifacts in target/ and orphaned temp files.
.PARAMETER Full
    Also cleans the dist/ release directory. Defaults to $false.
.EXAMPLE
    .\scripts\clean.ps1
.EXAMPLE
    .\scripts\clean.ps1 -Full
#>
[CmdletBinding()]
param(
    [switch]$Full = $false,
    [switch]$Soft = $false
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  TM TRADING - WORKSPACE CLEANUP UTILITY                 " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Clean Cargo target directory
if ($Soft) {
    Write-Host "[...] Soft-cleaning incremental caches, release artifacts, and stale deps (>3d)..." -ForegroundColor Yellow
    foreach ($sub in @("target\debug\incremental", "target\release", "target\tmp")) {
        if (Test-Path $sub) {
            Remove-Item -Path $sub -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    if (Test-Path "target\debug\deps") {
        $cutoff = (Get-Date).AddDays(-3)
        Get-ChildItem -Path "target\debug\deps" -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $cutoff } |
            Remove-Item -Force -ErrorAction SilentlyContinue
    }
    Write-Host "[OK] Soft cleanup completed (active debug deps preserved)." -ForegroundColor Green
} elseif (Test-Path "target") {
    Write-Host "[...] Cleaning Cargo build cache (cargo clean)..." -ForegroundColor Yellow
    & cargo clean
    Write-Host "[OK] Cargo target directory cleaned." -ForegroundColor Green
} else {
    Write-Host "[INFO] Cargo target directory is already clean." -ForegroundColor Gray
}

# 2. Clean temporary state directories
$tmpDirs = @("target\tmp", ".tmp-qa", ".tmp-ui-audit")
foreach ($t in $tmpDirs) {
    if (Test-Path $t) {
        Write-Host "[...] Removing temp directory: $t" -ForegroundColor Yellow
        Remove-Item -Path $t -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# Ensure TMP/TEMP do not point to deleted target\tmp (which triggers MSVC cl.exe error D8050)
$sysTemp = [System.IO.Path]::GetTempPath().TrimEnd('\')
if ([string]::IsNullOrWhiteSpace($env:TMP) -or -not (Test-Path $env:TMP)) {
    $env:TMP = $sysTemp
}
if ([string]::IsNullOrWhiteSpace($env:TEMP) -or -not (Test-Path $env:TEMP)) {
    $env:TEMP = $sysTemp
}

# 3. Clean root temporary logs
Get-ChildItem -Path $RepoRoot -Filter "*.log" -File -ErrorAction SilentlyContinue | ForEach-Object {
    Remove-Item -Path $_.FullName -Force -ErrorAction SilentlyContinue
}

# 4. Optional full clean
if ($Full -and (Test-Path "dist")) {
    Write-Host "[...] Removing dist/ release directory..." -ForegroundColor Yellow
    Remove-Item -Path "dist" -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "[OK] dist/ directory cleaned." -ForegroundColor Green
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  CLEANUP COMPLETED SUCCESSFULLY!                         " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Cyan
