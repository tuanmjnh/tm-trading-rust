# Legacy Source Inventory

> Audit date: 2026-11-17`<br>
> Source repository: `D:\Applications\tm-trading`<br>
> Target repository: `D:\Applications\tm-trading-rust`
> Authority: `docs/TM-TRADING-RUST-MASTER-ROADMAP.md`

## 1. Audit method and status

This is a static source audit of the legacy repository. It records the code, documentation, tests, and declared MongoDB contracts that must be considered during the port.

| Check | Result | Evidence |
|---|---|---|
| Source commit identified | PASS | `HEAD 0c0ede7ac09415ba211b308e06ef4b24093762d2` on `main` |
| Source branch and remote state captured | PASS | `main...origin/main` at audit time |
| Source working tree inspected | PASS | Existing uncommitted changes were present and were not modified |
| V5 product authority read | PASS | `docs/web-app/tm-trading-central-dashboard-v5.0.md` |
| V4.2 runtime/operations references read | PASS | Service inventory, operations audit, and reliability drills |
| Domain, market, service, AI, notification, and shared trees inspected | PASS | Paths listed below |
| MongoDB models and declared indexes inspected | PASS | `engine/models/*.mjs` |
| Live MongoDB collections and indexes inspected | BLOCKED | No verified live database inspection was available |
| Legacy test suites executed | NOT RUN | The source working tree is dirty; no source changes were authorized |
| Live TM Hub contract executed | BLOCKED | Deployed app ID, registration state, and signing mode were not verified |

Historical collection counts and prior drill reports are evidence from the legacy repository, not proof of the current live environment.

## 2. Source baseline

### 2.1 Repository state

- Package: `tm-trading@0.1.0`, private ESM project.
- Package manager: `pnpm@12.15.0` through `pnpm-lock.yaml` and `pnpm-workspace.yaml`.
- Runtime baseline: Node.js `>=18`; local audit environment used Node `v24.22.2` and pnpm `12.15.0`.
- UI/API framework: Nuxt `^4.6.2` with Nitro `^2.11.0`.
- Database layer: Mongoose `^9.10.3` over MongoDB.
- Identity client: `@tuanmjnh/tm-hub-client` `1.2.0`.
- Token verification: `jose ^5.9.6` in the satellite and TM Hub.

Existing uncommitted source paths at the audit boundary:

```text
.agents/AGENTS.md
.env.example
AGENTS.md
app/composables/useHub.ts
app/composables/useUsers.ts
app/utils/hub.ts
i18n/locales/vi.json
package.json
pnpm-lock.yaml
pnpm-workspace.yaml
```

These changes belong to the legacy repository and were neither overwritten nor used as evidence of a clean baseline.

### 2.2 Authority documents

| Source path | Purpose | Rust use |
|---|---|---|
| `docs/web-app/tm-trading-central-dashboard-v5.0.md` | Current product boundary and information-first UI model | Product/scope authority; source behavior reference only |
| `docs/web-app/v5-baseline-inventory.md` | Static inventory and cleanup disposition | Migration input |
| `docs/web-app/v5-service-runtime-map.md` | Current process ownership and readiness semantics | Agent design input |
| `docs/web-app/v4.2-service-inventory.md` | Detailed lifecycle, readiness, command, and operation contracts | Supervisor parity input |
| `docs/web-app/v4.2-operations-control-audit.md` | Honest control-plane behavior and authorization | API/operation parity input |
| `docs/web-app/v4.2-drills.md` | Process-level reliability drills | Rust drill acceptance input |
| `docs/web-app/data-ownership-map.md` | Collection ownership classification and backfill state | Data migration input |
| `docs/web-app/system-integration-map.md` | Route, service, model, and integration wiring | API/domain mapping input |
| `docs/web-app/v5-route-migration-map.md` | V5 information architecture | Desktop information architecture input |
| `docs/engine/time-rules.md` | UTC and business-day rules | Time contract input |
| `docs/indicator/vsa-wyckoff/vsa-wyckoff-method.md` | VSA behavior specification | Research-engine golden parity input |
| `docs/indicator/vsa-wyckoff/vsa-optimization.md` | Measurement-honesty decision | Do not introduce unsupported optimization |

## 3. Source module inventory

### 3.1 Runtime and UI

| Source path | Responsibility | Migration disposition |
|---|---|---|
| `app/` | Nuxt dashboard, composables, pages, overlays, administration surfaces | Rebuild in `apps/desktop`; later `apps/web` |
| `server/api/v1/` | Nitro HTTP endpoints | Recreate public contracts in `apps/management-api` |
| `server/middleware/auth.ts` | Offline Hub-token verification and refresh self-heal | Reimplement in `crates/auth` and the API boundary |
| `server/middleware/api-guards.ts` | Feature-driven CORS/rate limiting | Recreate with explicit, fail-closed local policy |
| `server/utils/authz.ts` | Capability checks and legacy aliases | Port compatibility mapping into `crates/auth` |
| `server/utils/owner.ts` | `ownerId` resolution | Port tenant invariant into domain/storage layers |
| `server/utils/operations.ts` | Durable service-command enqueue | Replace file inbox with MongoDB-backed operation/outbox |
| `server/plugins/` | Embedded runtimes and Nitro lifecycle hooks | Split into independent Rust Agent/workers |
| `server/routes/ws/` | Legacy WebSocket routes | Reconsider under SSE-first roadmap; not part of the first slice |
| `shared/types/` | TypeScript contracts shared by app and Nitro | Map and port to `crates/contracts`; do not duplicate definitions |
| `shared/utils/` | Formatting, errors, operation streaming, Hub environment rules | Port only when used by a Rust contract or service |

### 3.2 Domain and data

| Source path | Responsibility | Migration disposition |
|---|---|---|
| `engine/ta.mjs`, `engine/indicators.mjs` | Deterministic technical calculations | Port to `crates/research-engine` with fixture parity |
| `engine/methods/` | VSA, price action, trend, and order-flow methods | Port one method/fixture set at a time |
| `engine/version.mjs` | Engine/params/data version stamps | Preserve equivalent provenance fields in Rust outputs |
| `engine/keys.mjs` | Deterministic idempotency keys | Port key derivation rules for retained read-only domains |
| `engine/news*.mjs` | News ingestion, clustering, extraction, assessment, outlook | Map to `workers/news-worker` and research crates |
| `engine/evidence.mjs`, `engine/evidenceSet.mjs` | Frozen evidence sets and hashes | Port to domain/contracts/storage; preserve append-only semantics |
| `engine/models/*.mjs` | MongoDB collection schemas and indexes | Keep existing collection/schema compatibility first |
| `engine/db.mjs` | MongoDB connection and model access | Replace with repository traits and MongoDB driver |
| `market/` | Canonical market events, clock, candle builder, quote/orderbook stores, Binance/Bybit/OKX providers | Port to `crates/market-providers` and `workers/market-worker` |
| `realtime/` | Realtime transport and event fan-out | Reuse event contracts; evaluate after core SSE/API stability |

### 3.3 Services and operations

| Source path | Responsibility | Migration disposition |
|---|---|---|
| `services/registry.mjs` | Machine-readable service definitions, dependencies, readiness rules | Reimplement as typed Rust registry in `crates/supervisor-core` |
| `services/supervisor.mjs` | Only lifecycle owner; operations, leases, reconciliation, restart policy | Reimplement in `apps/agent` and `crates/supervisor-core` |
| `services/workerHost.mjs` | Worker cycle loop, heartbeat, IPC protocol, graceful drain | Reimplement as Rust worker runtime/protocol |
| `services/commandInbox.mjs` | Atomic file command inbox and quarantine | Replace with MongoDB atomic claims/leases; preserve visibility of dead letters |
| `services/supervisorStore.mjs` | Durable local supervisor state and audit | Use MongoDB operation/service collections plus minimal local checkpoints |
| `services/readiness.mjs` | Observed state and tri-state readiness projection | Port as pure, tested Rust state machine |
| `services/workerProbes.mjs` | Mongo, sink, provider, and output evidence | Port as typed worker evidence |
| `services/*.mjs` collectors/scanners | Funding, scanner, news, regime, liquidation, confluence, macro, etc. | Port one worker at a time; omit bot execution workers |
| `tools/drills/` | Real supervisor/worker process drills | Reproduce as Rust integration/crash drills |

### 3.4 AI and notifications

| Source path | Responsibility | Migration disposition |
|---|---|---|
| `ai/gateway.mjs` | Provider protocol and health handling | Port to `crates/ai-providers` behind a generic OpenAI-compatible adapter first |
| `ai/providerStore.mjs`, `engine/models/aiProvider*.mjs` | Provider/model registry and encrypted credentials | Keep metadata compatible; move secrets to the secret store |
| `ai/daily-brief.mjs` | Evidence-bound briefing | Port after AI worker contract is stable |
| `ai/agent.mjs` | Research/copilot runtime | Port only read-only research behavior; no execution authority |
| `notify/outbox.mjs`, `notify/outboxDriver.mjs` | Durable notification queue and retries | Port to `crates/notification-core` and notification worker |
| `notify/router.mjs`, `notify/policy.mjs` | Routing and policy | Port after outbox parity |
| `notify/send.mjs`, `services/telegram.mjs` | Telegram delivery | Port to `crates/telegram-client`; token remains a secret reference |
| `engine/models/notification*.mjs` | Mutable outbox and immutable delivery ledger | Preserve separation and idempotency semantics |

### 3.5 Explicit exclusions and quarantine

The following source areas are not active migration targets:

```text
exec/                  # order/risk/paper execution paths
simulation/            # order simulation/execution
engine/models/order.mjs
engine/models/paper_order equivalent
engine/models/paper_orders, paper_fills, paper_accounts
engine/models/position.mjs
engine/models/exchange*.mjs
engine/models/bot*.mjs
engine/models/aiAgent*.mjs when configured for autonomous execution
server/api/v1/orders/**
server/api/v1/paper/**
server/api/v1/positions/**
server/api/v1/exchange-demo/**
server/api/v1/bots/**
```

Read-only historical research records may later be exposed only after their provenance and semantics are proven. No Rust endpoint or worker may call the excluded legacy execution modules.

`pine/` remains an external supporting research artifact. It is not copied into the Rust runtime unless a later parity task explicitly requires it.

## 4. Legacy runtime topology

The legacy system mixes several execution models:

1. The Nuxt/Nitro application serves UI and `/api/v1` endpoints.
2. Some continuous runtimes are embedded in Nitro plugins.
3. The market plane can run as a separate process or be embedded in Nitro.
4. The supervisor owns a set of worker host processes.
5. Periodic collectors are also available through the older in-process `services/run.mjs` path.

The Rust target removes that ambiguity:

```text
Desktop / Web
  -> Management API
  -> durable MongoDB operation + command records
  -> one Agent per host
  -> allowlisted Rust workers
```

The Agent becomes the only process owner. The API produces commands and read models but never spawns workers.

## 5. Existing behavioral invariants to preserve

These are implementation requirements, not optional optimizations:

- A process is not ready merely because a PID exists.
- A start operation completes only after readiness evidence is observed.
- A stop drains through the worker control channel; Windows does not assume `SIGTERM` is graceful.
- Desired state is operator intent and is never inferred from a heartbeat.
- Unexpected exits use bounded backoff and are audited.
- Operations are idempotent, asynchronous, and observable by operation ID.
- A skip is not a success and does not refresh last-success evidence.
- Readiness evidence is tri-state: observed true, observed false, or unknown.
- A stale or lost Agent is `UNKNOWN`, not falsely `STOPPED`.
- Durable queues prevent duplicate claims through atomic ownership and leases.
- Dead-lettered or expired work remains visible to operators.
- Closed candles are facts; news revisions and invalidations are append-only.
- Time is UTC, and observation/ingestion/publication timestamps are distinct.
- Output carries a schema/producer/input version and source provenance.

## 6. Existing test and drill inventory

### 6.1 Important legacy suites

| Area | Representative files/commands |
|---|---|
| Engine and Pine parity | `engine/test.mjs`, `tests/pineParity.test.ts`, `pnpm run test:engine` |
| Market normalization | `market/test.mjs`, `tests/market-feed.test.ts`, `pnpm run test:market` |
| Service registry/readiness | `tests/service-registry.test.ts`, `tests/service-readiness.test.ts` |
| Supervisor and operations | `tests/supervisor.test.ts`, `tests/service-actions.test.ts`, `tests/operations-view.test.ts` |
| Worker protocol/evidence | `tests/worker-probes.test.ts`, `tests/worker-evidence.test.ts`, `tests/worker-channel.test.ts` |
| Process reliability drills | `tools/drills/run.mjs`, `pnpm run drills` |
| Tenancy | `tests/tenancy-isolation.test.ts`, `tests/scope.test.ts` |
| News evidence | `tests/news*.test.ts` |
| AI/provider | `tests/aiProviders.test.ts`, `tests/aiProviderModels.test.ts`, `tests/aiResearch.test.ts` |
| Notification/outbox | `tests/notify.test.ts`, `tests/notify-policy.test.ts`, `tests/notify-outbox.test.ts` |
| API/application | `tests/run-tests.mjs`, `pnpm run test:app` |
| Aggregate legacy gate | `pnpm run verify` |

### 6.2 Migration evidence plan

Legacy tests are not blindly translated. Each port records:

1. the behavior or invariant being preserved;
2. the Rust test and fixture that proves it;
3. the old/new output comparison where deterministic;
4. any deliberate contract difference and its compatibility impact.

## 7. Known gaps and blockers

| Item | Status | Required next action |
|---|---|---|
| Live MongoDB collection names/indexes/TTLs | BLOCKED | Read-only inspection with the production-equivalent URI and credentials |
| Live MongoDB record shapes and legacy null/missing patterns | BLOCKED | Export sanitized representative documents from a non-production copy |
| Live TM Hub app registration and app ID | BLOCKED | Inspect `apps` and `system_routes` in the target Hub environment |
| Deployed Hub signing algorithm and key rotation | BLOCKED | Read `GET /api/v1/auth/jwks` and inspect token headers without logging tokens |
| Hub refresh/MFA response shape used by Desktop | BLOCKED | Execute the documented login/TOTP/refresh flow against a test account |
| Hub capabilities for the Rust app | BLOCKED | Define and seed read-only capabilities before enabling protected endpoints |
| Legacy working-tree cleanliness | PASS as a recorded fact | Do not use the dirty tree as a cutover baseline without owner review |

## 8. Phase 0 conclusion

The source contains enough domain, data, runtime, and reliability evidence to begin the Rust workspace. The first port must remain a read-only process/API/Agent slice and must not claim live database or Hub integration parity until the blocked checks above are completed.
