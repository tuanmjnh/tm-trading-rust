# Service Catalog

> Status: Phase 2 sample-worker runtime implemented and hardened on 2026-11-18; Phase 5 market-worker runtime implemented on 2026-11-20; Phase 6 research-worker runtime implemented on 2026-11-20; Phase 7 news-worker runtime implemented on 2026-11-20; Phase 7 ai-worker runtime implemented on 2026-11-22<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## `sample-worker`

| Field | Value |
|---|---|
| Service definition ID | `sample-worker` |
| Executable | `tm-trading-sample-worker.exe` on Windows; `tm-trading-sample-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Exercises process startup, readiness, heartbeat, restart, command, log, and graceful-shutdown paths without reading or writing market data |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker reports `stopping`, drains, reports `stopped`, and exits |

### State projection

The service keeps the three lifecycle dimensions separate:

- Desired state: `RUNNING` or `STOPPED`.
- Observed state: `STARTING`, `RUNNING`, `STOPPING`, `STOPPED`, `FAILED`, or `UNKNOWN`.
- Readiness: `READY`, `DEGRADED`, `NOT_READY`, or `UNKNOWN`.

A live process is not sufficient for `READY`. The sample worker must also report verified capability and a recent heartbeat.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | The worker completed its sample capability self-check |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `recent-success` | No | A real, non-skipped cycle succeeded within 60 seconds; stale evidence degrades readiness |

Missing required evidence is `NOT_READY`, never an implicit success. A skipped cycle is not recorded as a successful cycle.

### Restart policy

An unexpected exit is `FAILED` and schedules an exponential restart with jitter. The default budget is five restart attempts, with a maximum backoff of 60 seconds. The restart count and pending schedule survive an Agent restart, so reconciliation does not reset the budget.

Protocol/schema, instance-identity, PID, executable-path, and executable-version failures are deterministic rejection causes. The Agent quarantines those workers as `FAILED`, blocks automatic retry, and persists the block in the local checkpoint. A deterministic failure must not be retried until the worker or its configuration is corrected; the local Agent API exposes an explicit retry-block clear for that operator action, but the Management API does not expose it yet.

### Ownership and local state

The Host Agent is the only process that may launch this worker. It uses a static service registry and clears the inherited child environment before adding an allowlist.

Each installation uses a dedicated Agent state directory. The Agent holds an exclusive `agent.lock` file for the lifetime of the process, so a second Agent fails closed instead of owning the same installation. The directory also contains `agent-state.json`, a minimal local checkpoint used only to reconcile desired state, process identity, restart scheduling, restart budget, and deterministic-failure quarantine after an Agent restart, plus `agent-state.json.bak` as the last known-good recovery copy. It is not an API contract, durable operation record, or service-snapshot repository.

An explicit service stop changes desired state to `STOPPED` and cancels a pending automatic restart. Agent-level shutdown changes only the observed process state: desired `RUNNING`, a pending restart backoff, and the restart budget are preserved so the next Agent launch can reconcile the service correctly.

The `worker.v2` handshake must match the spawned PID, canonical executable path, and executable version. Windows workers run inside a kill-on-close Job Object. Unix workers run inside a process group, and explicit forced termination targets that group; descendant cleanup after an abrupt Agent loss has not yet been verified on a Unix runtime.

### Current limitations

- The sample worker performs no market, research, news, AI, notification, or storage work.
- Its regular cycle is a deterministic readiness signal only.
- The Agent runtime is not yet connected to the Management API, durable operation repository, or service-snapshot persistence.
- Unix runtime shutdown and abrupt-loss descendant-containment drills require a Linux execution environment.

## `market-worker`

| Field | Value |
|---|---|
| Service definition ID | `market-worker` |
| Executable | `tm-trading-market-worker.exe` on Windows; `tm-trading-market-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Collects closed public bars from one provider definition into the `candles` collection and reports the provider health it observed |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker aborts an in-flight cycle, reports `stopped`, and exits |

