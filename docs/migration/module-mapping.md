# Module Mapping

> Audit date: 2026-11-17`<br>
> Source: `D:\Applications\tm-trading`<br>
> Target: `D:\Applications\tm-trading-rust`
> Authority: `docs/TM-TRADING-RUST-MASTER-ROADMAP.md`

## 1. Mapping policy

The Rust project is a controlled port, not a line-by-line translation.

- Preserve domain meaning, deterministic outputs, idempotency, provenance, and operational honesty.
- Recreate runtime boundaries where the target architecture differs.
- Keep existing MongoDB collection/schema compatibility where practical.
- Do not recreate Nuxt, Nitro, Vue, the old in-process service runner, or a second supervisor.
- Do not port order/execution capability.
- Introduce only the packages needed by the current vertical slice.

Package names describe ownership boundaries. Executable names describe process boundaries. A package does not imply a separate process.

## 2. Workspace mapping

### 2.1 Initial packages

| Target package | Responsibility | Legacy source |
|---|---|---|
| `crates/domain` | Domain enums, value objects, lifecycle invariants, provenance primitives | `engine/`, `shared/types/`, selected `services/readiness.mjs` semantics |
| `crates/contracts` | Versioned API DTOs, events, errors, OpenAPI schemas | `shared/types/`, `shared/utils/errors.ts`, `server/api/v1/` response shapes |
| `crates/api-client` | Typed client used by Desktop and Web | `app/composables/useHub.ts` client usage, legacy `$fetch` call sites |
| `crates/auth` | Hub token verification, capability checks, ownership helpers | `server/middleware/auth.ts`, `server/utils/authz.ts`, `server/utils/owner.ts` |
| `crates/storage` | Repository traits, MongoDB implementation, indexes, test repositories | `engine/db.mjs`, `engine/models/`, `server/utils/mongo.ts` |
| `crates/config` | Typed configuration and validation | `.env.example`, `nuxt.config.ts`, `server/modules/config/` |
| `crates/observability` | Tracing setup, structured fields, correlation IDs | Legacy console/JSON logs and Hub audit logging |
| `crates/agent-protocol` | Registration, heartbeat, command, progress, result contracts | `services/workerHost.mjs` IPC messages and roadmap Agent protocol |
| `crates/supervisor-core` | Registry, desired/observed/readiness state machine, restart policy | `services/registry.mjs`, `services/readiness.mjs`, `services/supervisor.mjs` |
| `apps/management-api` | Axum HTTP process and API composition | `server/api/v1/`, `server/middleware/`, `server/plugins/` |
| `apps/agent` | Host Agent executable and process owner | `services/supervisor.mjs` runtime CLI |
| `workers/sample-worker` | First end-to-end worker used to prove lifecycle behavior | Legacy fixture worker used by `tools/drills/` |
| `crates/research-engine` | Pure deterministic calculations: technical-analysis primitives, then indicators, methods, and backtest logic | `engine/ta.mjs` |
| `apps/desktop` | Dioxus Desktop client | `app/` |

### 2.2 Deferred packages

Create these only when their owning slice begins:

| Target package | Deferred source |
|---|---|
| `crates/provider-core` | Generic provider registry/capability/health contracts |
| `crates/market-providers` | `market/providers/` and market REST/WS adapters |
| `crates/news-providers` | RSS/Atom and news source adapters |
| `crates/ai-providers` | `ai/gateway.mjs` and provider protocol adapters |
| `crates/telegram-client` | `notify/send.mjs`, Telegram transport |
| `crates/notification-core` | `notify/outbox.mjs`, `notify/router.mjs`, `notify/policy.mjs` |
| `crates/ui-components` | Dioxus components shared by Desktop and Web |
| `apps/web` | Future Dioxus Web client |

## 3. Source-to-target mapping

### 3.1 Market plane

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `market/events.mjs` | `crates/domain` + `crates/contracts` | Port/normalize | Preserve canonical event identity, source, timestamps, and payload compatibility |
| `market/clock.mjs` | `crates/domain` | Port | Trading time and wall time remain distinct |
| `market/candleBuilder.mjs`, `market/aggregate.mjs` | `crates/research-engine` or market domain | Port later | Closed candle output and aggregation boundaries must match fixtures |
| `market/recorder.mjs` | `crates/storage` + `workers/market-worker` | Ported: closed-bar repository and bounded collection worker | Keep `candles` natural ID and schema version |
| `market/quoteStore.mjs`, `market/orderbookStore.mjs` | Market worker state | Defer | In-memory state is worker-owned, not API-owned |
| `market/eventBus.mjs` | Internal worker messaging | Reimplement | Keep high-frequency fan-out internal and bounded |
| `market/providers/binance.mjs` | `crates/market-providers` | Port REST candles done; WebSocket streams later | Public data only; no account/order endpoints |
| `market/providers/bybit.mjs`, `market/providers/okx.mjs` | `crates/market-providers` | OKX REST candles done; other adapters later | Public data only; verify public endpoint terms before activation |
| `market/quality.mjs` | `crates/domain` | Port | Quality states must be explicit, not inferred by UI |
| `market/monitor.mjs`, `market/healthSnapshot.mjs` | Market worker readiness/health | Ported basics: per-instance health and worker readiness | Freshness and source evidence feed readiness |
| `realtime/` | SSE/event layer or worker transport | Defer | Do not preserve raw WebSocket API merely for compatibility |

### 3.2 Research engine

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `engine/ta.mjs` | `crates/research-engine` | Port, primitives done | Deterministic outputs and numeric edge cases; goldens are generated from the legacy module by `crates/research-engine/tests/fixtures/generate_ta_vectors.mjs` |
| `engine/indicators.mjs` | `crates/research-engine` | Port | Preserve parameter names and warmup behavior |
| `engine/methods/vsa.mjs` | `crates/research-engine` | Port with golden fixtures | Pine parity remains the acceptance source |
| `engine/methods/priceAction.mjs`, `trend.mjs`, `orderflow.mjs` | `crates/research-engine` | Port one method at a time | Each method needs approved fixtures and version stamp |
| `engine/version.mjs` | `crates/domain` + research engine | Port | Preserve engine version, params hash, data hash, universe snapshot, Git revision |
| `engine/keys.mjs` | `crates/domain` | Port retained keys | Deterministic key derivation must remain stable for existing records |
| `engine/backtest.mjs`, `engine/league.mjs` | `crates/research-engine` | Later | No execution capability; results are research artifacts |
| `engine/evidence.mjs`, `engine/evidenceSet.mjs` | `crates/domain`, `crates/storage` | Port | Deterministic set ID, content hash, frozen facts, append-only usage references |
| `engine/pointInTime.mjs` | `crates/domain` | Port | No future data leakage |
| `engine/manifest.mjs` | `crates/contracts` | Port | Dataset/source manifests remain versioned |

### 3.3 Services and supervisor

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `services/registry.mjs` | `crates/supervisor-core` | Reimplement | Typed service ID, dependencies, intervals, readiness rules, supported actions |
| `services/readiness.mjs` | `crates/supervisor-core` | Reimplement | Desired/observed/readiness separate; tri-state evidence |
| `services/supervisor.mjs` | `crates/supervisor-core` + `apps/agent` | Reimplement | One owner, idempotent operations, leases, reconciliation, bounded restart |
| `services/workerHost.mjs` | Worker runtime in `crates/supervisor-core` or shared runtime | Reimplement | Explicit shutdown channel, heartbeat, cycle/result protocol, graceful drain |
| `services/workerProbes.mjs` | `crates/supervisor-core` + worker evidence module | Reimplement | Evidence must distinguish unknown from false |
| `services/supervisorStore.mjs` | MongoDB operations/services plus local checkpoint | Reimplement | Durable operations survive process restart; minimal local state is recoverable |
| `services/commandInbox.mjs` | `crates/storage` operation/outbox repository | Replace | Atomic claim, lease expiry, fencing, dead-letter visibility |
| `services/telemetry.mjs` | `crates/observability` | Reimplement | Resource saturation may degrade health but is not process state |
| `services/slo.mjs` | `crates/observability` | Defer | Add only with measured operational need |
| `services/run.mjs` | None as a second runner | Remove from target design | Only the Agent owns worker lifecycle |
| `services/funding.mjs`, `scanner.mjs`, `news.mjs`, `regime.mjs`, `liquidation.mjs`, `confluence.mjs`, `crossScan.mjs`, `macro.mjs` | Domain services + `workers/market-worker`, `workers/research-worker`, `workers/news-worker` | Port incrementally | Keep output contracts, source provenance, deterministic parts |
| `services/signalProducer.mjs` | `workers/research-worker` | Port later | Research event only; no order path |
| `services/alertRules.mjs` | `workers/research-worker` + notification core | Port later | Rule identity and owner scope remain durable |
| `services/outboxDrain.mjs` | `workers/notification-worker` | Port later | Dedicated outbox consumer remains independently observable |
| `services/aiAgentWorker.mjs`, `aiReviewWorker.mjs` | `workers/ai-worker` | Port later as read-only | No autonomous execution |
| `services/botWorker.mjs`, bot-related services | None | Exclude | Execution capability is outside product scope |

### 3.4 Notifications

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `notify/outbox.mjs` | `crates/notification-core` | Port | One mutable queue row per event/channel, bounded retries, dead state |
| `notify/outboxDriver.mjs` | `crates/notification-core` + notification worker | Port | Claim/lease, attempt count, next-attempt time, terminal state |
| `notify/router.mjs` | `crates/notification-core` | Port | Immutable delivery ledger separate from mutable queue |
| `notify/policy.mjs` | `crates/notification-core` | Port | Category, priority, quiet hours, and dedupe behavior |
| `notify/send.mjs`, `services/telegram.mjs` | `crates/telegram-client` + notification worker | Port later | Normalize 429/retry-after; redact token; never call Telegram from UI |

### 3.5 AI and providers

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `ai/gateway.mjs` | `crates/ai-providers` | Reimplement behind trait | Protocol translation only; provider metadata and health are explicit |
| `ai/providerStore.mjs` | `crates/storage` + provider core | Port later | Keep provider connection/model metadata; move secret to secret store |
| `ai/daily-brief.mjs` | `crates/ai-providers` + AI worker | Port later | Persist model/prompt/input versions and source references |
| `ai/agent.mjs` | AI worker/application service | Port later, read-only | Facts vs interpretation; no mutation of deterministic research records |
| `ai/review.mjs` | AI worker | Port later | Human-review requirements remain visible |
| `engine/models/aiProviderConnection.mjs` | `crates/storage` | Compatible read/write | `secretEnc` compatibility is read-only until secure store cutover is proven |

### 3.6 API and UI

| Legacy path | Target | Action | Compatibility requirement |
|---|---|---|---|
| `server/api/v1/health.get.ts` | `apps/management-api` | Recreate | Health reflects API, DB, Agent, and actual service states |
| `server/api/v1/operations/**` | `apps/management-api` + `crates/storage` | Recreate | `202 Accepted`, operation ID, correlation ID, idempotency |
| `server/api/v1/watchlists/**` | `apps/management-api` + `crates/storage` | Recreate done | Owner scope and per-owner unique names |
| `server/api/v1/market-sources/health.get.ts` | `apps/management-api` + `crates/market-providers` | Recreate done for public market providers | Bounded public probe; liveness and freshness remain separate; no credentials |
| `server/api/v1/news/**` | API + news worker | Recreate later | Publication/ingestion times and canonical/origin URLs |
| `server/api/v1/ai/providers/**` | API + provider core | Recreate later | Secrets masked and never returned |
| `app/composables/useHub.ts` | `crates/api-client` + `crates/auth` | Reimplement | Same Hub identity source; no browser-only assumptions |
| `app/components/dashboard/**` | `crates/ui-components` + `apps/desktop` | Rebuild | Single Dashboard with independent panel states |
| `app/components/**` overlays | `crates/ui-components` | Rebuild later | Modal/Slideover behavior and accessibility |
| `app/pages/**` | `apps/desktop` | Rebuild selectively | One Dashboard plus one Control Center, not the old route tree |

### 3.7 Excluded API and UI

No target route is created for:

```text
/api/v1/orders/**
/api/v1/paper/**
/api/v1/positions/**
/api/v1/exchange-demo/**
/api/v1/bots/**
/api/v1/research/proposals/** (until reclassified as read-only research)
/api/v1/admin/services/run-once (replaced by typed service operations)
```

## 4. First vertical-slice mapping

The first slice proves process ownership and API truth before domain data is ported.

| Target component | Legacy behavior reused | New behavior |
|---|---|---|
| `crates/domain` | Desired/observed/readiness distinctions and operation states | Rust enums and invariants with unit tests |
| `crates/contracts` | Legacy success/error envelopes and operation identifiers | One versioned `/api/v1` error model and OpenAPI schemas |
| `crates/storage` | Durable operation and service records | Trait-based repository plus in-memory test implementation; MongoDB implementation later |
| `crates/supervisor-core` | `services/supervisor.mjs` and `workerHost.mjs` invariants | Pure state machine and typed worker protocol |
| `apps/agent` | Supervisor process owner | Independent executable that starts/stops the allowlisted sample worker |
| `workers/sample-worker` | Drill fixture worker | Long-running worker with heartbeat, readiness, logs, and graceful drain |
| `apps/management-api` | `server/api/v1/operations/**` semantics | Axum endpoints that accept commands and expose actual state/operations |
| `apps/desktop` | V5 single Dashboard concept | Minimal Dioxus window that calls the API; no direct MongoDB access |

### 4.1 Required first-slice flow

1. Desktop submits a typed start operation to the API.
2. API validates the request and creates an operation with an ID and idempotency key.
3. API returns `202 Accepted`; it does not claim the worker started.
4. Agent claims the command and starts only the registered sample-worker executable.
5. Worker reports heartbeat and readiness evidence.
6. Agent records observed state and readiness separately.
7. API exposes the operation and service state from persisted records.
8. Stop uses the explicit worker shutdown channel, drains, and records escalation only if required.
9. Closing Desktop does not affect the Agent or worker.

## 5. State and operation mapping

### 5.1 Service state

| Legacy concept | Rust target | Notes |
|---|---|---|
| `desired: RUNNING | STOPPED` | `DesiredState` enum |
| `observed: STARTING | RUNNING | STOPPING | STOPPED | FAILED | UNKNOWN` | `ObservedState` enum |
| readiness checks/projections | `Readiness: READY | DEGRADED | NOT_READY | UNKNOWN` | Required and optional rules remain distinguishable |
| `health` | Derived read model | Do not store a fourth independent lifecycle truth |
| `serviceInstanceId`, `generation`, `pid` | `ServiceInstance` snapshot | PID is observational only |
| heartbeat/evidence | Typed worker report | Missing evidence is `UNKNOWN`, never false |

### 5.2 Operation contract

Legacy operation behavior maps to the roadmap contract as follows:

| Legacy field/behavior | Rust target |
|---|---|
| `operationId` | `operationId` |
| `idempotencyKey` | `idempotencyKey` |
| `requestedBy` / `actor` | `requestedBy` |
| `requestedAt` | `requestedAt` |
| `serviceId` + scope | `serviceInstanceId` and `agentId` |
| `action` | Allowlisted typed action |
| `state` | `ACCEPTED | QUEUED | CLAIMED | RUNNING | SUCCEEDED | FAILED | CANCELLED | EXPIRED` |
| `phase` | Progress detail, not a second status enum |
| lease data | `leaseOwner`, `leaseExpiresAt`, fencing token |
| `startedAt`, `finishedAt` | Same |
| `result` | `resultSummary` |
| `error` | `redactedError` plus typed error code |
| missing correlation ID | Add `correlationId` |
| file inbox claim | Atomic MongoDB find-and-update claim |

## 6. API mapping

The target uses one versioned namespace. Existing legacy paths are not aliased indefinitely; clients migrate to the consistent groups below.

| Target endpoint | Legacy/read-model source |
|---|---|
| `GET /api/v1/health` | `server/api/v1/health.get.ts`, enriched with actual Agent state |
| `GET /api/v1/version` | Build/version metadata; no legacy equivalent |
| `GET /api/v1/services` | `server/api/v1/operations/services/index.get.ts` and supervisor snapshot |
| `GET /api/v1/services/{serviceInstanceId}` | Service snapshot/readiness evidence |
| `POST /api/v1/services/{serviceInstanceId}/operations` | `server/api/v1/operations/services/{serviceId}/{action}.post.ts` |
| `GET /api/v1/operations/{operationId}` | `server/api/v1/operations/operations/{operationId}.get.ts` |
| `GET /api/v1/operations` | Operation history/read model |
| `GET /api/v1/agents` | New Agent read model, derived from registered heartbeats |
| `GET /api/v1/events/stream` | Legacy operation stream; SSE-first |
| `GET /api/v1/watchlists` | `server/api/v1/watchlists/index.get.ts` |
| `GET /api/v1/news` | `server/api/v1/news/index.get.ts` |
| `GET /api/v1/research/events` | Replaces signal/research read models with explicit research-event semantics |
| `GET /api/v1/dashboard/summary` | New aggregate read model over existing domain collections |

## 7. Cutover order

1. Implement shared contracts and a test repository.
2. Implement Agent lifecycle and sample worker; prove crash/restart/drain drills.
3. Implement Axum operation and service endpoints.
4. Connect a minimal Dioxus Desktop client through `api-client`.
5. Port market contracts and Market Worker; compare captured inputs.
6. Port Research Worker and prove deterministic fixture parity.
7. Port News Worker.
8. Port AI provider/model management and AI Worker.
9. Port notification outbox and Telegram Worker.
10. Add cloud Agent transport and remote API after local parity.

For each worker cutover:

- use a per-worker source-of-truth flag;
- prevent old and Rust writers from targeting the same production records;
- compare counts, schema, timestamps, quality, and deterministic values;
- retain the legacy rollback path until soak evidence exists;
- never delete legacy runtime in the same change that enables Rust writes.

## 8. Current mapping decisions

- The API is introduced with the Agent slice, but remains a separate executable.
- The sample worker is intentionally not a market worker; its purpose is to prove lifecycle truth.
- The in-memory repository is the first test implementation, not a production cache.
- MongoDB operations/outbox schema is introduced before remote Agents, so local and remote modes use the same contract.
- Domain/research/data ports wait until process/API architecture is verified.
- Execution, paper trading, bot launch, exchange-demo, and autonomous-agent behavior are excluded.
