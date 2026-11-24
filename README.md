# TM Trading Rust

A Rust-first, API-first, Desktop-first market-intelligence and service-management platform.

The authoritative product and architecture specification is [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](docs/TM-TRADING-RUST-MASTER-ROADMAP.md).

## Current status

The implemented platform now includes the Axum Management API, typed API client, Dioxus Desktop Control Center, Market Overview/Market Pulse panels and Signal Center with a research-event detail Slideover, independent Host Agent, opt-in Market, Research, News, and AI workers, MongoDB-backed durable operations, provider configuration, owner-scoped watchlists and research-event reads, the deterministic TA/indicator/VSA research engine, the legacy-compatible news source registry and news item store, the RSS/Atom feed adapter with the opt-in News Worker, the owner-scoped news read API, the Desktop News & Events panel with its story detail Slideover, the AI provider/model/task-route registries, evidence-bound AI briefing storage and owner-scoped reads, the durable owner-scoped AI job queue, and the Desktop AI Briefing panel with its research-event request action and detail Slideover.

Phase 7's core news and AI briefing deliverables are implemented, including dedicated Anthropic Messages and Gemini `generateContent` adapters; OpenAI and OpenRouter currently use the OpenAI-compatible adapter. Phase 8 delivers the complete multichannel notification system: domain models, durable outbox queue and audit logs, Telegram transport client, supervised notification worker, Management API endpoints, and Desktop Notification Center panel. Phase 9 completes the local desktop release hardening: secret-redacted system diagnostics, support bundle export, Microsoft Edge WebView2 runtime checks, configuration backup/restore with SHA-256 integrity verification, automated Windows release packager (`scripts/package-desktop-windows.ps1`), and detailed operational runbooks. The project continues to reuse the existing MongoDB data where practical and does not perform a destructive migration.

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

### Running the Desktop UI in Development

You can run the entire system (Backend API + Desktop UI) in a **single command**, or run them separately:

#### Option A: 1-Click Unified Runner (Recommended)
Automatically checks and spins up the backend Management API on `127.0.0.1:8787`, sets up tokens, and launches the Desktop UI:

```powershell
# Windows (PowerShell) - Single command to start both API & UI:
.\scripts\dev.ps1

# With Auto-Reload / File Watcher enabled:
.\scripts\dev.ps1 -Watch

# Switch language to English:
.\scripts\dev.ps1 -Locale en
```

On Linux / macOS:
```bash
./scripts/dev.sh
```

#### Option B: Manual Multi-Terminal Runner
If you prefer running components in isolated terminals:

```powershell
# Terminal 1: Run the Management API backend on loopback (port 8787)
cargo run -p tm-trading-management-api

# Terminal 2: Launch the Desktop UI
cargo run -p tm-trading-desktop
```

#### Option C: Native Dioxus Hot-Reload (`dx serve`)
To enable instant template and CSS hot-reloading without re-compilation, install the official Dioxus CLI:
```powershell
cargo install dioxus-cli --locked
cd apps/desktop
dx serve --platform desktop
```
*(The configuration file `apps/desktop/Dioxus.toml` is pre-configured for desktop hot-reloading).*

#### Option D: Workspace Cleanup & Disk Optimization
Cargo build caches can accumulate gigabytes of debug symbols and intermediate objects over time. You can clean up with:
```powershell
# Quick clean of target/ and temporary logs:
.\scripts\clean.ps1

# Full clean including dist/ release bundles:
.\scripts\clean.ps1 -Full

# Or clean before dev run:
.\scripts\dev.ps1 -Clean
```
*Note: The project's root `Cargo.toml` is configured with `[profile.dev.package."*"] debug = 1` to prevent multi-gigabyte PDB debug symbol bloat from third-party dependencies while preserving full debuginfo for workspace crates.*

#### Environment Variables for Desktop Development:
- `TM_TRADING_API_URL`: Backend API URL (default: `http://127.0.0.1:8787`).
- `TM_TRADING_LOCAL_API_TOKEN`: Local operator bearer token for authenticated commands.
- `TM_TRADING_LOCALE`: UI language (`en` for English, `vi` for Vietnamese, default: `en`).
- `TM_DATABASE_URI`: MongoDB connection URI (default in dev scripts: `mongodb://127.0.0.1:27017`).
- `TM_DATABASE_NAME`: Database name (default: `tm-trading-rust`).
- `TM_STORAGE_ALLOW_INSECURE_DEV`: Set to `1` or `true` in local development to permit non-TLS MongoDB connections and activate automatic dev fallback configurations.
- `TM_TRADING_DESKTOP_DATA_DIR`: Custom data cache directory for WebView2 profile storage.
- `RUST_LOG`: Log level filter (e.g. `info`, `debug`, `trace`).

```powershell
# Example: Launch Desktop UI with Vietnamese language and custom API endpoint
$env:TM_TRADING_LOCALE="vi"
$env:TM_TRADING_API_URL="http://127.0.0.1:8787"
cargo run -p tm-trading-desktop
```

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
- [`docs/deployment/cross-platform-build.md`](docs/deployment/cross-platform-build.md) — multi-platform compilation guide (Windows, Linux, macOS)
- [`docs/deployment/desktop-windows.md`](docs/deployment/desktop-windows.md) — Windows Desktop release deployment and packaging guide
- [`docs/operations/service-catalog.md`](docs/operations/service-catalog.md) — registered worker definitions and readiness probes
- [`docs/operations/runbooks.md`](docs/operations/runbooks.md) — operational runbooks (diagnostics, support bundles, backup/restore, shutdown)
- [`docs/ai/providers-and-models.md`](docs/ai/providers-and-models.md) — AI provider profiles, adapter protocols, and models
- [`docs/migration/source-inventory.md`](docs/migration/source-inventory.md) — audited legacy source inventory
- [`docs/migration/module-mapping.md`](docs/migration/module-mapping.md) — source-to-target mapping
- [`docs/migration/data-contract-map.md`](docs/migration/data-contract-map.md) — MongoDB and event contract map
- [`docs/migration/tm-hub-integration-map.md`](docs/migration/tm-hub-integration-map.md) — TM Hub integration contract
- [`docs/api/openapi.yaml`](docs/api/openapi.yaml) — generated API contract snapshot

## Development rules

See [`AGENTS.md`](AGENTS.md). The legacy repository at `D:\Applications\tm-trading` is a read-only migration reference during the port; it is not modified by this project.
