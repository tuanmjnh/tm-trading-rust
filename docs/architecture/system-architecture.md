# Current System Architecture

> Status: Phase 1 foundation, Phase 2 local Agent/sample-worker runtime, Phase 3 local Management API, Phase 4 Dioxus Desktop shell, Phase 5 market data, watchlists, provider configuration, the opt-in Market Worker, the closed-candle API read model, and the Market Overview/Market Pulse panels are implemented. Phase 6 has the deterministic TA/indicator/VSA engine, the research event contract and store, the owner-scoped research event API, the opt-in Research Worker runtime, and the Desktop Signal Center with its research-event detail Slideover. Phase 7's core scope has the legacy-compatible News source registry, the legacy-compatible News item contract with memory/MongoDB repositories, the RSS/Atom/RDF provider crate, the opt-in News Worker, the owner-scoped news read API, the Desktop News & Events panel with its story detail Slideover, the AI provider profile registry with its bounded OpenAI-compatible, Anthropic Messages, and Gemini `generateContent` health/generation adapters, the AI model registry, the AI task-routing registry, the Desktop AI Provider Center, the evidence-bound AI briefing contract/store/API/UI, the durable owner-scoped AI job request source, the opt-in supervised AI Worker that publishes briefings, and the Desktop research-event briefing request action. Dedicated Anthropic and Gemini adapters are implemented; OpenAI and OpenRouter currently use the OpenAI-compatible adapter.<br>
> Last updated: 2026-11-22<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## 1. Current scope

The repository currently contains the shared Rust foundation, an independent local Host Agent with one sample worker, a loopback Management API, and a Dioxus Desktop Control Center that drives that API.

Implemented:

- a pinned Rust workspace and locked dependency graph;
- domain lifecycle and durable-operation invariants;
- versioned API DTOs, a stable error model, and a generated OpenAPI snapshot;
- typed environment configuration with production fail-closed checks;
- tracing initialization and correlation spans;
- a MongoDB connection boundary and an in-memory operation repository for tests;
- a versioned JSON-line worker IPC protocol with explicit shutdown and drained status;
- a static worker registry plus deterministic desired/observed/readiness state transitions, bounded logs, heartbeat freshness, and bounded restart policy;
- an independent Tokio Host Agent that owns allowlisted worker processes, separates liveness from readiness, captures bounded logs, restarts crashed workers, and drains workers explicitly;
- an exclusive per-installation Agent lock, a recoverable local process checkpoint, worker PID/path/version handshake validation, Windows Job Object containment, and Unix process-group termination;
- bounded worker-event batches, receipt-time heartbeat evaluation, persisted restart budgets and deterministic-failure quarantine, monotonic Agent shutdown, and bounded forced-process reaping;
- a JSON-line sample worker with handshake, readiness, heartbeat, cycle, operation, and graceful-shutdown messages;
- an Axum Management API that authenticates bearer credentials, enforces exact capabilities, and serves the service read model plus durable operations;
- a durable MongoDB operation store with atomic idempotency, fenced compare-and-swap transitions, expiry, and startup index creation;
- a durable `service_instances` read model replaced per Agent heartbeat, so the Desktop keeps showing the last reported state across an API restart;
- an Agent command channel that heartbeats per-Agent service snapshots and executes claimed operations;
- a typed `api-client` crate that depends only on the versioned public contracts;
- a Dioxus Desktop Control Center with live service state, Start/Stop/Restart operations with in-flight status, connection state, an owner-scoped watchlist editor, Market Overview, Market Pulse, Signal Center, News & Events, AI Briefing, and AI Provider Center panels, research-event, news-story, and AI-briefing detail Slideovers with their provenance, an owner-scoped research-event AI briefing request action with queued and terminal job feedback, and EN/VI localization;
- a provider catalog and provider instance configuration with bounded, read-only health probes;
- an opt-in Market Worker that collects closed public bars from one provider definition, records the provider health it observed, and writes each multi-candle batch in one majority-write-concern transaction;
- a dual-writer guard: the Market Worker refuses to start unless `TM_TRADING_MARKET_SOURCE_OF_TRUTH` is explicitly enabled, exiting with code `78` before any provider or candle access;
- bounded provider fallback of at most three instances of one definition, ordered by health, with each attempt's observed health recorded;
- a closed-candle Management API read model at `GET /api/v1/market/overview` and `GET /api/v1/market/pulse`, with explicit `OK`, `PARTIAL`, `STALE`, and `EMPTY` states, bounded symbol and bar counts, and the owner's default watchlist as the implicit symbol set;
- Desktop Market Overview, Market Pulse, and Signal Center panels reading that API through `api-client`, with EN/VI localization;
- a pure `research-engine` crate with legacy-matched technical-analysis primitives, the canonical indicator catalog, and the VSA/Wyckoff method (method 0), compared against committed golden fixtures;
- a `ResearchEvent` domain contract and memory/MongoDB repositories that preserve owner scope, method and producer versions, input snapshot identity, observation/detection/ingestion times, quality state and reasons, evidence, and context;
- owner-scoped research event reads at `GET /api/v1/research/events` and `GET /api/v1/research/events/{event_id}`, with bounded filters, computed freshness, and the exact `trading.research.read` capability;
- an opt-in Research Worker that scans only persisted closed candles, runs the deterministic VSA method, upserts owner-scoped research events with full provenance, enforces its own source-of-truth flag, and becomes ready only after a successful non-stale scan;
- a legacy-compatible News source registry with a fail-closed license/rights gate and memory/MongoDB repositories that preserve `news_sources` field names, string `_id = sourceId`, insert-only seeded policy, independent health updates, and secret-free public feed URLs;
- a legacy-compatible News item contract with memory/MongoDB repositories that preserve `news_items` field names, the deterministic `news_<sha1(canonicalUrl)>` identifier, canonical/origin URL duality, distinct publication/first-seen/ingestion times, a bounded rights-safe excerpt, material-change fingerprinting, and frozen observation times on re-ingest;
- a dependency-free RSS 2.0/Atom/RDF adapter that extracts only title, canonical link, publication time, a plain-text bounded excerpt, and category tags, behind a fetch path with one timeout, a 2 MB ceiling, no redirect following, and safe-URL validation;
- an opt-in News Worker that polls only due, license-allowed sources, upserts normalized items, records each source's observed health, reports a cycle without a successful poll as skipped or failed, and becomes ready only after a real collection;
- owner-scoped news reads at `GET /api/v1/news`, `GET /api/v1/news/{news_id}`, and `GET /api/v1/news/sources`, with bounded filters, read-time freshness, display-rights gating, and the exact `trading.news.read` capability;
- an AI provider profile registry with memory/MongoDB repositories, exact `trading.ai.read`/`trading.ai.write` capabilities, secret-free profile reads that expose only an API-key environment-variable name, `POST /api/v1/ai-providers/{id}/test` recording bounded OpenAI-compatible health observations, a model registry that stores provider-owned models, declares their context/output limits and capabilities, records per-model health through `POST /api/v1/ai-models/{id}/test`, and deletes them with their provider;
- an AI task-routing registry with memory/MongoDB repositories, one route per task, a primary model and an ordered fallback chain that must resolve to registered models, a bounded attempt/concurrency/output budget, and `GET`/`POST`/`PATCH`/`DELETE` endpoints under `/api/v1/ai-task-routes`;
- an evidence-bound AI briefing contract with memory/MongoDB repositories, append-once publication, exact typed evidence references, provider/model/prompt/input/producer/schema provenance, separated facts/interpretations/uncertainties, explicit missing/stale disclosures, owner-scoped list/detail API reads, and an independent Desktop briefing panel and detail Slideover;
- a durable owner-scoped AI job contract and memory/MongoDB repositories with atomic idempotency, lease-protected claims, fencing tokens, bounded expiry, and distinct failed versus unknown outcomes;
- owner-scoped AI job list/create/detail endpoints, and an opt-in supervised AI Worker that claims one job at a time, assembles bounded evidence, validates model output, and atomically publishes the briefing while completing the job.

