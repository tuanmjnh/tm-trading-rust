# TM Trading Desktop (Windows x64) Deployment & Packaging Guide

> Status: Complete as of Phase 9 Local Desktop release hardening (2026-11-23).<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

---

## 1. Architecture Overview

The TM Trading Desktop application (`tm-trading-desktop`) is an ultra-fast, native Windows operator dashboard powered by **Dioxus Desktop**, **Wry**, and the **Microsoft Edge WebView2 Evergreen Runtime**. It communicates strictly over HTTP/JSON with the local **Management API** (`tm-trading-management-api`) bound to loopback (`127.0.0.1:8787`).

```text
+-----------------------------------------------------------------------------------+
|                           TM Trading Desktop (GUI)                               |
|   (Dioxus Desktop + WebView2 Runtime + Reactive Coroutines + Bilingual EN/VI)     |
+-----------------------------------------------------------------------------------+
                                    |
                    REST / JSON (Loopback Bearer Auth)
                                    v
+-----------------------------------------------------------------------------------+
|                         Management API (127.0.0.1:8787)                          |
|         (Axum + Redb/MongoDB + Health/Diagnostics + Backup/Restore API)          |
+-----------------------------------------------------------------------------------+
          |                                               |
  Unix Domain Socket / Named Pipe                Command Polling & Heartbeats
          v                                               v
+-----------------------------+                 +-----------------------------------+
|      Supervised Agent       |                 |     Specialized Micro-Workers     |
| (Job Object / Process-Wrap) | <=============> |  (Market, News, AI, Notification) |
+-----------------------------+                 +-----------------------------------+
```

### Core Security & Safety Boundaries
1. **Information-First & Read-Only**: The desktop interface provides market overview, signal feeds, news clustering, AI analysis briefings, and notification delivery metrics. It does not place live orders and does not collect broker trading credentials.
2. **Loopback Isolation**: The Management API binds exclusively to `127.0.0.1` and authenticates every request using capability-based bearer tokens (`trading.*`).
3. **Redacted Support & Secrets**: All diagnostics, support bundles, and backup archives strictly strip and redact bot tokens, API keys, and user secrets.

---

## 2. System Prerequisites

| Component | Minimum Requirement | Recommended |
| :--- | :--- | :--- |
| **Operating System** | Windows 10 (Build 1809+, 64-bit) | Windows 11 (64-bit) |
| **WebView2 Runtime** | Microsoft Edge WebView2 Evergreen (`>= 100.0.0.0`) | Pre-installed on Windows 11 & modern Win 10 |
| **Architecture** | `x86_64` (AMD64) | `x86_64` |
| **Memory** | 512 MB RAM | 2 GB RAM |
| **Disk Space** | 100 MB free space | 500 MB free space (for historical cache) |

### Microsoft Edge WebView2 Verification
If running on Windows Server or specialized enterprise Windows editions lacking Edge components, download the **WebView2 Evergreen Bootstrapper**:
- Download Link: [https://go.microsoft.com/fwlink/p/?LinkId=2124703](https://go.microsoft.com/fwlink/p/?LinkId=2124703)
- Detection Registry Key: `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\pv`

The desktop application performs pre-flight verification at startup and displays real-time runtime health inside the **System Diagnostics** panel.

---

## 3. Automated Windows Packaging Tool

The project includes an automated PowerShell release packager: [`scripts/package-desktop-windows.ps1`](../../scripts/package-desktop-windows.ps1).

### Running the Packager

```powershell
# From repository root in PowerShell:
.\scripts\package-desktop-windows.ps1
```

To skip re-compiling if release binaries are already built:
```powershell
.\scripts\package-desktop-windows.ps1 -SkipBuild
```

### Packaging Outputs
The packaging script produces:
1. Staging directory: `dist/tm-trading-desktop-windows-x64/`
   - `bin/`: All compiled release binaries (`tm-trading-desktop.exe`, `tm-trading-management-api.exe`, `tm-trading-agent.exe`, workers).
   - `.env.example`: Default environment variable configuration template.
   - `README.txt`: Quick instructions and security notices.
   - `start-all-services.cmd`: One-click startup for Management API & Desktop Control Center.
   - `start-desktop-only.cmd`: Standalone launcher connecting to an existing server.
2. Distribution zip: `dist/tm-trading-desktop-windows-x64-v0.1.0.zip`
3. Integrity checksum: `dist/tm-trading-desktop-windows-x64-v0.1.0.zip.sha256`

---

## 4. Configuration & Environment Variables

| Variable | Default Value | Description |
| :--- | :--- | :--- |
| `TM_TRADING_API_URL` | `http://127.0.0.1:8787` | Management API server endpoint |
| `TM_TRADING_LOCAL_API_TOKEN` | `""` | User bearer token for local API access (`trading.*` capabilities) |
| `TM_TRADING_LOCALE` | `en` | Initial UI locale (`en` for English, `vi` for Vietnamese) |
| `TM_TRADING_DESKTOP_DATA_DIR`| `<exe_dir>\data\desktop` | WebView2 profile directory and cache |
| `TM_LOG_FORMAT` | `compact` | Logging format (`compact` or `json`) |

---

## 5. Operations & Health Verification

Once launched, verify system health using the built-in panels:
1. **Top Bar Connection Indicator**: Shows green dot (`Connected`) when polling the Management API successfully.
2. **Notification Center**: Review the Outbox delivery queue and send test pings.
3. **System Diagnostics & Environment**:
   - WebView2 version, distribution type, and detection source.
   - Server version, host architecture, and uptime.
   - Storage kind (Redb/Postgres/Memory) and database name.
   - Active worker counts and notification outbox status.
4. **Export Support Bundle**: Click **"Export Support Bundle"** to inspect or copy a complete diagnostic snapshot with 100% redacted secrets.

---

## 6. Troubleshooting Common Issues

### Issue 1: "WebView2 Evergreen Runtime is not installed"
- **Symptom**: Application fails to launch or exits immediately with a warning in console/logs.
- **Fix**: Run the Microsoft Evergreen Bootstrapper installer from [Microsoft Official Link](https://go.microsoft.com/fwlink/p/?LinkId=2124703).

### Issue 2: "Disconnected: connection refused"
- **Symptom**: Desktop UI opens but shows red dot with "Disconnected".
- **Fix**: Ensure the Management API is running (`start-all-services.cmd` or `cargo run -p tm-trading-management-api`). Check that port `8787` is not blocked by a firewall or occupied by another process.

### Issue 3: Backup verification failed (SHA-256 mismatch)
- **Symptom**: Attempting to restore a configuration backup returns `400 Bad Request`.
- **Fix**: The backup JSON payload was altered or truncated. Re-export the archive from the source system or verify using `POST /api/v1/system/backup/verify`.
