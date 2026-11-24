# TM Trading Rust — Complete Environment Variables Reference

This document provides an exhaustive inventory of all environment variables supported by the TM Trading Rust system across the Management API, Host Agent, Desktop Client, and all 6 Worker services.

---

## 1. Global & Management API Settings

| Variable Name | Type | Default Value | Required | Purpose & Description |
|---|---|---|---|---|
| `TM_TRADING_API_URL` | URL | `http://127.0.0.1:8787` | No | Base URL for Desktop and Agent to communicate with Management API. |
| `TM_MANAGEMENT_API_BIND` | Host:Port | `127.0.0.1:8787` | No | Network interface and port bound by the Axum Management API server. |
| `TM_TRADING_LOCAL_API_TOKEN` | String (Secret) | `local-dev-operator-token` | No (in dev) | Operator bearer token for authorized Desktop client requests. |
| `TM_TRADING_AGENT_TOKEN` | String (Secret) | `local-dev-agent-token` | No (in dev) | Shared agent secret for Host Agent heartbeat & status synchronization. |
| `TM_TRADING_AGENT_API_TOKEN` | String (Secret) | `local-dev-agent-token` | No (in dev) | Alias for `TM_TRADING_AGENT_TOKEN`. |
| `TM_TRADING_LOCALE` | String (`en` \| `vi`) | `vi` | No | User interface localization language for Desktop application. |
| `TM_LOG_FORMAT` / `TM_TRADING_LOG_FORMAT` | String (`compact` \| `json`) | `compact` | No | Output format for structured logs. |
| `RUST_LOG` | String | `info` | No | Filtering directives for `tracing` and `env_logger`. |

---

## 2. Storage & Database Configuration

| Variable Name | Type | Default Value | Required | Purpose & Description |
|---|---|---|---|---|
| `TM_DATABASE_URI` | MongoDB URI | `mongodb://127.0.0.1:27017` | No | MongoDB connection string. Must use TLS (`mongodb+srv://` or `ssl=true`) in production unless insecure dev is enabled. |
| `TM_DATABASE_NAME` | String | `tm-trading-rust` | No | Target MongoDB database name. |
| `TM_STORAGE_ALLOW_INSECURE_DEV` | Flag (`1` \| `0`) | `0` | No | When `1`, permits non-TLS connections, standalone MongoDB servers without transactions, and auto-seeding mock providers. |
| `TM_TRADING_STORE` | String (`mongodb`) | `mongodb` | No | Primary persistence engine identifier. |
| `TM_TRADING_DATA_DIR` | Path | `data` | No | Local embedded storage directory (used by redb checkpoint caches). |

---

## 3. Worker Fleet Registration Flags

All worker services (except `sample-worker` which is always enabled) are registered dynamically based on these boolean flags:

| Variable Name | Type | Default Value | Forwarded To | Purpose & Description |
|---|---|---|---|---|
| `TM_TRADING_MARKET_WORKER_ENABLED` | Flag (`1` \| `0`) | `0` | Host Agent | Enables supervision and lifecycle management of `tm-trading-market-worker`. |
| `TM_TRADING_RESEARCH_WORKER_ENABLED` | Flag (`1` \| `0`) | `0` | Host Agent | Enables supervision and lifecycle management of `tm-trading-research-worker`. |
| `TM_TRADING_NEWS_WORKER_ENABLED` | Flag (`1` \| `0`) | `0` | Host Agent | Enables supervision and lifecycle management of `tm-trading-news-worker`. |
| `TM_TRADING_AI_WORKER_ENABLED` | Flag (`1` \| `0`) | `0` | Host Agent | Enables supervision and lifecycle management of `tm-trading-ai-worker`. |
| `TM_TRADING_NOTIFICATION_WORKER_ENABLED` | Flag (`1` \| `0`) | `0` | Host Agent | Enables supervision and lifecycle management of `tm-trading-notification-worker`. |

---

## 4. Market Worker Environment

| Variable Name | Type | Default Value | Description |
|---|---|---|---|
| `TM_TRADING_MARKET_SOURCE_OF_TRUTH` | Flag (`1` \| `0`) | `0` | **Mandatory** authority flag. If `0` and not in dev mode, worker exits with code `78`. |
| `TM_TRADING_MARKET_SYMBOLS` | Comma-separated | `BTCUSDT,ETHUSDT` (in dev) | Symbols monitored for market candle fetching. |
| `TM_TRADING_MARKET_TIMEFRAME` | String | `1m` | Candle timeframe (e.g. `1m`, `5m`, `15m`, `1h`, `4h`, `1d`). |
| `TM_TRADING_MARKET_BARS` | Integer | `100` | Historical candle depth retrieved per cycle. |
| `TM_TRADING_MARKET_CYCLE_MS` | Milliseconds | `5000` | Polling interval between candle ingestion passes. |
| `TM_TRADING_MARKET_CYCLE_TIMEOUT_MS`| Milliseconds | `15000`| Maximum allowed duration for one market collection cycle. |
| `TM_TRADING_MARKET_HEARTBEAT_MS` | Milliseconds | `5000` | Heartbeat frequency sent to Host Agent. |
| `TM_TRADING_MARKET_PROVIDER_ID` | String | `binance` | Preferred market data provider ID. |