Not implemented:

- WebSocket trade/ticker/depth streams;
- Bybit and other provider adapters;
- TM Hub token verification and permission mapping; local bearer credentials use an explicit local capability set;
- durable Agent credential rotation;
- TM Hub login, TOTP, and refresh flow in the Desktop client;
- reusable data-backed panels beyond Market Overview, Market Pulse, Signal Center, and News & Events;
- deterministic news scoring and clustering beyond same-cycle headline grouping;

## 2. Target runtime and current implementation

The target runtime remains:

```text
Desktop -> Management API -> durable operations -> Host Agent -> registered workers
```

The current workspace maps that design as follows:

```text
tm-trading-domain
  ^                 ^
  |                 |
tm-trading-contracts  tm-trading-storage
  ^        ^     ^        ^
  |        |     |        |
  |        |     +-> tm-trading-api-client -> tm-trading-desktop
  |        +-> tm-trading-management-api -> loopback HTTP
  |                     ^
  |                     |
  +-> tm-trading-agent  (outbound command channel)

tm-trading-config -> tm-trading-observability

tm-trading-market-providers -> tm-trading-domain  (read-only public data)
        |
        +-> tm-trading-management-api  (provider catalog and bounded health probe)
        +-> tm-trading-market-worker   (bounded collection cycles, agent-owned)

tm-trading-research-engine  (pure deterministic calculations; no I/O or clock)
        |
        +-> tm-trading-research-worker -> tm-trading-storage
                (bounded closed-candle scans, agent-owned)

tm-trading-news-providers -> tm-trading-domain  (read-only public feed parsing)
        |
        +-> tm-trading-news-worker -> tm-trading-storage
                (bounded feed collection, agent-owned)

tm-trading-ai-providers -> tm-trading-domain  (OpenAI-compatible protocol translation)
        |
        +-> tm-trading-management-api  (bounded provider/model health probes)
        +-> tm-trading-ai-worker -> tm-trading-storage
                (durable job claims and evidence-bound briefing publication, agent-owned)

tm-trading-agent-protocol -> tm-trading-supervisor-core -> tm-trading-agent
        |                                      |
        |                                      +-> local locked checkpoint
        +-> tm-trading-sample-worker -> worker.v2 JSON lines
```