The worker is opt-in. It is only registered when `TM_TRADING_MARKET_WORKER_ENABLED=1`; otherwise the Agent starts and supervises only the sample worker.

### Configuration

| Variable | Meaning |
|---|---|
| `TM_DATABASE_URI` | TLS MongoDB URI; only the market worker receives it |
| `TM_DATABASE_NAME` | Database name, default `tm-trading-rust` |
| `TM_TRADING_MARKET_SYMBOLS` | Comma-separated symbols, required |
| `TM_TRADING_MARKET_TIMEFRAME` | Candle timeframe, default `1m` |
| `TM_TRADING_MARKET_BARS` | Bars requested per cycle, default `100` |
| `TM_TRADING_MARKET_CYCLE_MS` | Delay between completed cycles |
| `TM_TRADING_MARKET_CYCLE_TIMEOUT_MS` | Hard deadline for one cycle |
| `TM_TRADING_MARKET_HEARTBEAT_MS` | Heartbeat interval |
| `TM_TRADING_MARKET_PROVIDER_ID` | Provider definition to use; required when the enabled set is ambiguous |
| `TM_TRADING_MARKET_SOURCE_OF_TRUTH` | `1`/`true` allows candle writes; defaults off |

`TM_TRADING_MARKET_SOURCE_OF_TRUTH` is deliberately off by default. With it disabled the worker exits with code `78` before probing a provider or writing a candle, so a misconfigured deployment cannot become a second writer.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | At least one selected provider answered a bounded health probe |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `recent-success` | Yes | A real, non-skipped cycle stored candles within 60 seconds |

A cycle that returns no closed candles is a skipped cycle, not a success. A provider or storage failure revokes readiness even after an earlier success. Aborting an active cycle during shutdown reports `drained: false`, because the cycle may not have finished.

### Provider selection and fallback

The worker reads enabled `provider_instances` and builds a bounded chain of at most three instances of one provider definition. Instances are ordered Healthy, then Unknown, then Unhealthy, preserving repository priority inside each health class. The chain never mixes Binance and OKX because their symbols are not interchangeable. Without `TM_TRADING_MARKET_PROVIDER_ID`, an enabled set that spans two definitions is rejected as ambiguous instead of guessed.

Each attempt records the observed health (`Healthy`/`Unhealthy`, latency, or a sanitized error code) back on the instance. A storage failure ends the cycle instead of falling back to another provider, because retrying a different source cannot fix an unwritable sink.

### Write path and atomicity

The worker writes only `state='closed'` bars keyed by `{symbol}:{timeframe}:{openTime}`. A batch with more than one candle uses one majority-write-concern MongoDB transaction, so a partial multi-symbol cycle cannot leave half its symbols recorded. A transaction commit retries a bounded number of times on `UnknownTransactionCommitResult`; the commit is idempotent, so a retry cannot duplicate bars.

### Current limitations

- REST polling only; no WebSocket streams.
- Binance and OKX are the only adapters.
- Candle rows are served by the Market Overview and Market Pulse API read models and Desktop panels.
- Unix runtime shutdown and descendant-containment drills remain unverified.

## `research-worker`

