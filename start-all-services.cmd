@echo off
title TM Trading Rust - All Services
setlocal

cd /d "%~dp0"
if exist ".env" (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        set "%%A=%%B"
    )
)

if "%TM_TRADING_API_URL%"=="" set "TM_TRADING_API_URL=http://127.0.0.1:8787"
if "%TM_TRADING_LOCAL_API_TOKEN%"=="" set "TM_TRADING_LOCAL_API_TOKEN=local-dev-operator-token"
if "%TM_TRADING_AGENT_TOKEN%"=="" set "TM_TRADING_AGENT_TOKEN=local-dev-agent-token"

echo Starting Management API Server in background...
start "TM Trading Management API" /min "%~dp0target\debug\tm-trading-management-api.exe"

timeout /t 2 /nobreak >nul

echo Starting Host Agent and Worker Fleet in background...
start "TM Trading Agent" /min "%~dp0target\debug\tm-trading-agent.exe" --api-url %TM_TRADING_API_URL% --start all

timeout /t 1 /nobreak >nul

echo Launching TM Trading Desktop Control Center...
start "" "%~dp0target\debug\tm-trading-desktop.exe"
exit /b 0
