# Current System Architecture

> Status: Phase 1 foundation and Phase 2 local Agent/sample-worker runtime implemented and hardened; API persistence not yet complete<br>
> Last updated: 2026-11-18<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## 1. Current scope

The repository currently contains the shared Rust foundation and an independent local Host Agent with one sample worker. It does not yet contain an executable Management API or Desktop client.

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
- a JSON-line sample worker with handshake, readiness, heartbeat, cycle, operation, and graceful-shutdown messages.

Not implemented:

- Axum Management API and HTTP authentication;
- TM Hub token verification and capability enforcement;
- Agent-to-Management-API polling and durable Agent credential rotation;
- MongoDB operation repository and service snapshot persistence;
- Dioxus Desktop client.

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
                             ^
                             |
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
- `apps/agent` is the sole OS process owner for workers, uses a static executable registry, clears inherited child environment before adding an allowlist, and never accepts arbitrary executable paths or arguments. It holds an exclusive state-directory lock, validates the worker's PID, executable path, and executable version during the `worker.v2` handshake, uses a Windows Job Object or Unix process group, and isolates a rejected worker without stopping other workers. Windows Job Object descendant containment is implemented; Unix forced termination targets the process group, while descendant cleanup after an abrupt Agent loss remains unverified on a Unix runtime.
- `workers/sample-worker` implements the versioned control protocol, performs its capability self-check before readiness, and reports explicit drain status.
- Executable processes remain thin composition roots around these crates.

## 3. Contracts and versioning

- Public endpoints use the `/api/v1` prefix.
- The API version is `v1`; the current contract version is `1.0.0`.
- Timestamps use UTC `chrono::DateTime<Utc>` values and serialize as RFC 3339 JSON timestamps.
- API errors use stable typed codes and include a correlation ID.
- The generated contract snapshot is [`docs/api/openapi.yaml`](../api/openapi.yaml).

The current OpenAPI document defines the initial health/version paths and the operation DTO schemas. It is a contract snapshot, not evidence that an HTTP server is running.

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

The production MongoDB operation repository is not implemented yet. Its first implementation must use the collection and field mapping in [`docs/migration/data-contract-map.md`](../migration/data-contract-map.md), atomic find-and-update claims, lease expiry, and unique idempotency keys. No destructive migration is permitted.

Live collection names, indexes, TTL settings, validators, representative documents, and an actual TLS handshake remain `BLOCKED` until read-only target-environment inspection is available. The current TLS unit tests validate connection-policy parsing only and do not claim a live certificate handshake.

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

TM Hub remains authoritative for human identity, sessions, RBAC, routes, and shared configuration. The Rust API must later enforce the exact `trading.*` capabilities documented in the TM Hub integration map.

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
- [ ] Persist operations and service snapshots through the Management API.
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