| Field | Value |
|---|---|
| Service definition ID | `research-worker` |
| Executable | `tm-trading-research-worker.exe` on Windows; `tm-trading-research-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Scans persisted closed candles with the deterministic VSA method and stores owner-scoped research events with provenance |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker aborts an in-flight scan, reports `stopped`, and exits |

The worker is opt-in. It is registered only when `TM_TRADING_RESEARCH_WORKER_ENABLED=1`; otherwise the Agent does not launch it.

### Configuration

| Variable | Meaning |
|---|---|
| `TM_DATABASE_URI` | TLS MongoDB URI; only the market and research workers receive it |
| `TM_DATABASE_NAME` | Database name, default `tm-trading-rust` |
| `TM_TRADING_RESEARCH_OWNER_SCOPE` | Required owner scope for every event |
| `TM_TRADING_RESEARCH_SYMBOLS` | Comma-separated symbols, required |
| `TM_TRADING_RESEARCH_TIMEFRAME` | Candle timeframe, default `1m` |
| `TM_TRADING_RESEARCH_BARS` | Closed candles read per symbol, default `120` |
| `TM_TRADING_RESEARCH_CYCLE_MS` | Delay between completed scans |
| `TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS` | Hard deadline for one scan |
| `TM_TRADING_RESEARCH_HEARTBEAT_MS` | Heartbeat interval |
| `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH` | `1`/`true` allows research event writes; defaults off |

`TM_TRADING_RESEARCH_SOURCE_OF_TRUTH` is deliberately off by default. With it disabled the worker exits with code `78` before opening MongoDB or writing an event, so a cutover installation cannot become a second writer.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | A bounded, non-skipped research scan completed successfully |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `dependencies-reachable` | Yes | MongoDB and the candle store were reachable |
| `sink-writable` | Yes | The research event store accepted the scan output |
| `recent-success` | Yes | A successful scan completed within 10 minutes |

The worker remains `NOT_READY` while the required candle window is missing, stale, or not closed. A skipped or failed cycle is never recorded as a success, and any non-success cycle revokes the previously verified capability.

### Scan and write path

Every symbol is read and validated before any event is written, so one invalid symbol cannot leave a partially published cycle. Each event keeps the candle source, provider method, deterministic input snapshot ID, producer/schema versions, quality state and reasons, evidence, context, observation/detection time, and ingestion time. The deterministic event ID makes replaying the same window an upsert rather than a duplicate.

### Current limitations

- Only the VSA/Wyckoff method is ported; additional deterministic research methods remain incremental work.
- The owner scope is installation configuration; multi-tenant identity binding to TM Hub is not yet available.
- Unix runtime shutdown and descendant-containment drills remain unverified.

## `news-worker`

| Field | Value |
|---|---|
| Service definition ID | `news-worker` |
| Executable | `tm-trading-news-worker.exe` on Windows; `tm-trading-news-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Polls due, license-allowed RSS/Atom sources, stores rights-safe normalized items in `news_items`, and records per-source health in `news_sources` |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker aborts an in-flight cycle, reports `stopped`, and exits |

The worker is opt-in. It is registered only when `TM_TRADING_NEWS_WORKER_ENABLED=1`; otherwise the Agent does not launch it.

### Configuration

| Variable | Meaning |
|---|---|
| `TM_DATABASE_URI` | TLS MongoDB URI; only the domain workers receive it |
| `TM_DATABASE_NAME` | Database name, default `tm-trading-rust` |
| `TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE` | Items kept per source and cycle, default `40`, maximum `200` |
| `TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE` | Sources polled per cycle, default `25`, maximum `100` |
| `TM_TRADING_NEWS_CYCLE_MS` | Delay between completed cycles, default `60000` |
| `TM_TRADING_NEWS_CYCLE_TIMEOUT_MS` | Hard deadline for one cycle, default `60000` |
| `TM_TRADING_NEWS_HEARTBEAT_MS` | Heartbeat interval, default `5000` |
| `TM_TRADING_NEWS_SOURCE_OF_TRUTH` | `1`/`true` allows news writes; defaults off |

`TM_TRADING_NEWS_SOURCE_OF_TRUTH` is deliberately off by default. With it disabled the worker exits with code `78` before opening MongoDB or polling a source, so a cutover installation cannot become a second writer.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | At least one due, allowed source was polled successfully |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `dependencies-reachable` | Yes | MongoDB, the registry, and the item store were reachable |
| `sink-writable` | Yes | The item store accepted the cycle output |
| `recent-success` | Yes | A successful collection completed within 30 minutes |