Dependency rules:

- `domain` has no UI, HTTP, MongoDB, or provider dependencies.
- `contracts` contains public DTOs only and has no BSON models.
- `storage` implements repository behavior behind traits and translates errors at its boundary.
- `market-providers` depends only on `domain` and HTTP; it never holds credentials and never reaches a private or order endpoint.
- `ai-providers` depends only on `domain` and HTTP; it translates the OpenAI-compatible protocol, reads a secret for one bounded request, and never exposes credentials or reaches an execution endpoint.
- `research-engine` is a pure calculation crate: no I/O, no clock, no provider or storage dependency, so any result can be replayed from stored bars. Its primitives keep the legacy `engine/ta.mjs` warmup and `na` rules, its indicator registry keeps the legacy ids, versions, parameter bounds, and output keys, and its VSA method keeps the legacy per-bar execution order and event semantics. The parity suites compare every one of them against goldens generated from those modules.
- `config` owns typed environment parsing and validation.
- `observability` owns tracing setup and correlation spans.
- `agent-protocol` owns the versioned worker IPC messages and explicit control commands.
- `supervisor-core` owns static worker definitions and deterministic lifecycle/readiness transitions; it has no OS process or UI dependency.
- `apps/agent` is the sole OS process owner for workers, uses a static executable registry, clears inherited child environment before adding an allowlist, and never accepts arbitrary executable paths or arguments. It holds an exclusive state-directory lock, validates the worker's PID, executable path, and executable version during the `worker.v2` handshake, uses a Windows Job Object or Unix process group, and isolates a rejected worker without stopping other workers. On Windows the worker is spawned with `DETACHED_PROCESS` so console control events reach only the Agent, which drains the worker over the explicit control channel. Windows Job Object descendant containment is implemented; Unix forced termination targets the process group, while descendant cleanup after an abrupt Agent loss remains unverified on a Unix runtime.
- `workers/sample-worker` implements the versioned control protocol, performs its capability self-check before readiness, and reports explicit drain status.
- `workers/market-worker` implements the same control protocol and runs one bounded collection cycle at a time in a spawned task, so heartbeats and control commands stay responsive. It requires the write-authority flag, selects a bounded set of provider instances from MongoDB, persists only closed bars, records observed provider health, and reports a skipped cycle as skipped rather than successful. On shutdown it aborts an in-flight cycle and reports `drained: false`.
- `workers/news-worker` implements the same control protocol and runs one bounded collection cycle at a time. It requires its own write-authority flag, polls only sources whose declared interval elapsed and whose license gate passes, fetches each feed with a timeout and a 2 MB ceiling without following redirects, stores only rights-safe normalized fields, records each source's sanitized health, and withholds readiness until a real poll succeeds. A cycle with no due source is skipped, and a cycle where every polled source failed is a failure. Shutdown aborts an active cycle and reports `drained: false`.
- `workers/research-worker` implements the same control protocol and runs one bounded VSA scan at a time over persisted closed candles. It requires its own write-authority flag, rejects missing or stale required input without publishing partial output, upserts deterministic owner-scoped events with source/input/producer/schema/quality provenance, and withholds readiness until a real scan succeeds. A cycle timeout or input failure revokes readiness; shutdown aborts an active scan and reports `drained: false`.
- `workers/ai-worker` implements the same control protocol and claims at most one durable AI job per tick in a cancellable task, so heartbeat and shutdown commands remain responsive. Before reporting readiness it verifies transaction-capable MongoDB and performs a publication-shaped briefing-sink upsert inside a transaction that is deliberately aborted, then runs one bounded route execution within the configured attempt, timeout, and output budgets and validates the response against the briefing schema, including the aggregate model-output content limit. MongoDB connection and repository/index initialization, the 15-second transaction/sink stage, and the route chain of up to 75 seconds share one 110-second worker startup deadline below the Agent's 120-second deadline; the route stage receives only the time remaining in that deadline. Later revalidations have a 90-second budget. The probe runs in a cancellable task and failed probes are retried at most once per minute after the failed attempt completes, so provider latency cannot block heartbeats or shutdown and billable checks cannot become an uncontrolled loop. It assembles owner-scoped evidence, rejects a request with no usable evidence before calling the provider, validates that the model returned only interpretations and uncertainties, and atomically publishes the briefing while completing the job; it never reports a lapsed lease as success. Provider, route, model-output, or final-briefing-schema failures revoke verified capability until a successful probe restores it, even when a lease expiry prevents the observed failure from being persisted; evidence-only failures and a lease lapse by itself do not. Dependency or storage failures, including publication failures, likewise require successful revalidation before readiness returns, and an aggregate route-probe timeout is treated conservatively as a dependency failure. Shutdown aborting an active job or probe reports `drained: false`.
- `apps/management-api` is the local Axum Management API. It binds to loopback only, is a command producer and read-model provider, never spawns a process, and never runs a continuous worker loop. User endpoints require a bearer credential that carries exact `trading.*` capabilities; Agent endpoints require the separate per-installation Agent credential and may only act as the Agent named in the request. Operations are durable and idempotent, and are claimed with an expiring lease and fencing token.
- `apps/agent` additionally opens an outbound command channel to the Management API: it heartbeats its service read model, claims commands addressed to its own Agent identity, executes them against its own worker registry, and reports the real outcome. The Agent never accepts an executable path or command line from the API.
- `crates/api-client` is the only client-side HTTP boundary. It depends on `contracts` alone and exposes health, service listing, operation creation/polling, watchlists, provider instances, market read models, research events, news items with their source registry, AI provider profiles/models/routes, owner-scoped AI briefings, and AI job requests.
- `apps/desktop` is a thin Dioxus view over `api-client`. It connects only to the Management API, never to MongoDB or a worker, and holds no credentials beyond the user bearer token. Every user-facing string comes from `apps/desktop/src/i18n.rs`.
- Executable processes remain thin composition roots around these crates.

