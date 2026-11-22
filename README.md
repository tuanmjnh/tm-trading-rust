# TM Trading Rust

A Rust-first, API-first, Desktop-first market-intelligence and service-management platform.

The authoritative product and architecture specification is [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](docs/TM-TRADING-RUST-MASTER-ROADMAP.md).

## Current status

The implemented platform now includes the Axum Management API, typed API client, Dioxus Desktop Control Center, Market Overview/Market Pulse panels and Signal Center with a research-event detail Slideover, independent Host Agent, opt-in Market, Research, News, and AI workers, MongoDB-backed durable operations, provider configuration, owner-scoped watchlists and research-event reads, the deterministic TA/indicator/VSA research engine, the legacy-compatible news source registry and news item store, the RSS/Atom feed adapter with the opt-in News Worker, the owner-scoped news read API, the Desktop News & Events panel with its story detail Slideover, the AI provider/model/task-route registries, evidence-bound AI briefing storage and owner-scoped reads, the durable owner-scoped AI job queue, and the Desktop AI Briefing panel with its research-event request action and detail Slideover.

Phase 7 is implemented. The next milestone is Phase 8: the durable Telegram notification plane and notification center. The project continues to reuse the existing MongoDB data where practical and does not perform a destructive migration.

## Product boundary

TM Trading Rust is information-first and read-only. It observes public market data, preserves research evidence, produces read-only research events and summaries, and manages supporting services.

It does **not** provide:

- live or demo order placement, amendment, or cancellation;
- private exchange or broker credentials;
- balances, positions, order tickets, or portfolio execution;
- autonomous or AI-driven trading;
- a second user, role, or application registry.

TM Hub remains authoritative for identity, sessions, RBAC, routes, and shared configuration.

## Prerequisites

- Rust stable toolchain (the repository pins the supported version in `rust-toolchain.toml`)
- Node.js and pnpm only when running or comparing the legacy source project
- MongoDB when testing against existing data; unit tests do not require it
- Windows 10/11 x64 for the first Desktop release

## Workspace commands

The required verification commands are:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Regenerate and verify the API contract snapshot with:

```powershell
cargo run -p tm-trading-contracts --example generate_openapi
git diff --exit-code -- docs/api/openapi.yaml
```

The CI workflow also runs a RustSec dependency audit and a repository secret scan with pinned third-party actions.

Build and exercise the independent Agent/sample-worker runtime:

```powershell
cargo build -p tm-trading-agent -p tm-trading-sample-worker
$selfTestState = Join-Path (Resolve-Path target/tmp).Path ("agent-self-test-" + [guid]::NewGuid().ToString("N"))
.\target\debug\tm-trading-agent.exe --self-test --worker-dir .\target\debug --state-dir $selfTestState
$crashState = Join-Path (Resolve-Path target/tmp).Path ("agent-crash-test-" + [guid]::NewGuid().ToString("N"))
.\target\debug\tm-trading-agent.exe --crash-after-ms 100 --worker-dir .\target\debug --state-dir $crashState
```

The Agent holds an exclusive lock for its state directory and persists a small local process checkpoint there. Windows workers run in a kill-on-close Job Object; Unix workers run in a process group, with abrupt Unix descendant cleanup still requiring runtime verification. A second Agent using the same state directory fails closed.

No command is considered passed unless it was executed and its result observed. Reports use `PASS`, `FAIL`, `NOT RUN`, `BLOCKED`, or `SKIPPED`.

## Documentation

- [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](docs/TM-TRADING-RUST-MASTER-ROADMAP.md) — product and architecture authority
- [`docs/architecture/system-architecture.md`](docs/architecture/system-architecture.md) — current implemented architecture and phase status
- [`docs/migration/source-inventory.md`](docs/migration/source-inventory.md) — audited legacy source inventory
- [`docs/migration/module-mapping.md`](docs/migration/module-mapping.md) — source-to-target mapping
- [`docs/migration/data-contract-map.md`](docs/migration/data-contract-map.md) — MongoDB and event contract map
- [`docs/migration/tm-hub-integration-map.md`](docs/migration/tm-hub-integration-map.md) — TM Hub integration contract
- [`docs/api/openapi.yaml`](docs/api/openapi.yaml) — generated API contract snapshot
- [`docs/operations/service-catalog.md`](docs/operations/service-catalog.md) — registered worker definitions and readiness probes
- [`docs/operations/runbooks.md`](docs/operations/runbooks.md) — Agent, crash/restart, and graceful-shutdown procedures

## Development rules

See [`AGENTS.md`](AGENTS.md). The legacy repository at `D:\Applications\tm-trading` is a read-only migration reference during the port; it is not modified by this project.