A cycle that polls nothing because no source is due is reported as skipped, not as a success. A cycle where every polled source failed is a failure, and an earlier success does not keep readiness.

### Collection and write path

Sources are considered only when their declared `pollIntervalSec` has elapsed since the last fetch, or when an operator requested a retry. A source must be enabled, have `licenseStatus = ok`, permit metadata storage, and carry an RSS feed URL; anything else is skipped without a request. Each fetch has one timeout, a 2 MB document ceiling, no redirect following, and a safe-URL check before the request, so the worker cannot become an SSRF proxy. Only title, canonical link, publication time, a plain-text 240-character excerpt, and category tags are extracted; full article text is never fetched or stored. Each fetch result updates only that source's health fields, and a failed fetch records a sanitized code instead of raw provider text.

### Current limitations

- RSS/Atom/RDF only; no source-specific API adapters and no news clustering yet.
- Items are stored with default `level = med` and `dir = flat`; deterministic scoring and event classification remain subsequent work.
- Items are not yet served by a Management API read model or the Desktop News & Events panel.
- Unix runtime shutdown and descendant-containment drills remain unverified.

## `ai-worker`

| Field | Value |
|---|---|
| Service definition ID | `ai-worker` |
| Executable | `tm-trading-ai-worker.exe` on Windows; `tm-trading-ai-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Claims one durable `ai_jobs` request at a time and publishes an evidence-bound briefing in `ai_briefings` |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker stops claiming, reports `stopped`, and exits |

The worker is opt-in. It is registered only when `TM_TRADING_AI_WORKER_ENABLED=1`; otherwise the Agent does not launch it.

### Configuration

| Variable | Meaning |
|---|---|
| `TM_DATABASE_URI` | TLS MongoDB URI; only the domain workers receive it |
| `TM_DATABASE_NAME` | Database name, default `tm-trading-rust` |
| `TM_TRADING_AI_WORKER_ENABLED` | `1`/`true` allows the worker to publish briefings; defaults off |
| `TM_TRADING_AI_CLAIM_MS` | Delay between two claim attempts, default `5000` |
| `TM_TRADING_AI_HEARTBEAT_MS` | Heartbeat interval, default `5000` |
| `TM_TRADING_AI_LEASE_MS` | Lease taken on a claimed job, default `300000` |
| `TM_TRADING_AI_*` | Provider credential namespace; the Agent forwards it to this worker only |

`TM_TRADING_AI_WORKER_ENABLED` is off by default, and the worker exits with code `78` before opening MongoDB when it is disabled, so an installation cannot publish briefings by accident. The provider credential namespace is forwarded because every AI provider profile names its credential inside `TM_TRADING_AI_`; the values stay in the process environment and are never logged or persisted.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | The `research_explanation` route, its provider, and its model are enabled and the credential resolves |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `dependencies-reachable` | Yes | MongoDB and the job, briefing, evidence, and route stores were reachable |
| `recent-success` | No | A briefing was published within the last hour; a stale one degrades the service |

The readiness probe resolves the provider and its credential but never calls the provider, so readiness never costs a billable request. The worker is idle between queued jobs, so a missing briefing is not a failure.

### Claim and publish path

The worker claims at most one job per tick, oldest first. A claim takes a bounded lease and increments the fencing token; completion and failure require the current lease holder and token. The worker assembles owner-scoped evidence, runs the configured route with the route's attempt, timeout, and output budgets, validates that the model returned only interpretations and uncertainties, then atomically publishes the briefing and completes the job in one MongoDB transaction. A request with no usable evidence fails before any provider call. A job whose lease lapses before completion stays `OUTCOME_UNKNOWN`; the worker never reports an unobserved run as successful.

### Current limitations

- Only the `research_explanation` task is executed; the other routes have no job producer.
- Unix runtime shutdown and descendant-containment drills remain unverified.
