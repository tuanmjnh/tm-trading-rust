# TM Trading Rust — Master Architecture & Implementation Roadmap

> **Project:** `tm-trading-rust`<br>
> **Document:** Master Product, Architecture, Migration, Deployment & AI-Agent Implementation Specification`<br>
> **Version:** 1.0`<br>
> **Date:** 2026-11-17`<br>
> **Primary target:** Windows Desktop first; Cloud Services and Web Dashboard later`<br>
> **Language policy:** All source code, code comments, logs, identifiers and technical documentation written by coding agents must be in English. User-facing Vietnamese/English strings must be localized through the UI localization layer.
> **Product authority:** This document is the authoritative direction for the new `tm-trading-rust` repository. Existing `tm-trading` V5.0 and V4.2 documents are migration references, not a reason to recreate the old Nuxt/Nitro architecture.

---

## 0. Instructions for AI coding agents

This document is intended to be sufficient for an AI coding agent to plan and implement the project without asking the owner to make routine architecture or technology decisions.

### 0.1 Mandatory execution rules

1. Read this entire document before modifying code.
2. Inspect the existing `tm-trading` repository and its V5.0/V4.2 technical documents when access is available. Treat the existing system as a source of business logic, contracts, data schemas, tests and operational lessons—not as code that must be copied verbatim.
3. The new repository is named **`tm-trading-rust`**. Do not rename it to another project name.
4. Desktop-first is the implementation priority. Do not start by building a complete Web Dashboard or cloud microservice fleet.
5. Design every shared capability so it can later run locally or remotely through the same versioned API and contracts.
6. Prefer a modular Cargo workspace and a small number of well-defined processes. Do not create one process per tiny function.
7. Reuse the existing MongoDB database and compatible collection/schema contracts where practical. Do not perform destructive data migrations during the initial port.
8. Keep TM Hub authoritative for shared app identity, authentication/session, IAM, RBAC and shared app configuration. Do not implement a second user/role administration system inside TM Trading.
9. Keep provider credentials and Telegram bot tokens out of source control, frontend state, ordinary database documents, URLs, diagnostics and logs.
10. Never claim a command, test, migration, provider test, service drill, build or deployment passed unless it was actually executed and its result was observed.
11. Record incomplete work as `NOT RUN`, `BLOCKED` or `SKIPPED`, with a reason and next action. Do not hide failing tests or delete tests to make a build green.
12. Do not delete legacy code, collections, indexes, files or deployment configuration merely because the Rust replacement exists. First prove that it has no remaining references or data dependencies and record a rollback plan.
13. Do not ask the owner to choose among options already decided in this document. Use the defaults below. If an external credential, paid account, account authorization, production data migration or irreversible action is required, implement the safe code path and report the exact blocker rather than inventing credentials or performing the irreversible action.
14. Work in small, reviewable commits. Each phase must build and test before the next phase begins.
15. Keep the product information-first and read-only. Do not add order placement, order amendment/cancellation, exchange trading credentials, portfolio execution, live/demo execution, or autonomous trading capabilities. Preserve market observations and research evidence only.

### 0.2 Default decisions (do not ask again)

| Decision | Fixed default |
|---|---|
| Project name | `tm-trading-rust` |
| First release platform | Windows 10/11 x64 Desktop |
| Desktop UI framework | Dioxus Desktop 0.7.x |
| Future Web UI | Dioxus Web / WebAssembly using shared UI/domain code where supported |
| Backend framework | Axum 0.8.x |
| Async runtime | Tokio |
| Serialization/contracts | Serde + versioned DTOs + OpenAPI generated with Utoipa |
| Database | Existing MongoDB; keep collection compatibility first |
| Desktop-local API | Axum bound to `127.0.0.1` only |
| Service process owner | One Rust Agent/Supervisor per host |
| Agent-to-cloud communication | Outbound HTTPS polling initially; WSS optional after the polling protocol is stable |
| Frontend hosting later | Cloudflare Pages static hosting candidate |
| Persistent cloud worker candidate | Oracle Cloud Always Free VM if capacity and account eligibility permit; otherwise local PC during development or a low-cost Linux VPS |
| Burst / bounded jobs | Cloud Run Jobs or request-based Cloud Run services when workload fits its limits |
| Messaging in first release | MongoDB-backed durable operations/outbox with atomic claims and leases |
| Future message broker | NATS JetStream only when real scale/replay/fan-out needs justify its operations cost |
| Market data integrations | Public/read-only market-data endpoints only; initial adapters Binance public market data and OKX public market data, subject to endpoint and terms verification |
| News integrations | RSS/Atom first, then source-specific adapters |
| AI providers | Generic OpenAI-compatible provider first, then dedicated adapters where protocol differences require them |
| Initial AI provider compatibility | OpenRouter, OpenAI-compatible custom endpoints, OpenAI, Anthropic, Google Gemini, and local OpenAI-compatible servers such as Ollama/LM Studio/vLLM when compatible |
| Telegram | Outbound Bot API notifications through a dedicated Notification Worker and durable outbox |
| Auth/IAM | TM Hub integration; local development fallback must be explicitly development-only |
| UI realtime | SSE for server-to-client updates; bounded polling fallback |
| Production secret handling | OS credential store locally; host secret manager/environment injection in cloud |
| Time standard | UTC in storage and APIs; localized display only in UI |
| Default architecture style | Modular monolith + independently managed worker processes; do not start with microservices everywhere |

### 0.3 First vertical slice

The first working milestone must contain:

- A Rust/Dioxus Desktop window.
- A local Axum Management API.
- A Local Agent/Supervisor process independent of the Desktop window.
- One sample worker process with start/stop/status/log behavior.
- A versioned API contract shared by Desktop and future Web.
- A MongoDB connection abstraction with a test implementation.
- Structured logs, service state, readiness, operation IDs and graceful shutdown.
- A clean Windows release build.

Do not begin by porting every market provider or rebuilding all dashboard panels. Prove the process and API architecture first.

---

# 1. Product definition

## 1.1 Product purpose

`tm-trading-rust` is a Rust-first market intelligence, signal research and service-management application. Its main purpose is to collect public market information, normalize and preserve evidence, produce read-only research events and AI summaries, notify the owner, and provide a dependable interface for managing all supporting services.

The product has two user-facing clients over time:

1. **Desktop App:** first deliverable; primarily used on the owner's Windows PC.
2. **Web Dashboard:** later deliverable; remotely views and manages local/cloud service instances through the same Management API.

The backend must not depend on either UI being open.

## 1.2 Product scope

### In scope

- Public market-data collection and normalization.
- Market overview, asset watchlists and market pulse.
- Deterministic research indicators/events, with source and data provenance.
- News collection, deduplication, categorization and source references.
- Evidence-based AI market/news summaries and briefings.
- AI provider/model configuration, health checks and bounded fallback.
- Market-data and news provider configuration, capability display and connection tests.
- Telegram Bot notifications and future notification adapters.
- Local/cloud Agent registration and service lifecycle management.
- Service status, readiness, heartbeat, logs, operations history and resource metrics.
- Storage usage, retention settings, backup/restore tooling and diagnostics.
- TM Hub authentication and permission integration.
- Multi-host operation: one user can have local and cloud Agents managed from the same clients.

### Explicitly out of scope

- Live or demo order placement, amendment or cancellation.
- Broker/exchange private trading credentials or account execution permissions.
- Order tickets, balances, open-position management and portfolio execution controls.
- Autonomous trading or AI-generated order execution.
- Connecting market research signals to an order submission pipeline.
- Duplicating TM Hub's user, role, permission or central app registry functions.
- Replacing MongoDB merely because the application language changes.
- Premature Kubernetes, Redis, Kafka or a large microservice fleet.

Legacy `exec/`, `simulation/` and order-oriented pages in the former project must be quarantined and audited. Do not migrate order execution into this project. Historical read-only research records may be retained only when their meaning and provenance are clear.

## 1.3 Product principles

1. **Information-first:** a signal is an observation/research event, not an instruction to transact.
2. **Evidence-first:** every market value, news item and AI statement must retain source, timestamps and input evidence.
3. **Service-first:** the core system must remain healthy without the Desktop window running.
4. **Client-independent:** Desktop and Web are clients, not owners of background work.
5. **Local-first, cloud-ready:** start on Windows; later move selected service instances to Linux without changing domain contracts.
6. **One source of identity:** TM Hub remains central identity and RBAC authority.
7. **Observable by default:** process liveness is not readiness; every operation has an ID and auditable result.
8. **Fail closed:** missing secrets, invalid permissions and stale or invalid data must never be silently treated as healthy.

---

# 2. Target architecture

## 2.1 Logical architecture

```text
                 +---------------------------+
                 |       Desktop App         |
                 | Dioxus Desktop (Rust UI)  |
                 +-------------+-------------+
                               |
                 Versioned HTTPS/JSON API
                               |
                 +-------------v-------------+
                 |   Management API (Axum)   |
                 | Auth / RBAC / Read Models |
                 | Providers / Operations    |
                 | Events / Audit            |
                 +-------------+-------------+
                               |
                    Durable command/outbox
                    + heartbeat/event store
                               |
                 +-------------v-------------+
                 |      Host Agent           |
                 | Registry / Lifecycle      |
                 | Health / Logs / Recovery  |
                 +---+---------+---------+---+
                     |         |         |
              +------v--+ +----v---+ +---v--------+
              | Market  | | News   | | Research   |
              | Worker  | | Worker | | Worker     |
              +---------+ +--------+ +------------+
                     |         |         |
              +------v--+ +----v---+ +---v--------+
              | AI      | | Notify | | Storage /  |
              | Worker  | | Worker | | Maintenance|
              +---------+ +--------+ +------------+
                     \         |         /
                      \        |        /
                   +---v-------v--------v---+
                   |  MongoDB / Data Layer  |
                   | Domain data, events,   |
                   | operations, outbox     |
                   +-----------------------+

 Future:
 Desktop App ----+
                 +---- Cloud Management API ---- Cloud Agent(s) ---- Workers
 Web Dashboard -+
```

## 2.2 Components and responsibility boundaries

### Desktop App

- Renders Dashboard, Signal Center, Watchlist, News, AI Briefing and Control Center.
- Uses `api-client` contracts to call local or remote Management API.
- Can store multiple named server/workspace profiles.
- Does not directly access MongoDB, provider credentials or worker ports.
- Does not own worker loops. Closing the UI must not stop background services.

### Management API