### Desktop WebView2 data directory

The Desktop app stores its WebView2 profile under `<exe_dir>/data/desktop`, overridable with `TM_TRADING_DESKTOP_DATA_DIR`. This mirrors the Agent's `<worker_dir>/data/agent` convention and keeps the profile beside the executable rather than in a per-user temporary directory. On the development host a profile outside the workspace caused `msedgewebview2.exe` to abort with `0x80000003` before the window could paint; the pinned in-workspace path is the verified configuration. Desktop exit never touches Agent or worker processes.

## 3. Contracts and versioning

- Public endpoints use the `/api/v1` prefix.
- The API version is `v1`; the current contract version is `1.0.0`.
- Timestamps use UTC `chrono::DateTime<Utc>` values and serialize as RFC 3339 JSON timestamps.
- API errors use stable typed codes and include a correlation ID.
- The generated contract snapshot is [`docs/api/openapi.yaml`](../api/openapi.yaml).

The current OpenAPI document defines the health/version paths, the service read model, durable operation endpoints, the watchlist endpoints, the provider catalog and instance endpoints, the market overview and pulse read models, the owner-scoped research event reads, the news item and news source reads, the AI provider profile and health-test endpoints, the AI model registry endpoints, the AI task-route endpoints, the owner-scoped AI briefing and AI job endpoints, and the Agent heartbeat/command endpoints. It is a contract snapshot; the running API serves the same document at `/api/v1/openapi.json`.

## 4. Service and operation model

Desired state, observed state, and readiness are separate domain values:

- `DesiredState`: `RUNNING` or `STOPPED`.
- `ObservedState`: `STARTING`, `RUNNING`, `STOPPING`, `STOPPED`, `FAILED`, or `UNKNOWN`.
- `Readiness`: `READY`, `DEGRADED`, `NOT_READY`, or `UNKNOWN`.

A durable operation carries an operation ID, schema version, idempotency key and request hash, requester, target Agent, service instance and service definition, typed action, bounded expiry, correlation ID, attempt count, lease, fencing token, lifecycle timestamps, result summary, redacted error, and typed error code. Lease-sensitive transitions require the current Agent identity and fencing token; a stale worker result is rejected.

The operation state sequence is:

```text
ACCEPTED -> CLAIMED -> RUNNING -> SUCCEEDED | FAILED
```

Cancellation and expiry are explicit terminal transitions. A claim increments both the attempt counter and fencing token. An active lease prevents a second claim; an expired lease may be reclaimed with a new fencing token.

The in-memory repository provides deterministic create, idempotency-conflict, lease/fencing, outcome, and expiry behavior for tests. Expired unclaimed work is transitioned to the terminal `EXPIRED` state. It is not a production cache.

## 5. Storage boundary

`MongoConnection` owns MongoDB client creation, database-name validation, and health checks. Connection strings must enable TLS, and configurations that allow invalid certificates are rejected. Certificate validation remains enabled.

`MongoOperationRepository` implements `OperationRepository` against the `operations` collection defined in [`docs/migration/data-contract-map.md`](../migration/data-contract-map.md) section 8.1. One document is both the durable operation and the command the Agent claims. The four required non-`_id` indexes are created at startup; the unique `{ requestedBy, idempotencyKey }` index makes idempotency atomic, and every state transition is a compare-and-swap on the fencing token so a stale writer cannot overwrite a newer state. A document that does not satisfy the domain invariants is rejected as corrupt instead of being loaded.

