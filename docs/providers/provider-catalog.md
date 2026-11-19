# Market Provider Catalog

> Status: Binance spot and OKX swap public REST adapters plus provider catalog/instance configuration with bounded health tests implemented on 2026-11-19. WebSocket streaming, the Market Worker, and the Control Center provider panel are not implemented yet.

## 1. Scope

Every adapter in this catalog is read-only. An adapter may:

- fetch public closed bars and normalize them into `tm_trading_domain::Candle`;
- run one small unauthenticated health request with a bounded timeout.

An adapter may never hold trading credentials, call a private or account endpoint, place or amend orders, or read balances, positions, or open orders. Attribution is always recorded: `source` is the provider id and `market` is the canonical market the adapter reports under.

## 2. Implemented adapters

| Catalog id | Crate type | Market | Base URL | Bars endpoint | Health endpoint | Bar limit |
|---|---|---|---|---|---|---|
| `binance` | `BinanceProvider` | spot | `https://api.binance.com` | `GET /api/v3/klines` | `GET /api/v3/ping` | 1000 |
| `okx` | `OkxProvider` | futures (swap) | `https://www.okx.com` | `GET /api/v5/market/candles` | `GET /api/v5/public/time` | 300 |

Both adapters are constructed with an explicit base URL and timeout, so a caller can point them at a test double or a regional endpoint without changing the crate:

```rust
let provider = BinanceProvider::new(BINANCE_SPOT_BASE_URL, Duration::from_secs(8))?;
let bars = provider.closed_candles("BTCUSDT", "1m", 100, now_ms).await?;
```

## 3. Closed-bar rules

A stored bar is a fact, so an adapter must never hand out a forming bar.

- Binance: the newest kline may still be open; a row is accepted only when `closeTime < now_ms`, where `now_ms` is the caller's observation time. The adapter requests one extra bar and truncates, so a request for `n` closed bars can still return `n`.
- OKX: rows carry `confirm`, which is `1` only for a closed bar. OKX reports the open time alone, so the close time is derived as `openTime + barLength - 1` and checked against `now_ms` as well.
- Either adapter rejects the whole payload when a row cannot be read, rather than silently dropping it; only rows that are legitimately not closed are skipped.
- Bars are returned oldest first. A bar that fails domain validation (for example `high` below the body) is an error, not a quiet skip.

## 4. Symbol and timeframe conventions

- Symbols are passed to the provider exactly as given. Binance spot uses `BTCUSDT`; OKX swap uses `BTC-USDT-SWAP`. The stored candle keeps that symbol, so rows from different providers never collide.
- Canonical timeframes are `{n}m`, `{n}h`, `{n}d`, `{n}w` and are normalized to lower case for storage and for Binance intervals.
- OKX bars use upper-case units for hours, days, and weeks (`4H`, `1D`, `1W`). Minutes stay lower case: on OKX `1M` is a monthly bar, so the adapter never emits it from a canonical minute timeframe.
- `timeframe_millis` rejects an unknown unit and a zero count.

## 5. Failure and health semantics

- `ProviderError::Status` carries the HTTP status; `Payload` names the unreadable field; `Request` carries the transport message; `Timeout` marks a request that ran out of time.
- Health is a single request per call. `ProviderHealth { provider, latency_ms }` is returned only when the request succeeded; no result is cached, and a successful health check says nothing about bar data being fresh.
- No secret is ever part of a URL, log line, or error message, because no adapter has one.

## 6. Provider definitions and instances

A provider definition is adapter code compiled into the service; a provider instance is the
operator's configuration for one definition. The catalog is static and exposed at
`GET /api/v1/providers/catalog`:

| Catalog id | Display name | Default base URL | Timeframes |
|---|---|---|---|
| `binance` | Binance Public | `https://api.binance.com` | `1m`, `5m`, `15m`, `1h`, `4h`, `1d`, `1w` |
| `okx` | OKX Public | `https://www.okx.com` | `1m`, `5m`, `15m`, `1h`, `4h`, `1d`, `1w` |

An instance stores its definition id, display name, enabled flag, base URL, bounded probe
timeout, and the result of the last health probe. Configuration and observation stay
separate: `updatedAt` changes only when the operator edits the instance, while
`lastHealthCheckAt`, `lastSuccessAt`, `lastLatencyMs`, and `lastErrorCode` describe the
newest probe. A failure clears the latency and keeps the last successful probe time.

Base URL rules, enforced by the domain model:

- the scheme must be `https`, or `http` for a loopback host only, so a local proxy or test
  server can be used while a remote provider always requires TLS;
- the URL must not carry credentials, a query, or a fragment;
- the probe timeout must be between 500 ms and 30 s.

The Management API owns these instances:

```text
GET    /api/v1/providers/catalog
GET    /api/v1/providers
POST   /api/v1/providers
GET    /api/v1/providers/{providerInstanceId}
PATCH  /api/v1/providers/{providerInstanceId}
DELETE /api/v1/providers/{providerInstanceId}
POST   /api/v1/providers/{providerInstanceId}/test
```

`POST .../test` runs exactly one bounded public request through the instance's own base
URL and timeout. A provider that fails the probe still answers `200`: the observation is
the result, and `healthState` plus `lastErrorCode` carry the verdict. Only an unknown
definition or an unusable configuration is a `422`.

Health failures are reported as sanitized classes, never as raw provider text:
`configuration`, `unavailable`, `timeout`, `http_status`, `invalid_payload`, and
`unsupported_capability`.

## 7. Not implemented yet

- Persistent provider health history and the Control Center Market Providers section.
- WebSocket trade/ticker/depth streams and the Market Worker that records bars into the `candles` collection.
- Bybit and other adapters.
- Provider fields from the roadmap that no public market adapter uses yet: priority, fallback
  group, maximum concurrency, rate-limit policy, and secret reference.

## 8. Verification

Unit tests cover payload parsing, forming-bar filtering, ordering, timeframe mapping, and domain rejection without touching the network.

Live tests reach the public endpoints and run only when `TM_TRADING_LIVE_PROVIDER_TESTS=1` is set:

```powershell
$env:TM_TRADING_LIVE_PROVIDER_TESTS = "1"
cargo test -p tm-trading-market-providers --test live_public
```

The live run fetches three closed one-minute bars per provider and asserts each bar closed before the observation time. Without the variable the test skips and prints `skipped`.