- Exposes the versioned API shared by Desktop and Web.
- Authenticates users through TM Hub integration or validated Hub sessions/tokens.
- Enforces authorization and resource ownership on every operation.
- Serves read models and provider/service metadata.
- Creates durable operation records and command messages.
- Does not directly spawn arbitrary OS processes from request handlers.
- Does not contain continuous market/news/AI loops.
- Can be hosted locally at first and remotely later.

### Host Agent / Supervisor

- Runs on each machine that hosts one or more workers.
- Registers its identity, capabilities and software version.
- Connects outbound to the Management API when remote mode is enabled.
- Owns worker process lifecycle on that host.
- Claims only commands addressed to itself and allowed by its capabilities.
- Reports heartbeat, process state, readiness, metrics and operation results.
- Applies bounded restart policy and graceful shutdown.
- Does not accept arbitrary shell commands from the API or UI.

### Workers

- Run a narrowly scoped domain function.
- Use shared domain contracts and repositories.
- Emit structured events and persist useful checkpoints.
- Report startup, heartbeat, output freshness and readiness.
- Have explicit timeouts, concurrency limits and graceful shutdown.
- Do not expose public administrative ports.

### Data Layer

- MongoDB remains the initial shared database.
- Repositories are isolated behind Rust traits/modules so the storage engine can evolve later.
- UI never connects to MongoDB directly.
- Secrets are not stored as ordinary provider configuration fields.

## 2.3 Deployment modes

The same codebase must support these modes without changing domain behavior.

| Mode | Description | Priority |
|---|---|---|
| `local-dev` | Desktop + local API + Agent + selected workers; development database/config | P0 |
| `local-desktop` | Installed Desktop + local API + Windows Agent + workers | P1 |
| `hybrid` | Desktop connects to Cloud Management API; local Agent and cloud Agents can coexist | P2 |
| `cloud` | Web and Desktop connect to remote API; Linux Agents run cloud workers | P3 |

A user may operate more than one Agent at once. A service definition is logical; a `ServiceInstance` identifies the process deployed on one Agent.

---

# 3. Technology stack

## 3.1 Fixed stack

| Layer | Selected technology | Use |
|---|---|---|
| Desktop UI | Dioxus Desktop 0.7.x | Rust UI for Windows first |
| Future Web UI | Dioxus Web / WASM | Reuse shared components and Rust view logic where possible |
| Backend HTTP | Axum 0.8.x | Local and cloud Management API |
| Async runtime | Tokio | Async tasks, timers, process supervision, sockets |
| API schema | Serde + Utoipa/OpenAPI | Versioned JSON DTOs and generated API documentation |
| HTTP client | Reqwest | TM Hub, provider REST APIs, Telegram Bot API |
| WebSocket client | Tokio Tungstenite | Public market-data streams and optional Agent WSS |
| Database | MongoDB Rust Driver 3.x | Existing MongoDB database and collections |
| Structured logging | Tracing + tracing-subscriber | Logs, spans and correlation IDs |
| Metrics | Prometheus exposition or OpenTelemetry metrics | Host/service observability; add only where useful |
| Local secret storage | `keyring` crate / Windows Credential Manager | Local provider and bot credentials |
| Configuration | Figment or typed Serde config loader | Environment, file and defaults; choose one after a small POC |
| Error handling | `thiserror` for domain errors; `anyhow` at executable boundaries | Consistent errors |
| IDs | UUID/ULID as appropriate | Resource, event and operation IDs |
| Time | `time` or `chrono` with UTC discipline | Timestamps and interval calculations |
| Container | Docker/OCI images | Linux workers/API packaging |
| Windows service | Windows Service integration later | Optional boot-time Agent mode |

### 3.2 Dioxus decision

Use **Dioxus** rather than egui for this project because the product requires Desktop first and a Web Dashboard later with a high level of shared UI and domain logic.

Important qualification: Dioxus Desktop renders through the operating system's WebView. The application logic and UI components are authored in Rust/RSX, but the rendering engine is not a custom pure-native widget renderer. This is an intentional trade-off for reusing the same component approach in Dioxus Web. Validate Windows WebView2 availability, packaging and performance in the first spike.

Do not introduce a second JavaScript SPA for the Web version unless a documented technical blocker proves Dioxus Web unsuitable. If a Web-only integration requires JavaScript, isolate it behind a small adapter and keep business logic/contracts in Rust.

### 3.3 Versioning policy

As of 2026-11-17, the research baseline found Dioxus 0.7.10, Axum 0.8.9, MongoDB Rust Driver 3.10.x documentation, Utoipa 6.x, `tracing` 0.1.x and `tokio-tungstenite` 0.30.x. These are research snapshots, not permission to float dependencies.

Implementation agent must:

1. Verify current stable compatible crate releases at implementation time.
2. Select mutually compatible versions and record them in workspace `Cargo.toml`.
3. Commit `Cargo.lock` for applications and deployment binaries.
4. Avoid wildcard dependency versions.
5. Run `cargo update` only in a dedicated dependency-update change with test results.
6. Pin the Rust toolchain in `rust-toolchain.toml` to a stable release supported by all selected dependencies.

### 3.4 Rules for dependency selection

- Prefer maintained crates with recent releases, documented security posture and platform support.
- Do not add Redis/NATS/Kafka/Kubernetes to the first release without a measured requirement.
- Do not add a second ORM or database driver.
- Prefer generic provider adapters over one crate per provider.
- Keep platform-specific APIs behind `platform` modules.
- Use `rustls` for TLS where compatible; avoid disabling certificate validation.

---

# 4. Cargo workspace and repository layout

Use a Cargo workspace. Start with only packages needed for the first vertical slice; create the rest as implementation progresses.

```text
tm-trading-rust/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── README.md
├── AGENTS.md
├── .env.example
├── .github/
│   └── workflows/
│       ├── ci.yml
│       ├── release-windows.yml
│       └── build-containers.yml
│
├── apps/
│   ├── desktop/                 # Dioxus Desktop executable
│   ├── web/                     # Dioxus Web executable, later phase
│   ├── management-api/          # Axum API executable
│   └── agent/                   # Host Agent/Supervisor executable
│
├── workers/
│   ├── market-worker/
│   ├── research-worker/
│   ├── news-worker/
│   ├── ai-worker/
│   ├── notification-worker/
│   └── maintenance-worker/
│
├── crates/
│   ├── domain/                  # Domain types, invariants and value objects
│   ├── contracts/               # API DTOs, events, versioned schemas
│   ├── api-client/              # Typed client used by Desktop and Web
│   ├── auth/                    # TM Hub integration and authorization helpers
│   ├── storage/                 # MongoDB repositories and indexes
│   ├── config/                  # Typed config and validation
│   ├── observability/           # Tracing, metrics and correlation IDs
│   ├── agent-protocol/          # Registration, heartbeat, commands and results
│   ├── supervisor-core/         # Process lifecycle and readiness state machine
│   ├── provider-core/           # Provider registry, capabilities and health
│   ├── market-providers/        # Market source adapters
│   ├── news-providers/          # RSS/API news source adapters
│   ├── ai-providers/            # LLM provider adapters and model registry
│   ├── telegram-client/         # Telegram Bot API client
│   ├── notification-core/       # Notification rules, outbox and deduplication
│   ├── research-engine/         # Deterministic read-only research calculations
│   └── ui-components/           # Shared Dioxus components where targets permit
│
├── config/
│   ├── defaults/
│   ├── schemas/
│   └── providers/
├── deploy/
│   ├── docker/
│   ├── windows/
│   ├── linux/
│   ├── cloudflare-pages/
│   └── runbooks/
├── migrations/
├── scripts/
├── fixtures/
├── tests/
└── docs/
    ├── architecture/
    ├── api/
    ├── providers/
    ├── operations/
    ├── deployment/
    ├── migration/
    └── decisions/
```

### Workspace design rules

- `domain` must not depend on UI, Axum, MongoDB or provider SDKs.
- `contracts` must not contain database-specific BSON models.
- `api-client` consumes only public versioned contracts.
- `supervisor-core` must not depend on Dioxus or browser code.
- Provider adapter crates implement traits from `provider-core`.
- Workers consume domain services and repositories; they do not duplicate business rules.
- Keep executable `main.rs` files thin: load config, initialize tracing, wire dependencies, run and shut down.

---

# 5. Service runtime and Agent design

## 5.1 One Agent/Supervisor owns each host

Every machine runs exactly one active TM Trading Agent instance per installation/host identity. The Agent manages the worker processes assigned to that machine.

The Management API is a command producer and read-model provider. It must not spawn OS processes directly from an HTTP handler. The Agent owns process lifecycle locally.

## 5.2 Service inventory

Initial service groups:

| Service ID | Responsibility | Dependencies | Initial host |
|---|---|---|---|
| `market-worker` | Public price/candle/volume/funding/open-interest observations where sources support them | Market provider instances, storage | Local Windows |
| `research-worker` | Deterministic scanners, research events, quality/provenance | Market observations, storage | Local Windows |
| `news-worker` | RSS/API collection, normalization, deduplication, source metadata | News providers, storage | Local Windows |
| `ai-worker` | Evidence-bound summaries, classification, market/news briefings | AI providers, research/news records | Local Windows |
| `notification-worker` | Telegram and future notification delivery | Notification outbox, Telegram config | Local Windows |
| `maintenance-worker` | Retention, consistency checks, backups and bounded maintenance | Storage | Local Windows / scheduled |

These are initial logical groups. Split a worker only when resource isolation, independent scaling, fault containment or deployment requirements demonstrate a need.

## 5.3 Three independent state dimensions

Every `ServiceInstance` must report:

```text
DesiredState:
  RUNNING | STOPPED

ObservedState:
  STARTING | RUNNING | STOPPING | STOPPED | FAILED | UNKNOWN

Readiness:
  READY | DEGRADED | NOT_READY | UNKNOWN
```

Definitions:

- `DesiredState` is the target requested by the user/policy.
- `ObservedState` is the process state actually reported by the Agent.
- `Readiness` is whether the worker can perform its declared function correctly.

A running PID is not enough for `READY`.

### Readiness examples

- Market Worker: provider connected, subscriptions active, valid recent market event received, schema validation passed and output freshness within configured bounds.
- Research Worker: dependencies healthy, bounded scan completed, valid research output published and no required input is stale.
- News Worker: at least one enabled source can be polled, recent successful collection exists and dedupe/storage are functioning.
- AI Worker: configured provider passes a bounded test, model is enabled, request budget remains and a test response validates against the expected schema.
- Notification Worker: Telegram credentials and destination are configured, a test delivery succeeds, and outbox processing is functioning.
- Maintenance Worker: storage available, task lease valid and last maintenance result recorded.