The store is selected by `TM_TRADING_STORE`: `memory` (local default) or `mongodb`. Production fails closed rather than losing durable operations on restart.

Verified against a TLS-enabled MongoDB 8.2 instance with certificate validation against a local CA: create, restart persistence, idempotent replay, claim/lease/fencing, stale-result rejection, and expiry all pass, and all five expected indexes exist.

`MongoServiceSnapshotRepository` implements `ServiceSnapshotRepository` against the `service_instances` collection from section 8.2. One document per service instance, keyed by the service instance identifier; the Agent remains authoritative, so a heartbeat upserts that Agent's instances and then deletes rows it no longer reports. A snapshot naming a different Agent is rejected instead of writing across owners. Upserts run before the stale-row delete so an interrupted heartbeat leaves an extra row until the next beat rather than losing state.

Verified live: after the Agent process is killed, and again after the Management API restarts, `GET /api/v1/services` still returns the last reported instance with its original `updated_at`; a fresh Agent heartbeat replaces that row instead of duplicating it. The `service_instances` collection carries only the subset of section 8.2 fields the current read model exposes (`generation`, `pid`, `host`, and the readiness-check list are not collected yet), plus `schemaVersion = 1`; a document with an unknown schema version or an unparseable enum is rejected as corrupt.

`MongoCandleRepository` implements `CandleRepository` against the `candles` collection from section 6.1, behind `InMemoryCandleRepository` for the local default. The natural key `{symbol}:{timeframe}:{openTime}` makes re-recording a closed bar an upsert instead of a duplicate, and the three declared indexes are created at startup. Reads accept the legacy document shape: an absent `volume` becomes `0`, an absent schema stamp is treated as `candle.v1`, and whole numbers may arrive as any BSON numeric type. A row whose `state` is not `closed`, whose `_id` disagrees with its fields, or whose schema stamp is unknown is rejected as corrupt. A batch with more than one candle is written in a single majority-write-concern transaction with a bounded retry on `UnknownTransactionCommitResult`, so a multi-symbol cycle is all-or-nothing; the commit is idempotent, so the retry cannot duplicate bars.

`GET /api/v1/market/overview` and `GET /api/v1/market/pulse` read closed candles through `CandleRepository` under `trading.market.read`. Both accept `symbols`, `timeframe`, and `bars`, default to `1m` and 60 bars, cap the request at 20 symbols and 500 bars, and fall back to the caller's default watchlist when `symbols` is absent. Every symbol carries an explicit state: `EMPTY` with no rows, `PARTIAL` when fewer than the requested bars exist, `STALE` when the newest close is older than half a timeframe (at least 60 seconds), and `OK` otherwise. The panel state is the worst state in the set, so an absent symbol is never presented as current data. Movers rank by absolute percentage change and include a volume ratio against the prior bars and a high-low volatility percentage.

`MongoWatchlistRepository` implements `WatchlistRepository` against the `watchlists` collection from section 6.5, behind `InMemoryWatchlistRepository` for the local default. `_id` remains an `ObjectId` and timestamps remain BSON datetimes so legacy rows stay compatible. The unique `{ownerId, name}` index uses primary-strength collation, and every query includes the owner, so one subject cannot read, replace, or delete another subject's list. The first list an owner creates becomes their default. The Management API serves these rows only through the authenticated subject; the Agent credential is rejected with `403`.

`MongoProviderInstanceRepository` implements `ProviderInstanceRepository` against the `provider_instances` collection from section 6.6, behind `InMemoryProviderInstanceRepository` for the local default. This collection is new to the Rust service, so it carries no legacy shape. Configuration fields and observed health fields are separate: a configuration edit rewrites the whole document, while a health probe updates only `healthState`, `lastHealthCheckAt`, `lastSuccessAt`, `lastLatencyMs`, and `lastErrorCode`, so a probe can never overwrite the operator's settings.

`MongoNewsSourceRepository` implements `NewsSourceRepository` against the legacy `news_sources` collection from section 6.7, behind `InMemoryNewsSourceRepository` for the local default. `_id` remains the deterministic source identifier string and equals `sourceId`. Seeded policy uses `$setOnInsert`, so an operator's edit survives a later seed; `updatedAt` remains monotonic, and health and manual-retry writes touch only their own fields. Feed URLs reject embedded credentials, credential-like query parameters, private or metadata IP literals, and non-loopback plain HTTP. Legacy edge values are normalized to safe defaults, while an unsafe stored feed URL disables that source instead of hiding the registry. Health errors are reduced to bounded diagnostic codes. The collection contains no secret field.