---

## 5. Research Worker Environment

| Variable Name | Type | Default Value | Description |
|---|---|---|---|
| `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH`| Flag (`1` \| `0`) | `0` | **Mandatory** authority flag to write research event documents. |
| `TM_TRADING_RESEARCH_SYMBOLS` | Comma-separated | `BTCUSDT,ETHUSDT` | Target symbols analyzed during technical scanning cycles. |
| `TM_TRADING_RESEARCH_METHODS` | Comma-separated | `vsa,price-action,trend,wonderwhales,sweep` | Analytical strategy engines activated for detection. |
| `TM_TRADING_RESEARCH_OWNER_SCOPE` | String | `default` | Multi-tenant tenant/owner boundary for research events. |
| `TM_TRADING_RESEARCH_TIMEFRAME` | String | `1m` | Timeframe of input candles loaded for pattern recognition. |
| `TM_TRADING_RESEARCH_BARS` | Integer | `120` | Depth of closed candles inspected for pattern completion. |
| `TM_TRADING_RESEARCH_CYCLE_MS` | Milliseconds | `15000` | Frequency of scanning sweeps. |
| `TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS`| Milliseconds | `20000` | Hard deadline for research event scan cycle. |
| `TM_TRADING_RESEARCH_HEARTBEAT_MS` | Milliseconds | `5000` | Heartbeat frequency sent to Host Agent. |

---

## 6. News Worker Environment

| Variable Name | Type | Default Value | Description |
|---|---|---|---|
| `TM_TRADING_NEWS_SOURCE_OF_TRUTH` | Flag (`1` \| `0`) | `0` | **Mandatory** authority flag to ingest and upsert news items. |
| `TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE`| Integer | `10` | Maximum RSS/news feed endpoints polled in one cycle. |
| `TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE` | Integer | `20` | Maximum fresh headlines parsed and recorded per source. |
| `TM_TRADING_NEWS_CYCLE_MS` | Milliseconds | `60000` | News feed polling cycle interval. |
| `TM_TRADING_NEWS_CYCLE_TIMEOUT_MS` | Milliseconds | `30000` | Maximum timeout before an HTTP feed fetch is cancelled. |
| `TM_TRADING_NEWS_HEARTBEAT_MS` | Milliseconds | `5000` | Heartbeat frequency sent to Host Agent. |

---

## 7. AI Worker & Provider Keys

| Variable Name | Type | Default Value | Description |
|---|---|---|---|
| `TM_TRADING_AI_CLAIM_MS` | Milliseconds | `5000` | Polling interval for claiming queued AI briefing jobs. |
| `TM_TRADING_AI_HEARTBEAT_MS` | Milliseconds | `5000` | Heartbeat frequency sent to Host Agent. |
| `TM_TRADING_AI_LEASE_MS` | Milliseconds | `300000` (5m) | Distributed fencing lease taken on claimed jobs. |
| `GEMINI_API_KEY` | String (Secret) | None | Google Gemini API key used by Gemini provider models. |
| `OPENAI_API_KEY` | String (Secret) | None | OpenAI API key used by OpenAI/ChatGPT models. |
| `OPENROUTER_API_KEY` | String (Secret) | None | OpenRouter API key used for unified multi-provider routing. |
| `ANTHROPIC_API_KEY` | String (Secret) | None | Anthropic API key used by Claude models. |
| `TM_TRADING_AI_*` | String (Secret) | None | Any credential prefixed with `TM_TRADING_AI_` is forwarded exclusively to `ai-worker`. |

---

## 8. Notification Worker & Telegram Channel

| Variable Name | Type | Default Value | Description |
|---|---|---|---|
| `TM_TRADING_NOTIFICATION_CLAIM_MS` | Milliseconds | `2000` | Outbox polling delay between queue drain sweeps. |
| `TM_TRADING_NOTIFICATION_HEARTBEAT_MS`| Milliseconds | `5000` | Heartbeat frequency sent to Host Agent. |
| `TM_TRADING_NOTIFICATION_LEASE_MS` | Milliseconds | `30000` | Distributed fencing lease taken on outbox items. |
| `TM_TRADING_TELEGRAM_BOT_TOKEN` | String (Secret) | None | Telegram Bot token for sending messages. |
| `TM_TELEGRAM_BOT_TOKEN` | String (Secret) | None | Alias for `TM_TRADING_TELEGRAM_BOT_TOKEN`. |
| `TM_TRADING_TELEGRAM_CHAT_ID` | String | None | Target chat / channel ID for notification dispatches. |
| `TM_TELEGRAM_CHAT_ID` | String | None | Alias for `TM_TRADING_TELEGRAM_CHAT_ID`. |
| `TM_TRADING_TELEGRAM_BASE_URL` | URL | `https://api.telegram.org` | Telegram API gateway base URL. |