## 5.4 Agent lifecycle

Agent must support:

- Register, authenticate, heartbeat and report capabilities.
- Fetch/claim commands addressed to its `agentId`.
- Start, stop and restart allowlisted workers.
- Probe process liveness and application readiness separately.
- Collect bounded log tails, exit codes and restart counters.
- Enforce dependency ordering.
- Apply per-worker startup and shutdown timeouts.
- Apply exponential backoff with jitter and maximum restart attempts.
- Avoid restart storms and mark configuration errors as non-retryable until changed.
- Graceful shutdown on OS shutdown, installer update or explicit stop.
- Reconcile actual process state against desired state after restart.
- Persist minimal local state/checkpoints safely.
- Report Agent version, host OS/architecture, available capabilities and resource metrics.

## 5.5 Safe process control

- The Agent may execute only a static registry of known worker binaries/actions.
- Never accept a shell string, PowerShell command, arbitrary executable path or arbitrary command-line arguments from a Web/desktop request.
- Worker arguments must be generated from validated typed configuration.
- Apply allow-listed environment variables and never pass all parent secrets to child processes.
- Store PID/process identity and executable version for reconciliation.
- Use OS process groups/job objects or equivalent so shutdown does not orphan descendants.
- Restrict worker data directories and permissions.

## 5.6 Durable operations

Every operation must have:

```text
operationId
idempotencyKey
requestedBy
requestedAt
agentId
serviceInstanceId
action
status
attempt
leaseOwner
leaseExpiresAt
startedAt
finishedAt
resultSummary
redactedError
correlationId
```

Operation states:

```text
ACCEPTED | QUEUED | CLAIMED | RUNNING | SUCCEEDED | FAILED | CANCELLED | EXPIRED
```

Flow:

1. Client submits typed operation request.
2. API validates identity, capability, target Agent, action and idempotency key.
3. API writes operation and durable command/outbox record.
4. API returns `202 Accepted` plus `operationId`.
5. Agent claims command through outbound HTTPS polling initially.
6. Agent performs an allowlisted action and reports progress/result.
7. API stores final state and audit record.
8. Desktop/Web follows the operation by ID and observes actual service state.

Use atomic database claims, expiring leases and fencing tokens to prevent two Agents from executing the same leased command. The command must have an expiry so stale actions are not executed much later.

## 5.7 Queue evolution

**Phase 1:** MongoDB-backed queue/outbox. Use atomic find-and-update claims, lease expiry, unique idempotency keys, bounded retries and durable status transitions. This minimizes infrastructure and reuses the current database.

**Later:** NATS JetStream only if multi-host throughput, event replay, fan-out or queue isolation becomes a measured problem. Keep a `Queue`/`EventBus` abstraction so the implementation can change without rewriting domain services.

Do not add Redis merely as a status cache in the first milestone.

---

# 6. Desktop UI and future Web Dashboard

## 6.1 Main UI layout

The default workspace is one central Dashboard, not a large navigation tree. Use one shared Control Center and reusable Modal/Slideover patterns.

### Main Dashboard panels

1. **Market Overview:** market regime, broad market context, selected public reference assets, activity and source freshness.
2. **Signal Center:** research events with asset, event type, timeframe, context, deterministic quality, detected time, source and freshness.
3. **Watchlist:** observed asset data, latest research event and data freshness.
4. **Market Pulse:** public market movers, unusual volume, volatility, supported public funding/open-interest observations and source quality labels.
5. **AI Briefing:** evidence-bound summary with model/provider, generated time, input snapshot, references and stale/missing input disclosures.
6. **News & Events:** headline, publisher, published/ingested timestamps, topics, dedupe cluster and source link.
7. **System Health:** compact status of Agents, services, providers, database and notification delivery.

Each panel must independently handle loading, empty, partial, stale and error states. One failing panel must not blank the whole Dashboard.

## 6.2 Control Center

One Control Center with these sections:

- **Services & Agents:** Agent list, worker state, readiness, heartbeat, uptime, CPU/RAM, logs, restart count and operation history.
- **Market Providers:** provider catalog, enabled sources, capabilities, endpoint/region, priority/fallback, rate-limit metadata, latest success, latency and health test.
- **News Providers:** RSS/Atom/API sources, enablement, fetch interval, source reliability, deduplication and last successful ingestion.
- **AI Providers & Models:** provider instances, models, model capabilities, default model by task, fallback chain, timeout, concurrency, token/output budget and bounded test.
- **Telegram & Notifications:** bot configuration, destinations, notification categories, test message, quiet hours, dedupe and delivery history.
- **Storage & Retention:** collection/storage usage, retention windows, indexes, backup status and maintenance history.
- **Application Settings:** safe app preferences, locale, theme, update channel and connection profiles.
- **Diagnostics:** build version, Git revision, API/DB/Agent status, correlation IDs, redacted logs and support bundle export.

## 6.3 Overlay behavior

- Modal: short confirmations, small forms, provider/model selection and simple rule changes.
- Slideover: service detail, provider configuration, AI model settings, news source detail, signal detail, logs and long forms.
- Only one primary overlay at a time.
- Prevent accidental loss of unsaved forms.
- Preserve form contents after recoverable errors.
- Include keyboard, focus, accessible labels and mobile responsive behavior.
- Detail overlays must show source and freshness metadata.

## 6.4 UI architecture

- Dioxus components should be presentational and use typed state/actions.
- Put data-fetching and command behavior in application services/hooks, not deeply inside visual components.
- Shared UI code must not depend on OS-only APIs.
- Isolate desktop-only features (tray, autostart, native notifications, filesystem integration) behind platform adapters.
- Keep Desktop and Web route/navigation shells separate if necessary, but share domain views, API client, contracts, formatting and most components.
- Do not expose provider tokens to Dioxus WebAssembly or browser storage.

---

# 7. API and contracts

## 7.1 API-first rule

The Desktop App must consume the same versioned API contract planned for Web. Even in local mode, it must call the local Axum API rather than bypassing it and accessing MongoDB or worker internals.

Use `/api/v1` for public application endpoints. Generate OpenAPI from typed handlers/DTO where practical. Keep API version and event schema version explicit.

## 7.2 Core endpoint groups

Suggested initial API surface (adjust path names consistently; do not duplicate aliases):

```text
GET    /api/v1/health
GET    /api/v1/version

GET    /api/v1/dashboard/summary
GET    /api/v1/market/overview
GET    /api/v1/market/pulse
GET    /api/v1/research/events
GET    /api/v1/research/events/{eventId}
GET    /api/v1/watchlists
POST   /api/v1/watchlists
PATCH  /api/v1/watchlists/{watchlistId}
DELETE /api/v1/watchlists/{watchlistId}
GET    /api/v1/news
GET    /api/v1/news/{newsId}
GET    /api/v1/ai/briefings
GET    /api/v1/ai/briefings/{briefingId}

GET    /api/v1/agents
GET    /api/v1/agents/{agentId}
POST   /api/v1/agents/register
POST   /api/v1/agents/{agentId}/revoke
GET    /api/v1/services
GET    /api/v1/services/{serviceInstanceId}
POST   /api/v1/services/{serviceInstanceId}/operations
GET    /api/v1/operations/{operationId}
GET    /api/v1/operations
GET    /api/v1/logs

GET    /api/v1/providers
POST   /api/v1/providers
GET    /api/v1/providers/{providerInstanceId}
PATCH  /api/v1/providers/{providerInstanceId}
POST   /api/v1/providers/{providerInstanceId}/test
POST   /api/v1/providers/{providerInstanceId}/enable
POST   /api/v1/providers/{providerInstanceId}/disable
GET    /api/v1/ai/models
POST   /api/v1/ai/models/{modelInstanceId}/test
PATCH  /api/v1/ai/models/{modelInstanceId}

GET    /api/v1/notifications/settings
PATCH  /api/v1/notifications/settings
POST   /api/v1/notifications/telegram/test
GET    /api/v1/notifications/deliveries
GET    /api/v1/events/stream
```

Agent-only endpoints must be separately authenticated and must not be available to an ordinary user token:

```text
POST   /api/v1/agent/session
POST   /api/v1/agent/heartbeat
POST   /api/v1/agent/commands/claim
POST   /api/v1/agent/commands/{commandId}/progress
POST   /api/v1/agent/commands/{commandId}/complete
POST   /api/v1/agent/events
POST   /api/v1/agent/services/report
```

Use a dedicated Agent identity/token scope, short-lived token rotation and per-Agent revocation. Do not use a shared static Agent secret across all hosts.

## 7.3 API contract rules

- JSON requests/responses use stable versioned DTOs.
- Use RFC 3339 UTC timestamps ending with `Z`.
- Include `requestId`/`correlationId` in errors and operation responses.
- Use pagination cursor or bounded page sizes; cap all user-controlled limits.
- Use ETags or `updatedAt` where they materially reduce polling.
- Use idempotency keys for operation creation and notification test requests.
- Provide typed error codes; do not make UI parse arbitrary text.
- Never place a token, key or secret in a URL.
- Use consistent `401`, `403`, `404`, `409`, `422`, `429` and `5xx` semantics.
- Enforce CORS allow-list for Web origins; local Desktop mode must not use wildcard CORS with credentialed requests.

## 7.4 Realtime

Initial choice: **SSE for one-way server-to-client events**, because UI needs to observe status/operation updates more than it needs bidirectional streaming.

- Browser/Desktop reconnects with a cursor/Last-Event-ID.
- Server sends sequence IDs and event schema version.
- Support bounded polling fallback.
- Coalesce high-frequency market updates.
- Send service/operation/notification status changes promptly.
- Do not broadcast every raw market tick to every client.
- WSS is available for Agent connectivity if later needed; initial Agent command channel can use HTTPS polling for simpler infrastructure and hosting compatibility.

---

# 8. Provider Management Center

Provider management is a first-class Control Center feature. The architecture must allow new providers to be added without rewriting Dashboard, worker lifecycle or domain models.

## 8.1 Provider categories

1. **Market Data Providers:** public market price, candles, trades, volume, market metadata and other supported public observations.
2. **News Providers:** RSS, Atom and vendor/source APIs.
3. **AI Providers:** LLM APIs and compatible local model gateways.
4. **Notification Providers:** Telegram first; additional adapters can be added later.
5. **Platform Integrations:** TM Hub for identity/config integration and future service integrations.