`MongoNewsItemRepository` implements `NewsItemRepository` against the legacy `news_items` collection from section 6.8, behind `InMemoryNewsItemRepository` for the local default. `_id` remains the deterministic `news_<sha1(canonicalUrl)>` string, so replaying a feed hits the same row instead of duplicating a story. Only the title and a 240-character excerpt are stored; the full article text never enters the record. `url` is the canonical identity and `originUrl` the untouched citation link, while `publishedAt`, `firstSeenAt`, and `ingestedAt` stay distinct and a missing publication time is never replaced by the ingestion time. A re-observation freezes `firstSeenAt` and `ingestedAt`; the envelope changes only when the material content fingerprint differs, and then `updatedAt` advances without moving backwards. Legacy rows with missing optional fields load with the legacy defaults, and an unknown schema stamp is rejected as corrupt. The two query indexes from the legacy model are created at startup; the retention TTL index stays owned by the legacy collection so this service never deletes rows.

`MongoAiBriefingRepository` implements `AiBriefingRepository` against the new `ai_briefings` collection from section 6.13, behind `InMemoryAiBriefingRepository` for the local default. The document uses camelCase fields and the `{ ownerScope, generatedAt }` read index. Publication uses `$setOnInsert`, so replaying an identifier cannot rewrite the first published briefing. Every read includes owner scope, and malformed records are rejected instead of served as valid evidence.

`MongoAiJobRepository` implements `AiJobRepository` against the new `ai_jobs` collection from section 6.14, behind `InMemoryAiJobRepository` for the local default. One document is both the owner-scoped request and the work item the AI Worker claims. It persists the server-generated correlation ID with the request. The unique `{ ownerScope, idempotencyKey }` index makes replay atomic; a claim increments the fencing token and takes an expiring lease, and every completion or failure checks the current lease holder and token. The API derives owner scope from the authenticated subject, and malformed records are rejected instead of served as valid work.

## 6. Configuration and security

Configuration is read from process environment variables with local development defaults. Production mode fails closed unless an explicit database URI, HTTPS TM Hub URL, and Hub application ID are supplied. Database URIs are redacted from debug output.

Current defaults and variables:

| Variable | Default | Purpose |
|---|---|---|
| `TM_ENVIRONMENT` | `development` | Runtime environment |
| `TM_DEPLOYMENT_MODE` | `local` | `local` enforces loopback API binding; `remote` permits an explicit non-loopback bind |
| `TM_API_BIND` | `127.0.0.1:4101` | Local API loopback bind |
| `TM_API_PREFIX` | `/api/v1` | Public API prefix |
| `TM_DATABASE_URI` | `mongodb://127.0.0.1:27017/?tls=true` | MongoDB connection |
| `TM_DATABASE_NAME` | `tm-trading-rust` | MongoDB database name |
| `TM_HUB_URL` | `http://127.0.0.1:4000` | TM Hub base URL |
| `TM_HUB_APP_ID` | unset | Dedicated Rust Hub application ID |
| `TM_LOG_FORMAT` | `compact` | `compact` or `json` tracing output |
| `TM_TRADING_STORE` | `memory` | `memory` or `mongodb` for operations, service snapshots, watchlists, providers, candles, research events, news sources, news items, AI provider profiles/models/routes, AI briefings, and AI jobs; production requires `mongodb` |
| `TM_TRADING_API_BIND` | `127.0.0.1:8787` | Management API bind; non-loopback is rejected in local mode |
| `TM_TRADING_LOCAL_API_TOKEN` | required | Local user bearer token; grants `trading.service.read`, `trading.service.control`, `trading.operation.read`, `trading.watchlist.read`, `trading.watchlist.write`, `trading.provider.read`, `trading.provider.write`, `trading.market.read`, `trading.research.read`, `trading.news.read`, `trading.ai.read`, and `trading.ai.write` |
| `TM_TRADING_AGENT_TOKEN` | required | Per-installation Agent-only bearer token; read from the environment, never the command line |
| `TM_TRADING_LOCAL_API_SUBJECT` | `local-operator` | Subject recorded on locally authenticated operations |
| `TM_TRADING_AGENT_ID` | `local-agent` | Agent identity the API expects on Agent endpoints |
| `TM_TRADING_API_URL` | unset | Management API base URL used by the Agent and Desktop client |
| `TM_TRADING_DESKTOP_DATA_DIR` | `<exe_dir>/data/desktop` | WebView2 profile directory for the Desktop app |
| `TM_TRADING_MARKET_WORKER_ENABLED` | unset | `1` registers the Market Worker with the Agent |
| `TM_TRADING_MARKET_SYMBOLS` | unset | Comma-separated symbols the Market Worker collects |
| `TM_TRADING_MARKET_TIMEFRAME` | `1m` | Market Worker candle timeframe |
| `TM_TRADING_MARKET_BARS` | `100` | Bars requested per collection cycle |
| `TM_TRADING_MARKET_CYCLE_MS` | `60000` | Delay between completed collection cycles |
| `TM_TRADING_MARKET_CYCLE_TIMEOUT_MS` | `30000` | Hard deadline for one collection cycle |
| `TM_TRADING_MARKET_HEARTBEAT_MS` | `5000` | Market Worker heartbeat interval |
| `TM_TRADING_MARKET_PROVIDER_ID` | unset | Provider definition the Market Worker uses; required when the enabled set is ambiguous |
| `TM_TRADING_MARKET_SOURCE_OF_TRUTH` | off | `1` authorizes candle writes; without it the worker exits with code `78` |
| `TM_TRADING_RESEARCH_WORKER_ENABLED` | unset | `1` registers the Research Worker with the Agent |
| `TM_TRADING_RESEARCH_OWNER_SCOPE` | unset | Required owner scope written to every research event |
| `TM_TRADING_RESEARCH_SYMBOLS` | unset | Comma-separated symbols the Research Worker scans |
| `TM_TRADING_RESEARCH_TIMEFRAME` | `1m` | Research candle timeframe |
| `TM_TRADING_RESEARCH_BARS` | `120` | Closed candles read per symbol and scan |
| `TM_TRADING_RESEARCH_CYCLE_MS` | `60000` | Delay between completed scan cycles |
| `TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS` | `30000` | Hard deadline for one research scan |
| `TM_TRADING_RESEARCH_HEARTBEAT_MS` | `5000` | Research Worker heartbeat interval |
| `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH` | off | `1` authorizes research event writes; without it the worker exits with code `78` |
| `TM_TRADING_NEWS_WORKER_ENABLED` | unset | `1` registers the News Worker with the Agent |
| `TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE` | `40` | Items kept per source and collection cycle (maximum `200`) |
| `TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE` | `25` | Sources polled per cycle (maximum `100`) |
| `TM_TRADING_NEWS_CYCLE_MS` | `60000` | Delay between completed collection cycles |
| `TM_TRADING_NEWS_CYCLE_TIMEOUT_MS` | `60000` | Hard deadline for one collection cycle |
| `TM_TRADING_NEWS_HEARTBEAT_MS` | `5000` | News Worker heartbeat interval |
| `TM_TRADING_NEWS_SOURCE_OF_TRUTH` | off | `1` authorizes news item writes; without it the worker exits with code `78` |
| `TM_TRADING_AI_WORKER_ENABLED` | unset | `1` registers the AI Worker with the Agent and permits briefing publication |
| `TM_TRADING_AI_CLAIM_MS` | `5000` | Delay between two AI job claim attempts |
| `TM_TRADING_AI_HEARTBEAT_MS` | `5000` | AI Worker heartbeat interval |
| `TM_TRADING_AI_LEASE_MS` | `300000` | Lease taken on one claimed AI job |
| `TM_TRADING_LOCALE` | `en` | Desktop initial locale (`en` or `vi`) |

