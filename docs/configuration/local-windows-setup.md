# TM Trading Rust — Local Windows Setup Guide

## 1. Prerequisites

To build and run TM Trading Rust on Windows 10/11 x64, ensure the following software is installed:

1. **Rust Toolchain**:
   - Install via [rustup.rs](https://rustup.rs) (`stable-x86_64-pc-windows-msvc`).
   - Rust 1.80+ or 1.82+ is recommended.
2. **Visual Studio C++ Build Tools (MSVC)**:
   - Visual Studio 2022 Community or Build Tools with "Desktop development with C++" workload installed.
   - Includes `cl.exe`, `link.exe`, and Windows SDK 10/11.
3. **MongoDB Community Server**:
   - MongoDB 7.0+ or 8.0+ running on `127.0.0.1:27017` (service or standalone).
   - Alternatively, Docker: `docker run -d -p 27017:27017 --name mongo-tm mongo:7.0`.

---

## 2. Preventing MSVC Compiler Error D8050

On some Windows systems with complex PATH or temporary directory configurations, MSVC `cl.exe` can fail while compiling C dependencies (e.g. `aws-lc-sys`) with:
```
cl : Command line error D8050 : cannot execute '...\c1.dll': failed to get command line into debug records
```

### Remediation:
1. Ensure both `TMP` and `TEMP` point to standard local short paths without trailing slashes, such as `C:\Users\<user>\AppData\Local\Temp`.
2. `scripts/dev.ps1` automatically verifies and normalizes `$env:TMP` and `$env:TEMP` before invoking `cargo`.
3. If running `cargo` manually from PowerShell, run:
   ```powershell
   $env:TMP = [System.IO.Path]::GetTempPath().TrimEnd('\')
   $env:TEMP = $env:TMP
   ```

---

## 3. Quick Start (Development Mode)

### Step 1: Copy Environment Template
```powershell
Copy-Item .env.example .env
```
Ensure `TM_STORAGE_ALLOW_INSECURE_DEV=1` is set in `.env` for local unencrypted MongoDB.

### Step 2: Start Full Stack with Auto-Build
Run the unified PowerShell development launcher:
```powershell
.\scripts\dev.ps1
```

Options:
- `.\scripts\dev.ps1 -Watch`: Watches `apps/desktop` and auto-restarts Desktop UI upon saving code.
- `.\scripts\dev.ps1 -Locale en`: Starts UI in English (default is `vi`).
- `.\scripts\dev.ps1 -Clean`: Runs `cargo clean` before building.

### Step 3: What Starts Automatically:
1. Pre-builds all workspace binaries (`api`, `agent`, `desktop`, and all 6 workers).
2. Spawns `tm-trading-management-api.exe` on `127.0.0.1:8787`.
3. Verifies port readiness via TCP probe.
4. Spawns `tm-trading-agent.exe` with `--start all` to supervise the worker fleet.
5. Launches the Dioxus Desktop UI (`tm-trading-desktop.exe`).

---

## 4. Production / Binary Distribution Launch

If running from the packaged distribution (`dist/tm-trading-desktop-windows-x64/`):
1. Double-click `start-all-services.cmd`.
2. This script:
   - Loads `.env`.
   - Starts Management API minimized in background.
   - Starts Host Agent with `--start all` in background.
   - Launches Desktop Control Center.

---

## 5. Verification & Log Inspection

Log files are placed in `target\logs\` during development:
- `target\logs\api.log`: Management API requests and endpoints.
- `target\logs\agent.log`: Host Agent lifecycle events, worker spawn messages, and heartbeat status.

To verify service status:
1. Open Desktop UI — the **Service Control Center** on the unified dashboard displays all 6 services with live PIDs, readiness indicators (`READY`), and uptime.
2. In browser or curl:
   ```bash
   curl http://127.0.0.1:8787/api/v1/services
   ```