Provider credentials and provider business logic must remain on the backend/Agent side. The UI edits metadata and requests bounded tests; it does not call providers directly with private secrets.

## 8.2 Provider plugin/adapter design

Use a typed registry and trait-based adapters. Example conceptual interfaces:

```rust
#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn health_check(&self, config: &ProviderConfig) -> Result<ProviderHealth>;
    async fn fetch_snapshot(&self, request: MarketSnapshotRequest)
        -> Result<MarketSnapshot>;
    async fn stream(&self, request: MarketStreamRequest)
        -> Result<MarketEventStream>;
}

#[async_trait]
pub trait NewsProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn health_check(&self, config: &ProviderConfig) -> Result<ProviderHealth>;
    async fn fetch_items(&self, request: NewsFetchRequest)
        -> Result<Vec<RawNewsItem>>;
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn list_models(&self) -> Result<Vec<ModelDescriptor>>;
    async fn health_check(&self, model: &ModelConfig) -> Result<ProviderHealth>;
    async fn generate(&self, request: AiRequest) -> Result<AiResponse>;
}
```

The actual implementation may use native `async fn` in traits when supported by the selected Rust toolchain or `async_trait` where needed. Keep the contract independent of a specific provider SDK.

### Provider adapter rules

- Adapter owns protocol translation only; domain logic stays outside adapters.
- Declare capabilities explicitly (REST, WebSocket, candles, funding, OI, RSS, JSON mode, streaming, tools, etc.).
- Validate response schema and timestamps.
- Capture provider request ID where available.
- Apply per-provider concurrency, rate limit and timeout controls.
- Implement retry only for transient errors and only with bounded backoff.
- Do not silently switch data sources without recording source provenance.
- Normalize error classes: authentication, rate limit, timeout, unavailable, invalid payload, unsupported capability, configuration.
- Never log authorization headers or request bodies containing secrets.

## 8.3 Initial market provider support

Implement only public/read-only interfaces in the first release.

| Adapter | Initial capability | Notes |
|---|---|---|
| Binance Public Market Data | Public REST snapshots and public WebSocket streams | No private account/order API, no trading credentials |
| OKX Public Market Data | Public market observations where supported | No private account/order API, no trading credentials |
| Generic REST/JSON source | Configurable read-only endpoint when schema mapping is declared | Restrict host allow-list or require explicit admin-controlled provider definition; protect against SSRF |
| Future sources | Added through `MarketDataProvider` adapters | Do not hardcode provider selection into UI |

Coin metadata/aggregators may be added after reviewing current API terms, limits and data provenance. Do not assume a free API plan is permanent.

## 8.4 Initial news provider support

- RSS 2.0 and Atom feed adapter first.
- Add source-specific APIs only when there is a clear need and terms/limits are reviewed.
- Maintain source catalog: source ID, display name, feed/API URL, language, region, topics, enabled flag, license/usage note, fetch interval and reliability state.
- Keep `publishedAt` and `ingestedAt` distinct.
- Deduplicate using canonical URL, normalized title, publisher and time window; do not merge unrelated stories solely by similar title.
- Preserve original URL and publisher attribution.

## 8.5 Provider instance configuration

Separate provider definition (adapter code/catalog) from provider instance (user configuration).

Suggested fields:

```text
ProviderInstance
- id
- providerType                 # market | news | ai | notification
- providerDefinitionId
- displayName
- enabled
- environment                 # local | cloud | shared
- baseUrl                     # validated; no credentials in URL
- capabilities
- priority
- fallbackGroup
- timeoutMs
- maxConcurrency
- rateLimitPolicy
- secretRef                   # pointer to secret store, never the secret value
- healthState
- lastHealthCheckAt
- lastSuccessAt
- lastErrorCode               # sanitized
- createdAt / updatedAt
```

Provider test operation must be bounded, use a small harmless request, have a timeout, record result metadata and never expose the token in UI/logs.

## 8.6 AI Provider and Model Registry

Support provider instances and individual model entries separately.

Initial compatibility targets:

- Generic OpenAI-compatible Chat Completions endpoint (first implementation priority).
- OpenRouter via OpenAI-compatible adapter and provider-specific headers when configured.
- OpenAI dedicated adapter if required by newer API capabilities.
- Anthropic dedicated messages adapter.
- Google Gemini dedicated adapter.
- Local OpenAI-compatible endpoints such as Ollama, LM Studio and vLLM when their configured API surface matches the adapter.
- Custom provider endpoint with an explicit protocol selection; never guess the protocol from URL alone.

The generic OpenAI-compatible adapter should support custom base URL, model ID, authorization configuration, streaming if supported, JSON response constraints where supported, timeouts and request budgets. Do not assume all providers support tools, JSON mode, image input or identical token semantics.

### Model descriptor

```text
ModelDescriptor
- providerInstanceId
- modelId
- displayName
- protocol
- contextWindow                 # optional; provider-reported or admin-entered
- maxOutputTokens               # optional
- supportsStreaming
- supportsJsonMode
- supportsTools
- supportsVision
- inputModalities
- outputModalities
- enabled
- testedAt
- healthState
```

### Task-based model routing

Allow selecting a default and fallback chain per task:

- `news_classification`
- `news_summary`
- `market_briefing`
- `research_explanation`
- `daily_digest`

Routing policy includes priority, timeout, maximum attempts, concurrency, output limit and usage budget. Only retry a request on a fallback provider when the failure is classified as transient or the configured policy permits it. Do not fallback on invalid input or permission errors.

AI content must:

- Cite the input news/research IDs or source URLs available to the model.
- Separate reported facts from model interpretation.
- State missing or stale evidence.
- Store provider/model ID, prompt version, input snapshot IDs and generated timestamp.
- Never fabricate market values or sources.
- Never mutate deterministic research events or use AI output to trigger execution actions.

## 8.7 Provider health and observability

Show:

- Enabled/disabled and configured state.
- Last health check and result.
- Last successful request.
- Latency and timeout rate.
- Rate-limit state and reset hints where available.
- Fallback provider selected (with reason).
- Credential-present indicator only; never show secret contents.
- Per-task AI model selection and budget status.

Health checks must be explicit/bounded and not create an uncontrolled billable request loop.

---

# 9. Telegram Bot and notification system

Telegram notification support is required for the first functional product, not a later optional add-on.

## 9.1 Architecture

```text
Market / Research / News / AI / System Events
                     |
                     v
             Notification Rules
                     |
                     v
              Durable Outbox
                     |
                     v
          Notification Worker
                     |
                     v
              Telegram Bot API
                     |
                     v
             Configured Chat(s)
```

All send operations must originate from the Notification Worker. UI/API may enqueue a test notification but must not call Telegram using the bot token in the frontend.

## 9.2 Telegram capabilities

- Configure bot token through a secure secret reference.
- Configure one or more destination chat IDs or channel usernames.
- Send a test message.
- Enable/disable Telegram delivery.
- Select notification categories.
- Set quiet hours and optional daily digest schedule.
- Preview a message before enabling a new template.
- View delivery history, Telegram message ID when returned, last error and retry state.
- Mask all token values.
- Allow token rotation without rebuilding the app.

Initial notification categories:

- `service_state_changed`
- `agent_offline`
- `provider_degraded`
- `market_research_event`
- `news_event_digest`
- `ai_briefing_ready`
- `storage_or_backup_warning`
- `system_error_summary`

Every category must be independently configurable. Default notification policy must be conservative to prevent spam.

## 9.3 Delivery reliability

Use a durable outbox with:

```text
notificationId
idempotencyKey
category
severity
recipientRef
payloadTemplateId
payloadData
createdAt
notBefore
attemptCount
status
leaseOwner
leaseExpiresAt
sentAt
telegramMessageId
lastErrorCode
lastErrorAt
```

Rules:

- Deduplicate by stable idempotency key.
- Claim items atomically with a lease.
- Retry network timeouts, HTTP 429 and transient Telegram 5xx errors using bounded exponential backoff and `retry_after` when returned.
- Do not endlessly retry invalid bot token, invalid chat ID, blocked bot or malformed message errors.
- Move exhausted items to a dead-letter state visible in Control Center.
- Escape message content correctly for the selected Telegram parse mode.
- Split long messages safely within Telegram message limits.
- Add a conservative internal per-chat/per-bot rate limit below Telegram's published limits.
- Store Telegram API response metadata, not bot credentials.
- Never put bot tokens in log fields, exception text or URLs.

Use Telegram Bot API `sendMessage` initially. Receiving Telegram commands/webhooks is not required for v1. Do not allow Telegram messages to execute shell commands or perform unrestricted system administration.

## 9.4 Telegram configuration UX

Control Center → Telegram & Notifications:

- Connection status.
- Bot token: masked input; save to secure store.
- Destination chat ID / channel username.
- Test connection / send test message.
- Notification category toggles.
- Quiet hours and digest options.
- Retry queue and dead-letter count.
- Delivery history.

The test action must return an operation ID and show the actual Telegram delivery result.

---

# 10. Identity, authorization and secret management

## 10.1 TM Hub integration

TM Hub remains authoritative for:

- Application identity and app registration.
- User authentication/session.
- RBAC and permission definitions.
- Shared route/feature permission catalog if used by the client.
- Shared application configuration where already owned by Hub.

TM Trading owns its own domain data, Agent registry, service instances, operations, provider instance metadata, AI model selections, notification preferences and delivery records.

Do not copy Hub users/roles into a second authoritative store. If the local Agent requires machine identity, that identity is not a human user and must use a dedicated Agent registration/token scope.

## 10.2 Authorization capabilities

Define application capabilities for API enforcement, for example:

```text
trading.dashboard.read
trading.market.read
trading.research.read
trading.watchlists.manage
trading.providers.read
trading.providers.manage
trading.ai_models.manage
trading.services.read
trading.services.control
trading.agents.manage
trading.notifications.manage
trading.diagnostics.read
trading.settings.manage
```

The exact mapping to TM Hub roles/permissions must be documented and seeded through the existing Hub mechanism when available. The API must enforce capabilities regardless of UI visibility.

## 10.3 Local secret storage

- Use Windows Credential Manager through the Rust `keyring` ecosystem for local bot tokens and provider API keys.
- Store only `secretRef`, credential metadata and masked-presence flags in MongoDB/config files.
- Never commit `.env` files containing real secrets.
- Never store secrets in localStorage, Dioxus persisted client state, URL query parameters or exported diagnostic bundles.
- Apply least-privilege scopes; for market data use public endpoints and no private exchange credential.
- Provide explicit remove/rotate behavior.

