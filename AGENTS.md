# AGENTS.md — tm-trading-rust

Guidance for AI coding agents working in this repository.

## Authority

- Read `docs/TM-TRADING-RUST-MASTER-ROADMAP.md` completely before changing code.
- The roadmap is the authority for product scope, architecture, phases, dependencies, and acceptance criteria.
- The legacy repository `D:\Applications\tm-trading` is a migration reference only. Do not modify it from this project.
- Migration maps under `docs/migration/` are mandatory before domain code is ported.

## Product boundary

This project is information-first and read-only.

Never add:

- live/demo order placement, amendment, or cancellation;
- private exchange or broker trading credentials;
- order tickets, balances, positions, or portfolio execution controls;
- autonomous or AI-generated trading;
- an execution path from research signals to orders.

Legacy `exec/`, `simulation/`, exchange-demo, paper-order, bot-execution, and order-oriented APIs are excluded from the active product. Historical records may be read only when their meaning, provenance, and owner scope are explicit.

TM Hub remains the only authority for users, identities, sessions, roles, permissions, route catalogs, and shared application configuration.

## Language

All source code, comments, logs, identifiers, commit messages, and technical documentation must be in English. User-facing strings must use the UI localization layer.

## Architecture boundaries

- Use a Cargo workspace with a small number of well-defined processes.
- `domain` must not depend on UI, Axum, MongoDB, or provider SDKs.
- `contracts` must not contain database-specific BSON models.
- `api-client` may depend only on public versioned contracts.
- The API never spawns arbitrary processes and never contains continuous worker loops.
- Exactly one Host Agent owns worker processes on a host.
- Workers use a static registry; never accept shell strings or arbitrary executables from a client.
- Desktop and Web clients use the same `/api/v1` contracts and never connect directly to MongoDB.
- Provider and bot secrets stay in a secret store, never in ordinary documents, URLs, logs, diagnostics, or UI state.

## Reliability invariants

- Desired state, observed state, and readiness are separate. A running PID is not `READY`.
- Every state-changing operation has an operation ID, idempotency key, correlation ID, durable status, and bounded expiry.
- Queue claims are atomic and lease-protected; stale work expires instead of running later.
- Process liveness and functional readiness are checked separately.
- Windows graceful shutdown uses an explicit worker control channel, not an assumed `SIGTERM`.
- A skipped cycle is not a successful cycle.
- Unknown outcomes remain unknown; do not report an unobserved action as successful.
- UTC is used in storage and APIs. Keep observation, ingestion, publication, and update times distinct.
- Stored research output carries schema, producer, input, and quality provenance.

## Workflow

For every phase or vertical slice:

1. Inspect relevant code, contracts, tests, and data structures.
2. Map dependencies and compatibility requirements.
3. Implement the smallest complete slice.
4. Test new behavior first where practical.
5. Run formatting, Clippy, targeted tests, and workspace tests as feasible.
6. Compare old and new behavior with fixtures for ports.
7. Update architecture, API, migration, provider, and operations documentation.
8. Report changed files, exact command results, limitations, risks, and rollback.
9. Commit one coherent conventional commit per logical slice.

Never claim a command passed unless it was actually run and observed. Use only `PASS`, `FAIL`, `NOT RUN`, `BLOCKED`, and `SKIPPED` for result reporting.

## Git and safety

- Keep `main` buildable and work in small reviewable commits.
- Do not delete legacy data, indexes, files, or deployments as part of a replacement.
- Do not perform destructive production migrations or deploy unreviewed branches.
- If a required credential, live contract, provider permission, or production dataset is unavailable, implement the safe path and mark the external verification `BLOCKED` with the exact requirement.
- Do not weaken or suppress quality checks to obtain a green build.

## Required verification

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```