TM Hub remains authoritative for human identity, sessions, RBAC, routes, and shared configuration. Until Hub verification is wired, the Management API authenticates two local bearer credentials with an exact-match comparison: a user credential that carries `trading.service.read`, `trading.service.control`, and `trading.operation.read`, and a separate Agent credential that may only act as the Agent named in the request. A missing or invalid credential is `401`; a credential used on an endpoint of the other kind is `403`. The Hub-enforced `trading.*` capability set documented in the TM Hub integration map replaces this local mode without changing the endpoint contracts.

The Desktop client reads its API base URL from `TM_TRADING_API_URL` and its user token from `TM_TRADING_LOCAL_API_TOKEN`. It never stores a token in a document, log, or UI-visible field.

## 7. Observability

Tracing uses `tracing` and `tracing-subscriber`. JSON output is available for structured local or cloud logs. Correlation spans carry a UUID without logging credentials or raw authorization headers.

The Agent now captures heartbeat timestamps, bounded log tails, exit codes, restart counters, PID/executable identity, and service-state projections. It refreshes readiness against the current time on every poll and evaluates heartbeat freshness at Agent receipt time, so a stale heartbeat cannot remain `READY`. The local checkpoint retains desired/observed state, process identity, the last exit code, restart count, restart schedule, and deterministic-failure retry block needed to reconcile after an Agent restart; MongoDB remains the target for durable operation and service-snapshot persistence. An explicit service stop persists desired `STOPPED`, while Agent-level shutdown stops the process but preserves desired `RUNNING` and any pending restart backoff without consuming the restart budget. The next slice must expose those snapshots and operation outcomes through the Management API.

## 8. Phase checklist