## 10.4 Cloud secret storage

Cloud workers/API must read secrets from the deployment platform's secret manager or protected environment injection. The code uses the same `SecretStore` abstraction; it must not require code changes between Windows Credential Manager and cloud secret injection.

## 10.5 Security controls

- Bind local API to `127.0.0.1`, not `0.0.0.0`, in local Desktop mode.
- Require a per-install local session/bootstrap token; do not assume that localhost is automatically trusted.
- Use TLS for remote API and Agent connections.
- Validate TLS certificates; never support an insecure production mode.
- Enforce CORS allow-list on cloud API.
- Apply rate limits and bounded request bodies.
- Validate all provider base URLs and prevent SSRF (private/link-local/metadata IP restrictions, redirect validation and host allow-lists for generic connectors).
- Redact Authorization, API-key and bot-token headers from tracing.
- Apply owner/tenant filters to every query and mutation.
- Agent command allow-list only; no arbitrary shell execution.
- Audit changes to provider settings, secret references, Agent registration, service operations and notification settings.
- Add dependency auditing and secret scanning to CI.

---

# 11. Data model, evidence and storage

## 11.1 Preserve MongoDB initially

The existing `tm-trading` project already uses MongoDB and has existing market, research, news, AI and notification structures. The new Rust application must inspect actual models/indexes/data before designing migrations.

Migration policy:

1. Inventory collections and indexes.
2. Map existing fields to Rust domain structs and BSON serialization.
3. Build compatibility tests against representative records.
4. Add indexes only after query patterns are understood.
5. Use expand → migrate → verify → contract.
6. Back up before any schema/data migration.
7. Do not drop or rename a collection in the same deployment that stops writes to it.
8. Never run destructive migration automatically in a default local startup path.

## 11.2 Evidence contract

Every stored research/news/AI record should preserve as applicable:

```text
id
ownerScope
recordType
sourceId
providerId
providerMethod
observedAt
ingestedAt
publishedAt                  # news-specific
asset/symbol/timeframe        # market/research-specific
inputSnapshotId
producerVersion
schemaVersion
qualityState
qualityReasons
sourceUrl                     # news/evidence where available
createdAt / updatedAt
```

Use UTC everywhere in storage. Keep `observedAt` separate from `ingestedAt`; for news preserve `publishedAt` separately. Mark revisions/invalidations explicitly rather than silently rewriting historical evidence.

## 11.3 Freshness and source provenance

Every panel and record must be able to show:

- Source/provider.
- Event/observation time.
- Ingestion time.
- Last successful update.
- Freshness bucket/state.
- Quality state and reason when invalid/degraded.

A stale value must never be presented as a current value without a stale label.

## 11.4 Retention and backups

- Per-collection retention settings with safe defaults.
- Never auto-delete important research evidence without a documented retention rule.
- Backup and restore must be tested on a non-production copy.
- Backup job must report start/end, bytes, result and verification state.
- Keep configuration exports free of secrets.
- Implement retention as a bounded maintenance job, not a UI page blocking request.

---

# 12. Desktop-first and cloud deployment strategy

## 12.1 Deployment target summary

Use the following targets as the initial deployment strategy. Hosting plans change; verify limits at the time of deployment and do not promise always-free production uptime.

| Component | First deployment | Later deployment | Reason |
|---|---|---|---|
| Windows Desktop | Local release installer | Signed installer + update channel | User's primary control surface |
| Local API | Bundled Axum executable on PC | Cloud Axum Management API | Same contract in both modes |
| Local Agent | Windows user-mode background process first | Optional Windows Service | Keeps workers alive when UI closes |
| Local workers | Separate Rust binaries | Linux containers | Independent lifecycle and host migration |
| Web Dashboard | Not required in first milestone | Cloudflare Pages static Dioxus Web build | Free static frontend candidate; decoupled from worker host |
| Cloud Management API | Not needed in first milestone | Oracle VM or Cloud Run for bounded/stateless API | Select based on persistent connection and cost needs |
| Always-on cloud workers | Not required in first milestone | Oracle Always Free VM if allocated; otherwise low-cost VPS | Needs persistent process capacity |
| Database | Existing MongoDB | Existing MongoDB or managed Atlas tier | Avoid data migration during app rewrite |
| Telegram | Local Notification Worker initially | Cloud Notification Worker or local Agent | Same outbox/adapter code |

## 12.2 Windows Desktop packaging

Initial release must target Windows 10/11 x64. Include:

```text
TM-Trading-Rust/
├── tm-trading-desktop.exe
├── tm-trading-api.exe
├── tm-trading-agent.exe
├── workers/
│   ├── market-worker.exe
│   ├── research-worker.exe
│   ├── news-worker.exe
│   ├── ai-worker.exe
│   ├── notification-worker.exe
│   └── maintenance-worker.exe
├── config/
├── logs/
└── data/                      # only local state/cache, not assumed to be full DB
```

Packaging requirements:

- Desktop can be closed/reopened without stopping Agent/workers.
- Agent starts with the current user by default; Windows Service mode is an optional later installation choice.
- Provide install, repair, upgrade, rollback and uninstall paths.
- Preserve user config and logs during upgrade unless the owner explicitly chooses reset.
- Avoid running all services as Administrator.
- Apply a dedicated data directory with appropriate ACLs.
- Installer must check WebView2 availability and provide a clear repair/install path if required by Dioxus Desktop.
- Build release artifacts deterministically and publish checksums.

## 12.3 Recommended Web Dashboard hosting

**Preferred candidate: Cloudflare Pages Free for static frontend assets.**

- Build Dioxus Web to static WASM/HTML/JS/assets.
- Deploy the static output to Cloudflare Pages.
- Configure SPA fallback and cache headers correctly.
- Configure API base URL per environment.
- Do not put credentials in build-time public variables.
- Web App connects to Axum Management API over HTTPS.
- API CORS allow-list contains only the actual dashboard origins.

