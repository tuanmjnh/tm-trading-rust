# TM Trading Rust — Configuration Overview

## 1. Architectural Philosophy

TM Trading Rust is an information-first, read-only trading intelligence and operational platform built in Rust. It does not execute live orders, hold broker trading credentials, or alter user accounts. Central identity, RBAC, and credentials reside in TM Hub.

The application runtime is composed of three tiers:
1. **Desktop Client (`apps/desktop`)**: High-density Dioxus Desktop UI interacting with the loopback Management API.
2. **Management API (`apps/management-api`)**: Axum-based HTTP server managing service states, settings, telemetry, market snapshots, AI jobs, and notification outbox queues.
3. **Host Agent (`apps/agent`) & Supervised Workers (`workers/*`)**:
   - The single Host Agent oversees 6 dedicated worker processes.
   - Child processes are isolated (`env_clear()`) and selectively receive allowlisted variables based on their service definition.
   - Inter-process communication occurs over standard stdin/stdout using line-delimited JSON (`worker.v2` schema).

---

## 2. Configuration Sources & Precedence

Configuration is resolved in the following priority order:
1. **Process Environment Variables**: Exported in the running shell or injected by Windows launcher scripts.
2. **Local `.env` File**: Placed at the repository root or application binary folder. Loaded automatically by launcher scripts (`scripts/dev.ps1`, `start-all-services.cmd`).
3. **Built-in Safe Defaults**: Documented default values compiled into the application binaries.

---

## 3. Configuration Segments

| Configuration Area | Key Purpose | Primary Environment Variables |
|---|---|---|
| **API & Security** | Loopback binding and bearer tokens | `TM_TRADING_API_URL`, `TM_TRADING_LOCAL_API_TOKEN`, `TM_TRADING_AGENT_TOKEN` |
| **Persistence** | MongoDB connection & dev mode | `TM_DATABASE_URI`, `TM_DATABASE_NAME`, `TM_STORAGE_ALLOW_INSECURE_DEV` |
| **Worker Enablement** | Opt-in worker service registration | `TM_TRADING_MARKET_WORKER_ENABLED`, `TM_TRADING_RESEARCH_WORKER_ENABLED`, etc. |
| **Market Data** | Binance/Mock candle ingestion | `TM_TRADING_MARKET_SOURCE_OF_TRUTH`, `TM_TRADING_MARKET_SYMBOLS`, `TM_TRADING_MARKET_TIMEFRAME` |
| **Research Analysis** | Multi-method signal scans (VSA, etc.) | `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH`, `TM_TRADING_RESEARCH_METHODS`, `TM_TRADING_RESEARCH_SYMBOLS` |
| **News Ingestion** | RSS and news stream ingestion | `TM_TRADING_NEWS_SOURCE_OF_TRUTH`, `TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE` |
| **AI Briefings** | LLM provider integration & job execution | `TM_TRADING_AI_CLAIM_MS`, `GEMINI_API_KEY`, `OPENROUTER_API_KEY`, `OPENAI_API_KEY` |
| **Notifications** | Outbox draining & Telegram delivery | `TM_TRADING_TELEGRAM_BOT_TOKEN`, `TM_TRADING_TELEGRAM_CHAT_ID` |

---

## 4. Environment Forwarding Discipline

To prevent credential leakage and accidental privilege escalation:
- The Host Agent **clears the child process environment** before spawning any worker.
- Variables are forwarded according to dedicated allowlists:
  - `MARKET_WORKER_ENVIRONMENT`: Database URI, allow-insecure-dev, market symbols, intervals, source-of-truth.
  - `RESEARCH_WORKER_ENVIRONMENT`: Database URI, allow-insecure-dev, research symbols, methods, intervals, source-of-truth.
  - `NEWS_WORKER_ENVIRONMENT`: Database URI, allow-insecure-dev, limits, source-of-truth.
  - `AI_WORKER_ENVIRONMENT`: Database URI, allow-insecure-dev, lease intervals, plus credentials matching `TM_TRADING_AI_*`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`.
  - `NOTIFICATION_WORKER_ENVIRONMENT`: Database URI, allow-insecure-dev, claim intervals, `TM_TRADING_TELEGRAM_*`, `TM_TELEGRAM_*`.