- [x] Pin Rust 1.99.1 with rustfmt and Clippy.
- [x] Create the Cargo workspace and commit `Cargo.lock`.
- [x] Add domain, contracts, config, observability, and storage crates.
- [x] Define API versioning and the stable error model.
- [x] Add MongoDB connection and test-repository abstractions.
- [x] Generate the first OpenAPI snapshot.
- [x] Add CI checks for formatting, Clippy, tests, release build, OpenAPI drift, dependency audit, and secret scanning.
- [x] Define the worker IPC protocol and static worker registry.
- [x] Implement deterministic lifecycle, readiness, heartbeat, log-tail, and restart-policy state transitions.
- [x] Implement the independent Host Agent and sample-worker executable.
- [x] Enforce one Agent owner per installation, Windows Job Object containment and Unix process-group termination, worker identity validation, bounded replacement startup, and local restart reconciliation.
- [x] Harden local lifecycle behavior with receipt-time heartbeat evaluation, persisted restart budgets and quarantine, bounded event draining and termination, and distinct explicit-stop versus Agent-shutdown semantics.
- [x] Implement the loopback Management API with exact-capability bearer auth and durable operations.
- [x] Implement the Agent heartbeat and claimed-command channel.
- [x] Implement the typed `api-client` crate over the public contracts.
- [x] Implement the Dioxus Desktop Control Center with Start/Stop/Restart operations and EN/VI localization.
- [x] Persist durable operations in the MongoDB `operations` collection with atomic idempotency and fenced transitions.
- [x] Persist service snapshots in the MongoDB `service_instances` projection, replaced per Agent heartbeat.
- [x] Port the closed-candle domain model and its memory/MongoDB repositories.
- [x] Implement public Binance spot and OKX swap candle adapters with bounded health checks.
- [x] Port the owner-scoped watchlist model and serve it through the Management API.
- [x] Implement the opt-in Market Worker with write-authority gating, bounded provider fallback, closed-bar-only persistence, and transactional multi-candle writes.
- [x] Implement the closed-candle Market Overview and Market Pulse API read models with explicit empty, partial, and stale states.
- [x] Add the Desktop Market Overview and Market Pulse panels with EN/VI localization.
- [x] Port the legacy technical-analysis primitives into `crates/research-engine` with hand-computed and legacy-generated goldens.
- [x] Port the canonical indicator registry with clamped parameters and legacy-matched outputs.
- [x] Port the session filter and the VSA volume buckets from `engine/ta.mjs`.
- [x] Port the VSA/Wyckoff method (method 0) with a legacy-generated scenario fixture.
- [x] Define the research event contract and preserve schema, producer, input, source, and quality provenance.
- [x] Add the memory/MongoDB research event repositories and owner-scoped research event API reads.
- [x] Implement the opt-in Research Worker with write-authority gating, bounded scans, stale-input rejection, provenance-preserving upserts, and success-only readiness.
- [x] Build the Desktop Signal Center from the owner-scoped research event API, with independent loading, empty, error, and stale states.
- [x] Add the research-event detail Slideover with computed freshness, direction, quality reasons, evidence, and full source/producer/schema/input provenance.
- [x] Add the legacy-compatible News source domain and memory/MongoDB repositories with insert-only policy seeds, independent health, and fail-closed license/rights handling.
- [x] Add the legacy-compatible News item domain and memory/MongoDB repositories with canonical-URL identity, frozen observation times, and material-change fingerprinting.
- [x] Add the bounded RSS/Atom/RDF feed adapter with safe-URL validation and secret-free health codes.
- [x] Implement the opt-in News Worker with interval-gated polls, per-source health, write-authority gating, and success-only readiness.
- [x] Serve owner-scoped news reads at `GET /api/v1/news`, `GET /api/v1/news/{news_id}`, and `GET /api/v1/news/sources` behind `trading.news.read`.
- [x] Build the Desktop News & Events panel and its story detail Slideover with independent loading, empty, error, and stale states and EN/VI localization.
- [x] Add the AI provider profile registry with bounded OpenAI-compatible health probes and exact read/write capabilities.
- [x] Add the AI model registry and task-route registry with validated provider/model chains and bounded execution budgets.
- [x] Define the evidence-bound AI briefing contract and memory/MongoDB repositories with insert-once publication and typed evidence references.
- [x] Serve owner-scoped AI briefing list/detail reads and build the Desktop AI Briefing panel and detail Slideover with EN/VI localization.
- [x] Run Windows crash, restart, readiness, and graceful-shutdown drills.

## 9. Verification

Required workspace commands:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

OpenAPI regeneration and drift check:

```powershell
cargo run -p tm-trading-contracts --example generate_openapi
git diff --exit-code -- docs/api/openapi.yaml
```

On Windows, if MSVC reports `D8050` because the inherited `TMP`/`TEMP` directory is not writable, point both variables at a writable workspace directory before running Cargo:

```powershell
New-Item -ItemType Directory -Force target/tmp
$env:TMP = (Resolve-Path target/tmp).Path
$env:TEMP = $env:TMP
```

If `cl.exe` still fails with `D8050` when Cargo launches it, run Cargo from a Developer Command Prompt environment and disable debug info for the dev profile:

```powershell
cmd /c '"C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && set CARGO_PROFILE_DEV_DEBUG=false && cargo test --workspace --locked'
```

This affects compilation only; it does not change shipped release builds.
