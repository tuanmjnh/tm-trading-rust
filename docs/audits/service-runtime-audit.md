# TM Trading Rust — Service Runtime & Lifecycle Audit

**Audit Date**: 2026-11-24  
**Audit Scope**: Host Agent Supervisor, Worker Process Lifecycle, Environment Forwarding, Readiness Probes  
**Repository Baseline**: `main` commit `b0fec883dde3d964c7810a59bce5df8e894fb0e7`

---

## 1. Executive Summary

During startup verification of `tm-trading-rust`, only 3 of 6 registered worker services ran successfully in typical local environments. The remaining 3 services either failed during startup, exited immediately, or remained in `NOT_READY` / `DEGRADED` states.

This audit details the root causes across:
1. **Conditional Registration Flags**: Workers disabled by default unless specific environment flags are present.
2. **Environment Variable Allowlisting in Host Agent**: Host Agent clears the child environment (`env_clear()`) and selectively forwards keys. Missing keys in allowlists caused worker initialization failures.
3. **Strict Source-of-Truth Preflight Checks**: `market-worker`, `research-worker`, and `news-worker` exit with code `78` (`WORKER_CONFIGURATION_EXIT_CODE`) if `SOURCE_OF_TRUTH` is false and insecure dev mode is not set.
4. **Blocking Live Capability Probes on Startup**: `ai-worker` invoked a live LLM route during startup readiness checking (`probe_research_explanation_capability`). When API credentials are absent or routes unconfigured in local dev, the worker failed or looped indefinitely.
5. **Notification Readiness Blockers**: `notification-worker` required an active Telegram Bot token or failed readiness checks instead of operating in outbox-drain or mock mode.

---

## 2. Worker Service Inventory & Audit Matrix

| Service | Binary Name | Default Registration Flag | Source-of-Truth Flag | Critical Dependencies | Failure Mode Identified |
|---|---|---|---|---|---|
| **sample-worker** | `tm-trading-sample-worker` | Always registered (unconditional) | None | None | **Runs OK** (1/6). Heartbeat only. |
| **market-worker** | `tm-trading-market-worker` | `TM_TRADING_MARKET_WORKER_ENABLED=1` | `TM_TRADING_MARKET_SOURCE_OF_TRUTH=1` | MongoDB, Binance/Mock providers, Symbols config | **Exits (Code 78)** if `SOURCE_OF_TRUTH` is unset or 0 and insecure dev mode is false; or disabled if flag unset. |
| **research-worker** | `tm-trading-research-worker` | `TM_TRADING_RESEARCH_WORKER_ENABLED=1` | `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH=1` | MongoDB, Closed 1m/5m/1h candles | **Exits (Code 78)** or reports `NOT_READY` if candles missing; disabled if flag unset. |
| **news-worker** | `tm-trading-news-worker` | `TM_TRADING_NEWS_WORKER_ENABLED=1` | `TM_TRADING_NEWS_SOURCE_OF_TRUTH=1` | MongoDB, RSS feeds / mock items | **Exits (Code 78)** if source of truth false; disabled if flag unset. |
| **ai-worker** | `tm-trading-ai-worker` | `TM_TRADING_AI_WORKER_ENABLED=1` | None | MongoDB, AI Provider credentials (Gemini, OpenRouter, etc.) | **Startup Crash / NOT_READY Loop**: Live capability probe fails when API key is missing or invalid. |
| **notification-worker** | `tm-trading-notification-worker` | `TM_TRADING_NOTIFICATION_WORKER_ENABLED=1` | None | MongoDB, Telegram Bot Token / Chat ID | **DEGRADED / NOT_READY**: Reports `telegram-not-configured` when bot token is absent in local dev. |

---

## 3. Detailed Root Causes

### 3.1. Conditional Registration in `WorkerRegistry::local_with_version`
In `crates/supervisor-core/src/lib.rs`:
```rust
if read_flag("TM_TRADING_MARKET_WORKER_ENABLED", false) { ... }
if read_flag("TM_TRADING_RESEARCH_WORKER_ENABLED", false) { ... }
if read_flag("TM_TRADING_NEWS_WORKER_ENABLED", false) { ... }
if read_flag("TM_TRADING_AI_WORKER_ENABLED", false) { ... }
if read_flag("TM_TRADING_NOTIFICATION_WORKER_ENABLED", false) { ... }
```
When running with default `.env` or missing variables, only `sample-worker` and whatever subset happened to be enabled in `.env` were registered by the Agent.

### 3.2. Host Agent Environment Isolation & Allowlist Gaps
In `apps/agent/src/lib.rs`, `spawn_worker` isolates child processes:
```rust
let mut command = Command::new(&target_path);
command.env_clear();
```
Only variables declared in `MARKET_WORKER_ENVIRONMENT`, `RESEARCH_WORKER_ENVIRONMENT`, `NEWS_WORKER_ENVIRONMENT`, `AI_WORKER_ENVIRONMENT`, and `NOTIFICATION_WORKER_ENVIRONMENT` are passed.
If a newly introduced configuration key (e.g. `TM_AI_ALLOW_MOCK_PROBES`, `TM_STORAGE_ALLOW_INSECURE_DEV`, or specific provider keys) is not explicitly in the allowlist, the worker receives an empty value.

### 3.3. Exit Code 78 on Source-of-Truth Validation
`market-worker`, `research-worker`, and `news-worker` enforce:
```rust
if !config.source_of_truth && !config.allow_insecure_dev {
    tracing::error!("Refusing startup: source of truth is false");
    std::process::exit(78);
}
```
For local developers starting up the stack with `scripts/dev.ps1`, unless `TM_STORAGE_ALLOW_INSECURE_DEV=1` or `TM_TRADING_*_SOURCE_OF_TRUTH=1` is exported and forwarded, these processes immediately abort.

### 3.4. AI Worker Live Capability Probe Failure
`workers/ai-worker` executes `probe_research_explanation_capability` during its startup initialization to confirm that an LLM provider and route are functional.
In local development environments without a pre-configured Gemini/OpenAI API key or when network access is restricted, this probe results in an error. Without a graceful fallback or dev mode mock, the worker fails to reach `READY` state.

### 3.5. Notification Worker Telegram Dependency
`workers/notification-worker` evaluates `telegram_configured`. If `TM_TRADING_TELEGRAM_BOT_TOKEN` is unset, it reports `telegram-not-configured` as a blocking error rather than a degraded/mock status.

---

## 4. Remediation Architecture

1. **Synchronize Root Environment**:
   - Provide a complete root `.env.example` containing explicit entries for all 6 workers enabled (`=1`) and `TM_STORAGE_ALLOW_INSECURE_DEV=1` for dev environments.
2. **Update Agent Environment Allowlist**:
   - Expand `apps/agent/src/lib.rs` allowlists to guarantee all workers receive `TM_STORAGE_ALLOW_INSECURE_DEV`, source-of-truth flags, polling intervals, and provider keys.
3. **Graceful Dev Mode in AI Worker**:
   - Allow `ai-worker` in insecure dev mode (`TM_STORAGE_ALLOW_INSECURE_DEV=1` or mock probe flag) to mark the capability probe as skipped/mocked with clear status reporting, enabling it to process queued jobs without crashing.
4. **Graceful Outbox-Drain in Notification Worker**:
   - Allow `notification-worker` when unconfigured to operate in outbox-drain/mock mode with `DEGRADED` status instead of entering a crash loop.
5. **Supervisor Readiness Status Reporting**:
   - Provide accurate operational states (`READY`, `DEGRADED`, `NOT_READY`, `FAILED`) to the Management API and Desktop Dashboard.
