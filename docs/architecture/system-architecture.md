# Current System Architecture

> Status: Phase 1 foundation and Phase 2 protocol/lifecycle core implemented; runtime not yet complete<br>
> Last updated: 2026-11-17<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## 1. Current scope

The repository currently contains the shared Rust foundation needed by the first Desktop-to-Agent vertical slice. It does not yet contain an executable API, Agent, Desktop client, or worker.

Implemented:

- a pinned Rust workspace and locked dependency graph;
- domain lifecycle and durable-operation invariants;
- versioned API DTOs, a stable error model, and a generated OpenAPI snapshot;
- typed environment configuration with production fail-closed checks;
- tracing initialization and correlation spans;
- a MongoDB connection boundary and an in-memory operation repository for tests;
- a versioned JSON-line worker IPC protocol with explicit shutdown and drained status;
- a static worker registry plus deterministic desired/observed/readiness state transitions, bounded logs, heartbeat freshness, and bounded restart policy.

Not implemented:

- Axum Management API and HTTP authentication;
- TM Hub token verification and capability enforcement;
- Host Agent executable, OS process lifecycle, or sample worker executable;
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

tm-trading-agent-protocol -> tm-trading-supervisor-core -> future tm-trading-agent
```

Dependency rules:

- `domain` has no UI, HTTP, MongoDB, or provider dependencies.
- `contracts` contains public DTOs only and has no BSON models.
- `storage` implements repository behavior behind traits and translates errors at its boundary.
- `config` owns typed environment parsing and validation.
- `observability` owns tracing setup and correlation spans.
- `agent-protocol` owns the versioned worker IPC messages and explicit control commands.
- `supervisor-core` owns static worker definitions and deterministic lifecycle/readiness transitions; it has no OS process or UI dependency.
- Executable processes will be thin composition roots in later phases.

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

The later Agent slice must add heartbeat, bounded log tails, exit codes, restart counters, and service-state reporting. Process liveness must not be reported as functional readiness.

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
- [ ] Implement the independent Host Agent and sample-worker executable.
- [ ] Persist operations and service snapshots through the Management API.
- [ ] Run crash, restart, readiness, and graceful-shutdown drills.

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
