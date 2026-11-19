# Current System Architecture

> Status: Phase 1 foundation, Phase 2 local Agent/sample-worker runtime, Phase 3 local Management API, and Phase 4 Dioxus Desktop shell implemented; MongoDB persistence not yet complete<br>
> Last updated: 2026-11-19<br>
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
- an Agent command channel that heartbeats per-Agent service snapshots and executes claimed operations;
- a typed `api-client` crate that depends only on the versioned public contracts;
- a Dioxus Desktop Control Center with live service state, Start/Stop/Restart operations with in-flight status, connection state, and EN/VI localization.

Not implemented:

- TM Hub token verification and capability enforcement (local bearer credentials are used today);
- the persisted `service_instances` snapshot projection;
- durable Agent credential rotation;
- TM Hub login, TOTP, and refresh flow in the Desktop client.

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

tm-trading-agent-protocol -> tm-trading-supervisor-core -> tm-trading-agent
        |                                      |
        |                                      +-> local locked checkpoint
        +-> tm-trading-sample-worker -> worker.v2 JSON lines
```

Dependency rules:

- `domain` has no UI, HTTP, MongoDB, or provider dependencies.
- `contracts` contains public DTOs only and has no BSON models.
- `storage` implements repository behavior behind traits and translates errors at its boundary.
- `config` owns typed environment parsing and validation.
- `observability` owns tracing setup and correlation spans.
- `agent-protocol` owns the versioned worker IPC messages and explicit control commands.
- `supervisor-core` owns static worker definitions and deterministic lifecycle/readiness transitions; it has no OS process or UI dependency.
- `apps/agent` is the sole OS process owner for workers, uses a static executable registry, clears inherited child environment before adding an allowlist, and never accepts arbitrary executable paths or arguments. It holds an exclusive state-directory lock, validates the worker's PID, executable path, and executable version during the `worker.v2` handshake, uses a Windows Job Object or Unix process group, and isolates a rejected worker without stopping other workers. On Windows the worker is spawned with `DETACHED_PROCESS` so console control events reach only the Agent, which drains the worker over the explicit control channel. Windows Job Object descendant containment is implemented; Unix forced termination targets the process group, while descendant cleanup after an abrupt Agent loss remains unverified on a Unix runtime.
- `workers/sample-worker` implements the versioned control protocol, performs its capability self-check before readiness, and reports explicit drain status.
- `apps/management-api` is the local Axum Management API. It binds to loopback only, is a command producer and read-model provider, never spawns a process, and never runs a continuous worker loop. User endpoints require a bearer credential that carries exact `trading.*` capabilities; Agent endpoints require the separate per-installation Agent credential and may only act as the Agent named in the request. Operations are durable and idempotent, and are claimed with an expiring lease and fencing token.
- `apps/agent` additionally opens an outbound command channel to the Management API: it heartbeats its service read model, claims commands addressed to its own Agent identity, executes them against its own worker registry, and reports the real outcome. The Agent never accepts an executable path or command line from the API.
- `crates/api-client` is the only client-side HTTP boundary. It depends on `contracts` alone and exposes health, service listing, and operation creation/polling.
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

The current OpenAPI document defines the health/version paths, the service read model, durable operation endpoints, and the Agent heartbeat/command endpoints. It is a contract snapshot; the running API serves the same document at `/api/v1/openapi.json`.

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

The store is selected by `TM_TRADING_OPERATION_STORE`: `memory` (local default) or `mongodb`. Production fails closed rather than losing durable operations on restart.

Verified against a TLS-enabled MongoDB 8.2 instance with certificate validation against a local CA: create, restart persistence, idempotent replay, claim/lease/fencing, stale-result rejection, and expiry all pass, and all five expected indexes exist.

Service snapshots are still an in-memory read model. Their persisted projection in `service_instances` is not implemented yet.

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
| `TM_TRADING_OPERATION_STORE` | `memory` | `memory` or `mongodb`; production requires `mongodb` |
| `TM_TRADING_API_BIND` | `127.0.0.1:8787` | Management API bind; non-loopback is rejected in local mode |
| `TM_TRADING_LOCAL_API_TOKEN` | required | Local user bearer token; grants `trading.service.read`, `trading.service.control`, `trading.operation.read` |
| `TM_TRADING_AGENT_TOKEN` | required | Per-installation Agent-only bearer token; read from the environment, never the command line |
| `TM_TRADING_LOCAL_API_SUBJECT` | `local-operator` | Subject recorded on locally authenticated operations |
| `TM_TRADING_AGENT_ID` | `local-agent` | Agent identity the API expects on Agent endpoints |
| `TM_TRADING_API_URL` | unset | Management API base URL used by the Agent and Desktop client |
| `TM_TRADING_DESKTOP_DATA_DIR` | `<exe_dir>/data/desktop` | WebView2 profile directory for the Desktop app |
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
- [ ] Persist service snapshots in the `service_instances` projection.
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
