# TM Trading Rust — Service Startup Troubleshooting Guide

## 1. Diagnostic Overview

When one or more services fail to enter `READY` state or exit unexpectedly, use this guide to identify the root cause and apply the documented fix.

---

## 2. Common Error Codes & Failure Modes

### Exit Code 78 (`WORKER_CONFIGURATION_EXIT_CODE`)
**Symptom**: Worker process terminates immediately upon spawn; Host Agent logs:
```
worker exited before becoming ready with code Some(78)
```
**Cause**: The worker failed its preflight configuration check. Common reasons:
1. `TM_TRADING_<SERVICE>_SOURCE_OF_TRUTH` is unset or `0` and `TM_STORAGE_ALLOW_INSECURE_DEV` is not `1`.
2. Missing symbol list: `TM_TRADING_MARKET_SYMBOLS` or `TM_TRADING_RESEARCH_SYMBOLS` is empty.
3. Disabled worker flag: Worker was executed directly while `TM_TRADING_<SERVICE>_WORKER_ENABLED=0`.

**Remediation**:
- In `.env`, ensure `TM_STORAGE_ALLOW_INSECURE_DEV=1` is set for local dev.
- Ensure all source-of-truth flags are `1`:
  ```ini
  TM_TRADING_MARKET_SOURCE_OF_TRUTH=1
  TM_TRADING_RESEARCH_SOURCE_OF_TRUTH=1
  TM_TRADING_NEWS_SOURCE_OF_TRUTH=1
  ```

---

### Startup Timeout (120 Seconds)
**Symptom**: Host Agent logs:
```
service failed to start at launch; continuing service_id="<worker>" error=worker did not become ready before the startup timeout
```
**Cause**: The worker started, passed initial handshake (`Hello`), but failed to emit `WorkerMessage::Ready` within its `startup_timeout` (typically 120s). The Agent then terminated the child process.

**Per-Service Breakdown**:
- **`news-worker`**:
  - *Cause*: RSS feed endpoint returned a redirect (e.g. 308) or was unreachable.
  - *Fix*: Use `https://www.coindesk.com/arc/outboundfeeds/rss` (no trailing slash). The updated `HttpFeedFetcher` now safely follows up to 5 safe redirects.
- **`ai-worker`**:
  - *Cause*: Live capability probe (`probe_research_explanation_capability`) failed because no route or API key was configured.
  - *Fix*: In local dev mode (`TM_STORAGE_ALLOW_INSECURE_DEV=1`), the AI worker logs a warning and marks capability verified, allowing it to start and await configuration from Desktop UI.
- **`research-worker`**:
  - *Cause*: Cold start where MongoDB contains zero closed candles.
  - *Fix*: Start `market-worker` first or ensure `market-worker` has completed at least one cycle to seed candle data.

---

## 3. Step-by-Step Diagnostic Runbook

### Step 1: Check Management API Health
```powershell
curl.exe http://127.0.0.1:8787/health
```
Expected: `200 OK` with JSON `{ "status": "ok" }`.

### Step 2: Query Live Services Snapshot
```powershell
curl.exe http://127.0.0.1:8787/api/v1/services
```
Inspect the returned array:
- `desired_state`: Should be `RUNNING`.
- `observed_state`: Should be `RUNNING`. If `STOPPED` or `FAILED`, inspect `last_exit_code`.
- `readiness`: Should be `READY` or `DEGRADED`.
- `readiness_checks`: Shows individual rule states (`passed`, `failed`, `unknown`).

### Step 3: Inspect Process Logs
Check `target\logs\agent.log` for worker supervisor events:
```powershell
Get-Content -Tail 50 target\logs\agent.log
```
Look for:
- `worker became ready`
- `protocol error`
- `worker exited before becoming ready`

---

## 4. Recovering a Quarantined or Blocked Service

If a service repeatedly crashes (e.g. 5 restart attempts exceeded), the Agent places it in quarantine with `retry_blocked = true`.

To recover:
1. Fix the configuration issue in `.env`.
2. From Desktop UI: Open the **Service Control Center** on the unified dashboard, click the `...` actions button for that service, and select **Restart** or **Start**.
3. Or restart the Agent via:
   ```powershell
   target\debug\tm-trading-agent.exe --api-url http://127.0.0.1:8787 --start all
   ```
