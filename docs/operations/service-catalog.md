# Service Catalog

> Status: Phase 2 sample-worker runtime implemented and hardened on 2026-11-18; Phase 5 market-worker runtime implemented on 2026-11-20<br>
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
- Candle rows are written but not yet served by a Management API read model or a Desktop panel.
- Unix runtime shutdown and descendant-containment drills remain unverified.
