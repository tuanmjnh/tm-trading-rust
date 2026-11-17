# TM Trading Rust

A Rust-first, API-first, Desktop-first market-intelligence and service-management platform.

The authoritative product and architecture specification is [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](docs/TM-TRADING-RUST-MASTER-ROADMAP.md).

## Current status

The Phase 0 migration audit is complete. The first implementation milestone will prove this path:

```text
Dioxus Desktop -> Axum Management API -> independent Host Agent -> sample worker
```

The project will reuse the existing MongoDB data where practical and will not perform a destructive migration.

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

Once the Cargo workspace is available, the required verification commands are:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

No command is considered passed unless it was executed and its result observed. Reports use `PASS`, `FAIL`, `NOT RUN`, `BLOCKED`, or `SKIPPED`.

## Documentation

- [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](docs/TM-TRADING-RUST-MASTER-ROADMAP.md) — product and architecture authority
- [`docs/migration/source-inventory.md`](docs/migration/source-inventory.md) — audited legacy source inventory
- [`docs/migration/module-mapping.md`](docs/migration/module-mapping.md) — source-to-target mapping
- [`docs/migration/data-contract-map.md`](docs/migration/data-contract-map.md) — MongoDB and event contract map
- [`docs/migration/tm-hub-integration-map.md`](docs/migration/tm-hub-integration-map.md) — TM Hub integration contract

## Development rules

See [`AGENTS.md`](AGENTS.md). The legacy repository at `D:\Applications\tm-trading` is a read-only migration reference during the port; it is not modified by this project.