Cloudflare Pages free limits observed in official documentation on the date of this document include up to 500 builds/month, 1 concurrent build, 20,000 files/site and 25 MiB max individual asset. Verify current limits before deployment. Reference: [Cloudflare Pages limits](https://developers.cloudflare.com/pages/platform/limits/).

Cloudflare Workers can host small edge/API functions or static-asset logic, but must not be treated as a general persistent Rust worker host. The official Free plan currently documents 100,000 requests/day and 10 ms CPU time/request. Reference: [Cloudflare Workers limits](https://developers.cloudflare.com/workers/platform/limits/).

Do not put long-running market streams, continuous scanner loops or permanent Agent processes into Cloudflare Workers.

## 12.4 Recommended cloud hosting for the Management API

Use one of these deployment paths; do not mix several platforms in the first cloud trial.

### Path A — One Linux VM for API + Agent + a small worker set

**Preferred low-cost/possible-free first cloud trial: Oracle Cloud Infrastructure Always Free Ampere A1, if capacity is available in the account's home region.**

- Build Rust Linux ARM64 binaries or multi-architecture OCI images.
- Run Axum Management API, Agent and a limited set of workers on the VM initially.
- Use systemd for Linux service supervision at the OS level; the TM Trading Agent remains the application-level worker manager.
- Use firewall rules to expose only HTTPS ingress (via reverse proxy/load balancer) and SSH restricted to the owner.
- Keep MongoDB outside the VM if using an existing managed database.
- Monitor CPU, RAM, storage, bandwidth and service health.

Oracle's official Always Free documentation describes an Ampere A1 allowance equivalent to up to 2 OCPUs and 12 GB memory, subject to the account's home region and availability. `Out of host capacity` can prevent provisioning. It is not an uptime guarantee. References: [Oracle Free Tier](https://www.oracle.com/cloud/free/) and [Always Free resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm).

Use this as a best-effort hobby/development host, not the only production dependency. Keep backup/export and a migration path to a low-cost VPS.

### Path B — Cloud Run for bounded/stateless API and scheduled work

Google Cloud Run is an alternative for the Management API if it can operate as a request-driven service and the Agent uses outbound HTTPS polling. It is also suitable for bounded jobs that start, complete and exit.

- Do not assume a request-based service can run an always-on market worker at no cost.
- Instance-based billing charges for the instance lifecycle, including idle periods.
- Long-lived connections and minimum instances can change the cost model.
- Apply billing alerts and usage budgets before enabling deployments.
- Keep continuous WebSocket market ingestion on a VM/container host designed for persistent processes.

References: [Cloud Run billing settings](https://docs.cloud.google.com/run/docs/configuring/billing-settings) and [Cloud Run pricing/free tier](https://cloud.google.com/run/pricing).

### Path C — Low-cost Linux VPS fallback

If Oracle free capacity is unavailable or the free tier has reliability/resource constraints, use one modest Linux VPS. This is the predictable fallback for persistent workers. Build a cost estimate before provisioning; do not assume free hosting will provide 24/7 service guarantees.

## 12.5 Hosts not suitable for persistent workers on their free plan

- **Render Free Web Services:** official docs say free services spin down after 15 minutes without inbound traffic, have ephemeral filesystems and can restart; other service types such as Background Workers are not free instances. Use for preview/demo API only, not continuous workers. Reference: [Render Free](https://render.com/docs/free).
- **Cloudflare Workers Free:** request/CPU-limited edge runtime; use for lightweight edge operations, not permanent worker processes. Reference: [Cloudflare Workers limits](https://developers.cloudflare.com/workers/platform/limits/).
- **Cloud Run request-based mode:** CPU is allocated for request processing; always-on/instance-based use has a different billing model. Use only for workloads that fit that model. Reference: [Cloud Run billing](https://docs.cloud.google.com/run/docs/configuring/billing-settings).

A host that sleeps, scales to zero or has ephemeral storage must never be assigned as the only location for a continuous market data stream or persistent queue processor.

## 12.6 Database hosting

- Keep current MongoDB first; do not move database as part of the Rust port unless a verified technical reason requires it.
- MongoDB Atlas Free may be used for development/testing if its current region, storage, backup, performance and networking limits fit. It is not the default for high-volume/critical continuous production data.
- Store connection strings as secrets; restrict network access and database roles.
- If Atlas Free limits are too small, use the existing database or a paid managed tier only after evaluating cost and backup needs.
- Never expose MongoDB directly to Desktop/Web clients.

Official reference: [MongoDB Atlas Free cluster limitations](https://www.mongodb.com/docs/atlas/reference/free-shared-limitations/).

## 12.7 Deployment recommendation by stage

| Stage | Web | API | Agent/workers | DB |
|---|---|---|---|---|
| Desktop MVP | None | Local Axum | Local Windows Agent + workers | Existing MongoDB |
| First remote test | Cloudflare Pages (optional static status/demo) | One Oracle VM Axum or Cloud Run request-based API | Oracle VM Agent or existing local PC Agent | Existing MongoDB / test Atlas |
| Hybrid personal setup | Cloudflare Pages | Remote Axum Management API | Local Windows Agent + Oracle/VPS Agent | Shared MongoDB with restricted network access |
| Wider use | Cloudflare Pages/CDN | Dedicated Axum API host/VM/container | Separate Linux worker hosts by resource need | Managed MongoDB with backup/restore and monitoring |

**Best initial long-term topology:**

- Cloudflare Pages: static Web Dashboard.
- One Linux VM/VPS: Axum Management API + Agent gateway + selected continuous Rust workers.
- Windows PC: Desktop + optional local Agent/workers.
- Existing MongoDB: shared domain database with proper network rules.
- Telegram Bot API: outbound notifications from Notification Worker.

Move services independently. Do not require all workers to live on one host forever.

---

# 13. Local and cloud Agent networking

## 13.1 Outbound-only connection

A local PC may be behind NAT/router/firewall. The Agent must initiate outbound HTTPS requests to the Management API; do not require inbound port forwarding on the user's PC.

Initial protocol:

- Agent registers with a one-time pairing code or approved registration flow.
- API issues a per-Agent credential with limited scope.
- Agent polls for commands with a bounded interval and long-poll where supported.
- Agent submits heartbeat and command progress/results.
- Credentials can be rotated/revoked by `agentId`.
- Backoff and jitter prevent reconnect storms.

Later, a WSS connection may reduce polling overhead, but it is an optimization, not a prerequisite.

## 13.2 Agent trust model

- An Agent is a machine identity, not a human user.
- Agent token can perform only Agent endpoints and only actions allowed by its registered capabilities.
- User commands are authorized by Management API before being placed in the durable queue.
- Agent revalidates command signature/target/expiry/action allow-list before execution.
- Revoking an Agent immediately prevents new command claims.
- A lost/stale Agent must appear `OFFLINE`/`UNKNOWN`; do not pretend its services are stopped if the host cannot be reached.

## 13.3 Local mode

- Local Management API binds only to loopback.
- Generate a per-install local access credential and protect it using Windows Credential Manager.
- Local Agent communicates with local API; do not make Desktop the process supervisor.
- Optional local Named Pipes can be used later for a narrowly scoped privileged installer/service bridge, but the main API remains HTTP so the same `api-client` can be used by Web and Desktop.

---

# 14. AI, news and research pipeline

## 14.1 Data-to-insight pipeline

```text
Public Provider Adapters
        |
        v
Raw Observation Validation
        |
        v
Normalized Market/News Records
        |
        v
Deterministic Research / Classification
        |
        v
Evidence Snapshot + Provenance
        |
        +-------> Dashboard Read Models
        |
        +-------> AI Worker (summary/interpretation only)
        |                   |
        |                   v
        |            AI Briefing Records
        |
        +-------> Notification Rules -> Outbox -> Telegram
```

## 14.2 Research event requirements

- Stable unique event ID.
- Asset/symbol/timeframe where relevant.
- Event category and descriptive context.
- Detection/observation timestamp.
- Ingestion timestamp.
- Source/provider/method.
- Evidence values and input snapshot ID.
- Data quality and freshness state.
- Producer and schema version.
- Invalidation/revision metadata where relevant.

Research logic must be deterministic where expected. Port existing engine calculations using golden fixtures and prove parity before replacing the source implementation.

## 14.3 AI evidence discipline

- AI is a summarization/research layer, not the source of truth.
- Store model/provider, prompt version, input snapshot IDs and generatedAt.
- Distinguish facts, interpretations and uncertainty.
- Provide links/IDs to underlying sources.
- State stale/missing input explicitly.
- Cap request timeout, concurrency, tokens/output and retries.
- Use model routing per task and a bounded fallback chain.
- Never allow AI to alter deterministic signal values or initiate execution actions.

---

# 15. Configuration and environment

## 15.1 Configuration layers

Use this precedence (highest priority first):

1. Process environment / deployment secret injection.
2. OS secure secret store referenced by `secretRef`.
3. User-managed application configuration file for non-secret preferences.
4. Remote application config from TM Hub where appropriate.
5. Built-in safe defaults.

Do not allow an insecure default to override a missing production secret. Production must fail closed for required credentials.

## 15.2 Example non-secret configuration

This is a shape example only; values must be validated and secret fields must be represented by references, not actual values.

```toml
[app]
name = "tm-trading-rust"
environment = "local-dev"
locale = "vi-VN"
timezone = "Asia/Ho_Chi_Minh"

[api]
local_bind = "127.0.0.1:4101"
api_prefix = "/api/v1"
request_timeout_seconds = 20

[database]
provider = "mongodb"
database_name = "tm_trading"
connection_secret_ref = "secret://database/tm-trading"

[agent]
mode = "local"
heartbeat_interval_seconds = 15
command_poll_interval_seconds = 10

[services.market-worker]
enabled = true
restart_policy = "on-failure"
max_restart_attempts = 5

[services.research-worker]
enabled = true
restart_policy = "on-failure"
max_restart_attempts = 5

[notifications.telegram]
enabled = false
bot_token_secret_ref = "secret://telegram/bot-token"
default_chat_secret_ref = "secret://telegram/default-chat"
```

## 15.3 Required config validation

- Validate URLs and protocols.
- Validate bind address by deployment mode.
- Reject missing production secret references.
- Reject unknown provider type/protocol.
- Validate timeouts, rate limits and retry bounds.
- Validate Agent identity and host capabilities.
- Warn if a provider is enabled but not configured or has no successful health check.
- Do not echo raw config values when validation fails; redact secret fields.

---

# 16. Development, build and CI standards

## 16.1 Required local commands

The repository must provide documented workspace-level equivalents of:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Also provide targeted package commands for Desktop, API, Agent and individual workers.

## 16.2 CI pipeline

On pull request:

1. Check formatting.
2. Run Clippy with warnings denied.
3. Run unit and integration tests.
4. Validate OpenAPI generation/diff.
5. Run secret scanning and dependency audit.
6. Build Windows x64 Desktop/API/Agent/workers.
7. Build Linux x86_64 containers for API/Agent/workers.
8. Build Linux ARM64 images when cross-build runners/toolchains are available.
9. Verify Docker images contain non-root runtime user and health checks.
10. Upload build artifacts only for trusted branches/tags.

Release pipeline:

- Version binaries together through a release manifest.
- Produce checksums and signed artifacts when signing identity is available.
- Build Windows installer.
- Build OCI images for API/Agent/workers.
- Publish deployment manifests and migration notes.
- Never deploy production automatically from an unreviewed branch.

## 16.3 Cross-platform targets

Initial:

- `x86_64-pc-windows-msvc` — Windows Desktop/API/Agent/workers.
- `x86_64-unknown-linux-gnu` — Linux server containers.

Later:

- `aarch64-unknown-linux-gnu` — Oracle Ampere ARM64 VM.
- macOS targets only after Windows/Server workflows are stable.

---

# 17. Migration from existing `tm-trading`

The current `tm-trading` repository is a substantial Nuxt/Node/MongoDB project, not a greenfield prototype. Migration is a controlled port, not a blind rewrite.

## 17.1 Audit before implementation

Inspect at least:

- `docs/web-app/tm-trading-central-dashboard-v5.0.md`
- `docs/web-app/v5-baseline-inventory.md`
- `docs/web-app/tm-trading-roadmap-v4.2.md`
- V4.2 service inventory, operations control audit and reliability drills.
- `engine/` and its golden test fixtures.
- `market/`, `realtime/`, `services/`, `ai/`, `notify/`, `shared/`.
- Existing MongoDB models, indexes and schemas.
- `server/plugins/` and Nitro lifecycle hooks.
- Existing TM Hub client/API contract and RBAC route definitions.
- Existing Telegram outbox/sender and notification rules.
- Existing `.env.example`, package manifests and deploy manifests.

Create a source-to-target mapping report before porting.

## 17.2 Migration disposition

| Existing area | Rust target | Default action |
|---|---|---|
| `market/` | `crates/market-providers`, market domain | PORT incrementally; preserve event contracts |
| `engine/` | `crates/research-engine` | PORT with golden fixture parity |
| `services/supervisor.mjs` and runtime | `apps/agent`, `crates/supervisor-core` | REIMPLEMENT lifecycle based on existing behavior and drills |
| `services/*` collectors/scanners | `workers/*` and provider crates | PORT one worker at a time |
| `ai/` | `crates/ai-providers`, `workers/ai-worker` | PORT provider gateway and evidence-bound summaries |
| `notify/` and Telegram sender/outbox | `crates/notification-core`, `crates/telegram-client`, notification worker | PORT with delivery/idempotency parity |
| `server/` Nitro APIs | `apps/management-api` | Recreate public contracts incrementally |
| `app/` Nuxt UI | `apps/desktop`, later `apps/web` | REBUILD as Dioxus; preserve product layout and behavior requirements |
| `shared/` contracts/types | `crates/domain`, `crates/contracts` | MAP and port; avoid duplicate definitions |
| MongoDB collections | Rust repository layer | KEEP schema first; migrate only when justified |
| `pine/` source | Keep original Pine Script | KEEP as external/supporting research artifact where useful |
| `exec/` and order-oriented UI/APIs | None in active V5 product | EXCLUDE/QUARANTINE; do not port execution capability |
| `simulation/` | None by default | Review for read-only historical evidence only; do not port order simulation/execution |
| tests | Rust unit/integration/fixture tests | PORT assertions and golden fixtures; keep old tests until Rust parity established |

## 17.3 Porting order

1. Audit domain contracts and data models.
2. Port shared primitives and golden fixtures.
3. Build Agent/Supervisor and sample worker.
4. Build API and shared API client.
5. Port Market Worker.
6. Port Research Worker and prove deterministic parity.
7. Port News Worker.
8. Port AI provider gateway and AI Worker.
9. Port Telegram/outbox Notification Worker.
10. Build Desktop Dashboard and Control Center against the API.
11. Add cloud Agent deployment profile.
12. Build Web Dashboard after API/contract stability.
13. Deprecate old Nuxt/Node features only after parity and rollback evidence.

## 17.4 Dual-run and cutover rules

- Read-only comparison may run old and Rust processors in parallel against the same captured input fixture.
- Do not let both versions write the same production records simultaneously without separate namespaces/idempotency safeguards.
- Use a source-of-truth flag for each worker during migration.
- Compare output counts, schema, timestamp handling, quality states and deterministic values.
- Cut over one worker at a time.
- Preserve old worker rollback until the Rust worker has passed soak/health tests.
- Do not delete old runtime as part of the same change that first enables Rust writes.

---

# 18. Testing and acceptance criteria

## 18.1 Desktop acceptance

- Desktop launches and renders the Dashboard without a cloud dependency in local mode.
- API/Agent/workers can run while Desktop is closed.
- Reopening Desktop restores actual service status from API/Agent; UI does not invent status.
- Multiple connection profiles can be selected safely.
- Panels support loading, empty, partial failure, stale and retry states.
- One panel error does not crash the whole app.
- Modal/Slideover keyboard, focus, escape, dirty-form and mobile/window-resize behavior are verified.
- Windows packaging installs, upgrades, rolls back and uninstalls without silently deleting user data.

## 18.2 API/Agent acceptance

- Every protected endpoint enforces authentication and capability checks.
- Agent registration is per host and revocable.
- Duplicate command claims are prevented by leases/fencing/idempotency.
- Stale commands expire rather than executing later.
- Worker crashes are detected and handled by bounded policy.
- Process liveness and functional readiness are reported separately.
- Service stop respects shutdown timeout and does not orphan child processes.
- Closing or crashing Desktop does not stop background workers.
- Agent loss is shown as offline/unknown, not falsely as stopped.

## 18.3 Provider acceptance

- Provider catalog and provider instances are separate.
- Add/edit/enable/disable/test flows work.
- Secrets are masked and stored through the secret store abstraction.
- Test calls are bounded and do not create uncontrolled loops.
- Provider rate limits, timeouts and transient errors are handled.
- Fallbacks record which provider served the data and why.
- Unsupported capabilities are shown clearly.
- Generic custom REST endpoints are protected against SSRF.

## 18.4 AI acceptance

- OpenAI-compatible adapter works with a controlled test endpoint.
- Provider/model CRUD and task-based model routing work.
- Health test and model test are bounded.
- AI timeouts, retries, concurrency and output limits work.
- Provider/model/prompt version and input evidence are persisted.
- AI facts and interpretations are distinguishable.
- AI output cannot modify deterministic research records or invoke execution capability.

## 18.5 Telegram acceptance

- Bot token is securely stored, masked and rotatable.
- Destination can be configured and tested.
- Test notification returns a tracked operation/delivery result.
- Notification outbox is durable across worker restart.
- Duplicate events are not repeatedly delivered.
- 429/retry-after and transient errors are handled with bounded retry.
- Invalid token/chat errors are surfaced without leaking secrets.
- Dead-letter notifications are visible and can be retried after configuration is corrected.

## 18.6 Data acceptance

- Existing representative MongoDB records deserialize correctly.
- Rust output matches golden fixtures for deterministic research behavior.
- UTC and `observedAt`/`ingestedAt` semantics are preserved.
- News retains publisher, canonical URL, `publishedAt` and `ingestedAt`.
- Data quality and freshness are explicit.
- Owner/tenant filters are tested.
- Backup restore is tested against a non-production copy.

## 18.7 Deployment acceptance

- Windows local mode works without inbound network exposure.
- Linux x86_64 container starts as non-root and has health checks.
- Linux ARM64 build is verified before Oracle deployment.
- Cloudflare Pages serves the Dioxus Web static build and SPA fallback correctly.
- Web calls the same `/api/v1` contract as Desktop.
- CORS and auth are correctly configured.
- Agent connects outbound and recovers after network interruption.
- Free-host sleep/scale-to-zero does not cause a false READY state.
- Cost alerts and usage limits are configured before any cloud trial that can incur charges.

## 18.8 Test result vocabulary

Every report must use:

- `PASS` — command/test actually completed and passed.
- `FAIL` — command/test actually completed and failed.
- `NOT RUN` — not executed.
- `BLOCKED` — requires unavailable credential/host/permission/dependency.
- `SKIPPED` — intentionally not run, with reason.

---

# 19. Implementation phases and deliverables

## Phase 0 — Repository setup and audit

**Tasks**

- Create `tm-trading-rust` repository/workspace.
- Set up README, AGENTS and this roadmap as authority.
- Audit source repo V5/V4.2 and produce source-to-target inventory.
- Record starting commit, Git state and available tests.
- Identify real MongoDB collections/indexes and TM Hub contracts.

**Deliverables**

- `docs/migration/source-inventory.md`
- `docs/migration/module-mapping.md`
- `docs/migration/data-contract-map.md`
- `docs/migration/tm-hub-integration-map.md`

**Exit gate:** no code porting begins until domain/process/data map is written.

## Phase 1 — Rust workspace and shared contracts

**Tasks**

- Set stable Rust toolchain.
- Create Cargo workspace and CI.
- Add domain, contracts, config, observability crates.
- Define API versioning and error model.
- Add MongoDB connection abstraction and test repository.

**Deliverables**

- Workspace builds.
- CI checks formatting, Clippy and unit tests.
- First OpenAPI document is generated.

## Phase 2 — Supervisor/Agent vertical slice

**Tasks**

- Implement Agent identity and local mode.
- Implement worker registry and process lifecycle.
- Implement desired/observed/readiness state.
- Implement heartbeat, logs and graceful shutdown.
- Implement sample worker and crash drill.
- Persist operations and service snapshots.

**Deliverables**

- Desktop can request start/stop/restart of sample worker via API.
- API returns operation ID.
- Agent reports actual result.
- Sample worker survives Desktop close.

## Phase 3 — Axum Management API and shared API client

**Tasks**

- Implement local API loopback binding.
- Integrate TM Hub authentication/permission mapping.
- Add Agent-only API scopes.
- Implement operation queue and service/read-model endpoints.
- Generate OpenAPI and typed API client.

**Deliverables**

- Local Desktop can connect through `api-client`.
- API permission tests and idempotency tests pass.

## Phase 4 — Dioxus Desktop shell and Control Center

**Tasks**

- Build window, app shell, theme, localization and navigation.
- Build single Dashboard shell and independent panels.
- Build Control Center with Services & Agents first.
- Add provider, AI model and Telegram settings surfaces as APIs become available.
- Add reusable Modal/Slideover system.

**Deliverables**

- Installable Windows development build.
- Service Control Center can operate sample worker.

## Phase 5 — Data and market providers

**Tasks**

- Port market-domain contracts and repository.
- Implement public Binance and OKX data adapters.
- Implement provider catalog/instance configuration and bounded health tests.
- Port Market Worker and Market Overview/Market Pulse panels.
- Port Watchlist read/write data model.

**Deliverables**

- Public market data is collected, timestamped, normalized and sourced.
- Provider failure/stale state is visible.

## Phase 6 — Research engine

**Status: COMPLETED & EXPANDED (2026-11-23)**

**Tasks**

- [x] Port deterministic calculations incrementally (TA indicators, VSA series).
- [x] Port golden fixtures and comparison suite (`indicator_parity`, `ta_parity`, `vsa_parity`).
- [x] Implement Research Worker, event contract and quality state.
- [x] **Phase 10 Extension: Advanced Research Methods ported from legacy `engine/methods`:**
  - `price_action.rs`: Pin bar, Engulfing, Inside bar, Break of Structure (BOS).
  - `trend.rs`: Fast/slow MA pullbacks, Donchian channel breakout, range bounces.
  - `wonderwhales.rs`: Parabolic SAR flip, EMA trend crossover, ATR filter.
  - `sweep.rs`: High/low liquidity sweeps with volume surge confirmation.
  - Multi-method scan support in `ResearchWorkerConfig` (`TM_TRADING_RESEARCH_METHODS`).

**Deliverables**

- [x] Rust result matches approved fixtures with 100% test pass.
- [x] Every research event has provenance, snapshot ID, and freshness.
- [x] Multi-method research worker scanning all methods concurrently.

## Phase 7 — News and AI Provider Center

**Status: COMPLETED (2026-11-23)**

**Tasks**

- [x] RSS/Atom source registry and News Worker.
- [x] News deduplication and event cluster.
- [x] AI Provider adapter interface and generic OpenAI-compatible implementation.
- [x] Dedicated adapters where required (OpenAI, Anthropic, Gemini, OpenRouter headers/protocol differences).
- [x] Model registry, task routing, fallback and bounded model testing.
- [x] AI Worker and evidence-bound briefing UI.

**Deliverables**

- [x] News & Events panel.
- [x] Provider/model configuration and status.
- [x] AI briefing with traceable input snapshot and source references.

## Phase 8 — Telegram and notification center

**Status: COMPLETED (2026-11-23)**

**Tasks**

- [x] Port notification rules/outbox.
- [x] Build Telegram client and Notification Worker.
- [x] Add secure token storage, destination configuration and test message.
- [x] Add retries, dedupe, dead-letter and delivery history.
- [x] **Rule-Based Alert Evaluation & Webhook Receiver:**
  - `alert_rules.rs`: Alert rules for Extreme Funding, Liquidation Spikes, and High-Conviction Research events.
  - Inbound TradingView webhook intake route (`POST /tm-alert`, `/tm-alert/{token}`, `/api/v1/webhooks/tradingview`).
  - Native outbox deduplication via `${notify_id}:${channel}` preventing alert spam.
  - Read-only simulation portfolio metrics (`portfolio.rs`).
  - Binance Futures public adapter for funding rates and liquidation zones (`/api/v1/market/funding`, `/api/v1/market/liquidation`).

**Deliverables**

- [x] End-to-end notification test reaches configured Telegram chat.
- [x] No secrets appear in logs or UI responses.
- [x] Management API endpoints for notification outbox, summary, delivery logs, test ping, and TradingView webhooks.
- [x] Desktop Notification Center panel with delivery metrics and manual ping.

## Phase 9 — Local Desktop release hardening

**Status: COMPLETED (2026-11-23)**

**Tasks**

- [x] Installer, autostart option and update/rollback.
- [x] WebView2 checks and pre-flight verification on Windows hosts.
- [x] Windows Agent process/service mode decision.
- [x] Backup/restore with SHA-256 cryptographic checksums and support bundle.
- [x] Resource limits, health and recovery drills.
- [x] Universal multi-platform build scripts for Windows, Linux, and macOS.

**Deliverables**

- [x] Desktop release candidate with stable local services (`dist/tm-trading-*-v0.1.0.*`).
- [x] System Diagnostics and secret-redacted Support Bundle generation (`/api/v1/system/support-bundle`).
- [x] Cryptographic backup/restore endpoints (`/api/v1/system/backup/export`, `/verify`, `/restore`).
- [x] Universal packaging scripts: `scripts/package.ps1` (Windows/PowerShell) and `scripts/package.sh` (Linux/macOS).
- [x] Deployment guides: `docs/deployment/desktop-windows.md` and `docs/deployment/cross-platform-build.md`.

## Phase 10 — Cloud Agent and remote Management API

**Tasks**

- Add Agent pairing/registration and revocation.
- Add outbound HTTPS polling and heartbeat.
- Add multi-Agent service listing and remote operation.
- Build Linux x86_64 and ARM64 images.
- Prepare Oracle VM or VPS deployment.
- Add remote auth/CORS/TLS and billing guardrails.

**Deliverables**

- Desktop can manage a remote Agent without exposing the Agent host inbound.
- Cloud operations and logs are audited.

## Phase 11 — Web Dashboard

**Tasks**

- Build Dioxus Web using shared components/contracts/api-client.
- Reuse the Dashboard and Control Center information architecture.
- Add secure login through TM Hub.
- Configure Cloudflare Pages static deployment.
- Test browser routing, CORS, authorization and realtime/polling.

**Deliverables**

- Web and Desktop can view the same Agents, service instances and operations.
- Both clients use the same API and RBAC.

## Phase 12 — Legacy decommission and optimization

**Tasks**

- Remove Node/Nuxt runtime only when all required capabilities are ported and verified.
- Preserve legacy data and migration rollback.
- Remove unused dependencies and dead code with evidence.
- Reevaluate MongoDB queue vs NATS only from measured use.
- Document production topology and operating costs.

**Deliverables**

- Rust-first release and complete migration report.

---

# 20. Required documentation generated by the coding agent

The agent must maintain these documents as code evolves:

- `README.md` — project purpose, quick start and supported modes.
- `AGENTS.md` — coding-agent rules and links to architecture documents.
- `docs/architecture/system-architecture.md` — current architecture.
- `docs/architecture/adr/` — architecture decision records.
- `docs/migration/source-inventory.md` — old repository inventory.
- `docs/migration/module-mapping.md` — old module to Rust crate/worker map.
- `docs/migration/data-contract-map.md` — MongoDB schema and event mapping.
- `docs/api/openapi.yaml` — generated API contract snapshot.
- `docs/providers/provider-catalog.md` — adapters and capabilities.
- `docs/operations/service-catalog.md` — worker definitions and readiness probes.
- `docs/operations/runbooks.md` — start/stop/restart/backup/recovery procedures.
- `docs/deployment/desktop-windows.md` — local install and upgrade.
- `docs/deployment/cloudflare-pages.md` — Web deployment.
- `docs/deployment/linux-agent.md` — cloud Agent/worker deployment.
- `docs/deployment/cost-and-free-tier-review.md` — verified host limits and cost warnings.
- `docs/telegram/telegram-setup.md` — bot setup, secure token handling and test flow.
- `docs/ai/providers-and-models.md` — provider/model support and routing.
- `docs/migration/cutover-report.md` — parity, tests, rollback and legacy decommission.

Any hosting-plan detail that changes over time must include a verification date and official source link.

---

# 21. AI-agent task protocol

For each phase, the coding agent must follow this cycle:

1. **Inspect:** read relevant code, docs, tests and data structures.
2. **Map:** identify dependencies and behavior that must remain compatible.
3. **Plan:** write a short phase-specific implementation checklist in the relevant docs/PR description; do not ask the owner routine design questions.
4. **Implement:** change one vertical slice at a time.
5. **Verify:** run formatter, Clippy, targeted tests and workspace tests as feasible.
6. **Compare:** for a port, compare old vs Rust output using fixtures or captured inputs.
7. **Document:** update module map, API docs, provider catalog and runbooks.
8. **Report:** list changed files, commands/results, limitations, migration risks and rollback.
9. **Commit:** use one coherent conventional commit per logical slice.

### 21.1 When the agent must stop rather than guess

- Required cloud credentials or account permissions are missing.
- A cloud action could incur charges beyond configured budget.
- A production data migration or deletion is destructive.
- Existing data schema meaning is ambiguous and cannot be established from source/tests.
- TM Hub auth contract cannot be verified.
- A provider endpoint's terms/permissions cannot be verified.
- A security control would otherwise be weakened.

In these cases, continue with safe local code, mock adapters and documentation. Mark the external step `BLOCKED`, list the exact environment variable/permission needed, and do not fabricate values.

### 21.2 Do not do these things

- Do not start a new Nuxt/Vue/Node application inside `tm-trading-rust`.
- Do not put long-running workers inside Dioxus UI or Axum request handlers.
- Do not create a second service supervisor.
- Do not connect a WebAssembly client directly to MongoDB or provider APIs requiring secrets.
- Do not store credentials in plain-text config/DB.
- Do not call Telegram from UI/browser code.
- Do not create a background worker on a platform that sleeps/scales-to-zero and label it always-on.
- Do not migrate private exchange credentials or order execution functionality.
- Do not remove source project code until the Rust replacement and rollback are verified.
- Do not deploy publicly with wildcard CORS, default admin credentials or weak JWT secrets.

---

# 22. Definition of Done

The project is not considered complete merely because Cargo builds successfully.

## Desktop-first completion

- [ ] Rust workspace, Windows x64 build and installer work.
- [ ] Dioxus Desktop Dashboard is functional.
- [ ] Axum local API uses versioned shared contracts.
- [ ] Desktop and API are separate processes.
- [ ] Agent/Supervisor is separate from Desktop.
- [ ] Worker lifecycle, readiness, heartbeat, crash recovery and graceful shutdown work.
- [ ] Service Control Center displays actual state and operation history.
- [ ] MongoDB current data can be read without destructive migration.
- [ ] Public market providers can be configured/tested safely.
- [ ] News ingestion and source provenance work.
- [ ] AI provider/model configuration and bounded health tests work.
- [ ] AI summaries preserve source/input provenance.
- [ ] Telegram bot can send a test notification and configured alerts.
- [ ] Telegram outbox, deduplication, retry and delivery history work.
- [ ] TM Hub auth and capability checks work.
- [ ] Secrets are protected and redacted.
- [ ] Closing Desktop does not stop services.
- [ ] Tests, drills, logs and known limitations are documented.

## Cloud/Web-ready completion

- [ ] API contract is independent of local filesystem/process internals.
- [ ] Local and remote Agents use the same protocol.
- [ ] Agent uses outbound authenticated connection and per-host identity.
- [ ] Multi-Agent listing and remote service management work.
- [ ] Linux x86_64 and ARM64 builds are verified.
- [ ] API and workers can deploy independently.
- [ ] Dioxus Web uses the same contracts and API client.
- [ ] Cloudflare Pages deployment works with secure API CORS.
- [ ] Free-tier constraints and fallback host are documented.
- [ ] Cloud cost alerts and secret handling are configured.

---

# 23. Official technology and hosting references

These links were checked for this document on **2026-11-17**. Hosting quotas, crate versions, product plans and provider APIs can change; verify again before actual deployment.

## Rust and UI

- [Dioxus documentation 0.7](https://dioxuslabs.com/learn/0.7/)
- [Dioxus Desktop platform guide](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop/)
- [Dioxus releases](https://github.com/DioxusLabs/dioxus/releases)
- [Axum documentation](https://docs.rs/axum/latest/axum/)
- [Tokio documentation](https://docs.rs/tokio/latest/tokio/)
- [Utoipa documentation](https://docs.rs/utoipa/latest/utoipa/)
- [MongoDB Rust Driver](https://www.mongodb.com/docs/drivers/rust/current/)
- [MongoDB Rust Driver runtime](https://www.mongodb.com/docs/drivers/rust/current/runtimes/)
- [Tokio Tungstenite](https://docs.rs/tokio-tungstenite/latest/tokio_tungstenite/)
- [Tracing](https://docs.rs/tracing/latest/tracing/)
- [Rust Keyring](https://docs.rs/keyring/latest/keyring/)

## Cloud hosting

- [Cloudflare Pages limits](https://developers.cloudflare.com/pages/platform/limits/)
- [Cloudflare Workers limits](https://developers.cloudflare.com/workers/platform/limits/)
- [Cloudflare Workers Rust](https://developers.cloudflare.com/workers/languages/rust/)
- [Oracle Cloud Free Tier](https://www.oracle.com/cloud/free/)
- [Oracle Always Free resources and capacity notes](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)
- [Google Cloud Run billing settings](https://docs.cloud.google.com/run/docs/configuring/billing-settings)
- [Google Cloud Run pricing](https://cloud.google.com/run/pricing)
- [Render Free limitations](https://render.com/docs/free)
- [MongoDB Atlas Free cluster limitations](https://www.mongodb.com/docs/atlas/reference/free-shared-limitations/)

## Notifications

- [Telegram Bot API](https://core.telegram.org/bots/api)

---

# 24. Final architecture decision

`tm-trading-rust` is a **Rust-first, API-first, Desktop-first and cloud-ready** application.

The first deliverable is a Windows Desktop App built with Dioxus, connected to a local Axum API and a Rust Agent/Supervisor. The Agent owns independent Rust workers. MongoDB and existing domain data are preserved during migration. Provider management, AI model management and Telegram notifications are built into the Control Center and use secure server-side adapters.

The future Web Dashboard is a second Dioxus client that calls the same API. The Web frontend can be served statically from Cloudflare Pages, while Management API and continuous workers run on a Linux VM/container host. Local and cloud Agents connect outbound to the same Management API, allowing Desktop and Web to view and manage service instances from multiple hosts.

Start simple: one API, one Agent per host, a durable MongoDB-backed operation/outbox queue, and a small set of worker processes. Add NATS, extra hosts, persistent WebSockets or additional infrastructure only when measured needs justify them.

**The target is not merely to rewrite JavaScript in Rust. The target is to establish one reliable Rust service platform that can be managed from Desktop today and Web tomorrow.**
