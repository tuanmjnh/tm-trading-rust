# Data Contract Map

> Audit date: 2026-11-17`<br>
> Source models: `D:\Applications\tm-trading\engine\models\*.mjs`<br>
> Target repository: `D:\Applications\tm-trading-rust`
> Authority: `docs/TM-TRADING-RUST-MASTER-ROADMAP.md`

## 1. Purpose and evidence boundary

This document maps the legacy MongoDB schemas and event semantics to the Rust domain, contracts, and repository layers.

The source evidence is code and historical audit documents. Live collection contents, effective indexes, TTL settings, validators, and document variants were not inspected. Every compatibility statement below therefore means:

- the model source declares this shape or index; and/or
- the source/tests document this behavior.

It does not claim that every live collection currently matches the model.

| Verification | Result | Next action |
|---|---|---|
| Model files and declared indexes read | PASS | Continue from `engine/models/` |
| Data ownership map read | PASS | Use as static classification evidence |
| Live collection list | BLOCKED | Read-only inspection in the target environment |
| Live indexes and TTLs | BLOCKED | Inspect `listIndexes` with sanitized metadata |
| Live document variants | BLOCKED | Export sanitized samples from a non-production copy |
| Rust deserialization of live samples | NOT RUN | Begins after repository trait and compatibility models exist |

## 2. Compatibility rules

1. Keep existing collection names and `_id` values where practical.
2. Keep existing field names for persisted legacy records; do not silently rename them.
3. New API DTOs may use clearer public names, but must map explicitly to legacy BSON fields.
4. Preserve deterministic natural IDs and unique indexes.
5. Do not rewrite historical facts to a new shape in place.
6. Store new versions, revisions, invalidations, or corrections append-only.
7. Use UTC in storage and RFC 3339 UTC strings in JSON APIs.
8. Keep observation, ingestion, publication, update, and business-day concepts distinct.
9. Keep `ownerId` separate from `accountId`; an account is never a tenant identity.
10. Treat mixed legacy payloads as untrusted until validated by kind/version.
11. Do not enable destructive migrations, mass backfills, or index removal during the initial port.
12. Every new write path must carry schema/producer/input version and source provenance.

## 3. Identity and ownership axes

| Axis | Legacy meaning | Rust treatment |
|---|---|---|
| `ownerId` | TM Hub user ID from JWT `sub` | `OwnerId` value in domain; required for owner-scoped reads/writes |
| `accountId` / `account` | Sub-book inside one owner | Separate value; never used as tenant scope |
| `serviceInstanceId` | One worker process generation on one host | New typed identity for service snapshots and operations |
| `agentId` | Host installation identity | New stable per-host identity, separate from process PID |
| `operationId` | Durable asynchronous control request | Required public correlation handle |
| `correlationId` | End-to-end request/correlation handle | Required in API errors, logs, commands, and results |
| `idempotencyKey` | Stable intent identity | Unique in the target scope; request hash guards payload reuse |

Legacy documents with `ownerId: null` or no owner field remain **unscoped legacy records**. The Rust layer must not silently attach them to the first authenticated user. A separate backfill/audit decision is required before owner-scoped APIs expose them.

## 4. Time semantics

The legacy schemas use several representations. Rust readers must accept these during compatibility periods and normalize only at the contract boundary.

| Legacy representation | Examples | Rust boundary |
|---|---|---|
| BSON date | `Date` in news/source/signal fields | `chrono::DateTime<Utc>` / `time::OffsetDateTime` |
| Epoch milliseconds | `ts`, `at`, `eventTime`, `ingestTime`, legacy operation timestamps | Explicit `UnixMilli` conversion |
| Epoch seconds | Some provider/event payloads | Validate range before conversion; never guess |
| ISO-like string | Domain events and audit records | Parse as UTC RFC 3339; reject ambiguous local timestamps |
| Business day | `YYYY-MM-DD` | Keep as a date string in business-day domains, never convert to local midnight |

Required distinctions:

- `observedAt`: when the system observed an event.
- `ingestedAt`: when the record entered storage.
- `publishedAt`: when a publisher stated the event happened.
- `firstSeenAt`: first observation by this system.
- `updatedAt`: last material update; it does not replace `ingestedAt`.
- `eventTime`: source/event time for market and trading-domain events.
- `expiresAt`: validity boundary; readers must label expired data stale.

## 5. Existing collection map

### 5.1 Market and research

| Collection | Source model | Scope/compatibility class | Rust target | Action |
|---|---|---|---|---|
| `candles` | `candle.mjs` | Global source facts | Market/storage repository | KEEP schema first |
| `market_feed` | `marketFeed.mjs` | Global append-only observations | Market repository | KEEP; type payload variants later |
| `market_snapshots` | `snapshot.mjs` | Owner-scoped decision context; historically execution-oriented | Research/storage | Read-only compatibility until semantics are audited |
| `intel` | `intel.mjs` | Mixed global snapshot/event records | Market/research read repository | KEEP collection; split typed DTOs by `kind` |
| `signals` | `signal.mjs` | Owner/account scoped research events | Research repository | PORT as read-only research event |
| `research_evidence_sets` | `evidenceSet.mjs` | Owner/account scoped frozen evidence | Evidence repository | KEEP deterministic ID/hash semantics |
| `runs` | `run.mjs` | Owner-scoped research/backtest run | Research repository | PORT read-only; preserve version stamps |
| `datasets` | `dataset.mjs` | Research dataset manifest | Research repository | PORT later |
| `indicator_runs` | `indicatorRun.mjs` | Owner-scoped indicator output | Research repository | PORT later with fixture parity |
| `experiments`, `experiment_runs` | `experiment*.mjs` | Owner-scoped research | Research repository | PORT only if retained in read-only product |
| `trading_domain_events` | `tradingDomainEvent.mjs` | Owner/account scoped domain events | Compatibility read repository | READ ONLY; execution delivery is excluded |
| `trade_context` | `tradeContext.mjs` | Owner-scoped research context | Research repository | PORT later, read-only |

### 5.2 News and macro

| Collection | Source model | Scope/compatibility class | Rust target | Action |
|---|---|---|---|---|
| `news_sources` | `newsSource.mjs` | Global provider/source registry | News storage/provider core | KEEP |
| `news_items` | `newsItem.mjs` | Global rights-safe normalized items | News storage | KEEP |
| `news_item_revisions` | `newsItemRevision.mjs` | Global append-only revision history | News storage | KEEP append-only |
| `news_assessments` | `newsAssessment.mjs` | Global versioned extraction | News/research storage | KEEP versions side by side |
| `news_market_events` | `marketEvent.mjs` | Global clustered story/event | News/research storage | KEEP corrections/retractions explicit |
| `news_market_outlooks` | `marketOutlook.mjs` | Global time-bounded research outlook | Research storage | KEEP expiry/invalidation |
| `news_evidence_sets` | `newsEvidenceSet.mjs` | Owner-scoped frozen news evidence | Evidence repository | KEEP hash/usage semantics |
| `news_model_evaluations` | `newsEvaluation.mjs` | Owner-scoped evaluation | Research storage | PORT later |
| `news_domain_events` | `newsDomainEvent.mjs` | Global durable audit events | Event storage | KEEP; longer retention than source items |
| `news_dead_letters` | `newsDeadLetter.mjs` | Global operator-visible failures | News storage | KEEP; body must not be retained |
| `news_macro_series` | `macroSeries.mjs` | Global series definition | Macro storage | PORT later |
| `news_macro_observations` | `macroObservation.mjs` | Global vintage-aware observation | Macro storage | KEEP vintage semantics |
| `news_macro_calendar` | `macroCalendarEvent.mjs` | Global calendar event | Macro storage | PORT later |

### 5.3 Alerts, notifications, and AI providers

| Collection | Source model | Scope/compatibility class | Rust target | Action |
|---|---|---|---|---|
| `alerts` | `alert.mjs` | Owner-scoped event | Alert/research repository | PORT informational alerts only |
| `alert_rules` | `alertRule.mjs` | Owner-scoped condition | Alert repository | KEEP rule identity and owner scope |
| `notification_outbox` | `notificationOutbox.mjs` | Owner-scoped mutable delivery queue | Notification repository | PORT with claim/lease parity |
| `notification_logs` | `notificationLog.mjs` | Owner-scoped immutable ledger | Notification repository | PORT; failures remain visible |
| `ai_provider_connections` | `aiProviderConnection.mjs` | App/owner provider metadata plus encrypted secret | Provider repository + secret store | KEEP metadata; migrate secrets separately |
| `ai_provider_models` | `aiProviderModel.mjs` | Per-connection model metadata | Provider repository | KEEP |
| `ai_provider_audit` | `aiProviderAudit.mjs` | App audit history | Provider audit repository | KEEP; no secrets |
| `ai_proposals` | `aiProposal.mjs` | Owner-scoped AI research output | Research repository | PORT only as read-only proposal |

### 5.4 User-owned application data

| Collection | Source model | Rust target | Action |
|---|---|---|---|
| `watchlists` | `watchlist.mjs` | Watchlist repository | PORT with owner isolation and name uniqueness |
| `news_evidence_sets` | `newsEvidenceSet.mjs` | Evidence repository | PORT with owner scope |
| `research_evidence_sets` | `evidenceSet.mjs` | Evidence repository | PORT with owner scope |

### 5.5 Quarantined collections

These collections are not part of the active Rust data plane:

```text
paper_accounts
paper_orders
paper_fills
positions
trades
equity
risk_state
exchange_demo_connections
exchange_order_mappings
exchange_events
venue_preferences
bot_profiles
bot_instances
bot_commands
bot_supervisors
journal
```

The Rust application must not expose write repositories for them. A future read-only archive feature requires a separate explicit scope decision and must not inherit execution semantics.

## 6. Representative schema contracts

### 6.1 `candles`

Source: `engine/models/candle.mjs`.

| Field | Legacy type | Required | Rust compatibility |
|---|---|---|---|
| `_id` | String | Yes | `${symbol}:${timeframe}:${openTime}` |
| `symbol` | String | Yes | String |
| `timeframe` | String | Yes | String |
| `market` | String/null | No | Keep optional |
| `source` | String | Yes | Provider/source ID |
| `state` | String | Yes | Always `closed` in this collection |
| `open`, `high`, `low`, `close` | Number | Required | Finite decimal values |
| `volume` | Number | No | Default `0` |
| `openTime`, `closeTime` | Epoch milliseconds | Required | Preserve integers |
| `eventTime` | Epoch milliseconds/null | No | Source/event time |
| `ingestTime` | Epoch milliseconds/null | No | Arrival observation |
| `schemaVersion` | String | No | Keep default/version stamp |

Declared indexes: `{ symbol, timeframe, openTime }`, `{ openTime }`, `{ source, openTime }`. Live index existence is `BLOCKED`.

### 6.2 `intel`

Source: `engine/models/intel.mjs`.

The collection mixes current snapshots and retained events under a unique `key`. `kind` is one of `mover`, `flow`, `accum`, `funding`, `news`, `alert`, `regime`, `zone`, `oi`, `supply`, `confluence`.

Compatibility fields:

```text
key: String (unique)
kind: String
symbol: String?
title: String
ts: Date
score: Number?
data: Mixed object
createdAt / updatedAt
```

Rust initially reads `data` as a validated JSON value and maps known `kind` variants to typed DTOs. It must not assume every variant has the same inner shape.

Declared indexes: `{ kind, ts }`, `{ symbol, ts }`, TTL `{ ts }` after seven days.

### 6.3 `signals`

Source: `engine/models/signal.mjs`.

Compatibility fields include:

```text
ts, symbol, tf, type, price, direction, confidence, ownerId, accountId,
source, signalKey, attribution, evidence, quality, createdAt, updatedAt
```

`type` currently allows `SV`, `BC`, `ST LONG`, `ST SHORT`, `NS`, `ND`, `EoM`. A partial unique index protects non-null `signalKey` values, allowing legacy rows without attribution or signal keys to coexist.

Rust must preserve legacy rows and expose them as research observations. It must not infer execution intent from the signal type.

### 6.4 `research_evidence_sets`

Source: `engine/models/evidenceSet.mjs`.

- `_id` and `setId` are deterministic.
- `kind`, `sourceId`, `hash`, and content are frozen facts.
- `usedByExperimentIds` is the only permitted append-only reference update.
- `ownerId` and `accountId` remain distinct.

Rust storage must reject a changed hash for an existing set ID and route the conflict as a contract error rather than overwrite the evidence.

### 6.5 `watchlists`

Source: `engine/models/watchlist.mjs`.

- `ownerId` scopes the list.
- `name` is unique per owner with case-insensitive collation.
- `symbols` is an ordered, de-duplicated list of uppercase symbols.
- `isDefault` identifies the default owner list.
- `sharedWith` is explicit sharing metadata, not a substitute for owner checks.

Rust must preserve the declared unique index semantics and test tenant isolation.

### 6.6 `news_items`

Source: `engine/models/newsItem.mjs`.

Important compatibility fields:

```text
_id, sourceId, url, originUrl, title, excerpt, language,
publishedAt, firstSeenAt, ingestedAt, marketGroup, eventType, tags,
level, score, dir, rights, hash, contentHash, updatedAt, schemaVersion
```

Rules:

- `url` is the canonical identity; `originUrl` is the citation link.
- `publishedAt` may be absent and must not be replaced with `ingestedAt`.
- `firstSeenAt` and `ingestedAt` are distinct.
- Only title and excerpt are stored; full article text is not.
- A material update first appends `news_item_revisions` and then updates the envelope.
- `ingestedAt` remains frozen on re-ingest and is the declared TTL base.

### 6.7 `news_market_events`

Source: `engine/models/marketEvent.mjs`.

- Deterministic fingerprint-derived `_id` clusters one event across source items.
- `status` distinguishes `active`, `corrected`, and `retracted`.
- Revisions are append-only.
- Corroboration and source items retain provenance.
- `lastSeenAt` is an observation timestamp, not a publication timestamp.

### 6.8 `news_market_outlooks`

Source: `engine/models/marketOutlook.mjs`.

- Deterministic identity is group, horizon, and UTC business day.
- `generatedAt`, `expiresAt`, and `invalidatedAt` are explicit.
- Evidence references point to event/item IDs and retain URLs where available.
- Expired or invalidated outlooks remain readable but must be rendered stale/invalidated.

### 6.9 `notification_outbox` and `notification_logs`

Source: `engine/models/notificationOutbox.mjs` and `notificationLog.mjs`.

The two collections intentionally have different jobs:

| Collection | Purpose | Mutability |
|---|---|---|
| `notification_outbox` | Durable work, attempts, next attempt, provider message ID, dead state | Mutable while retryable |
| `notification_logs` | Immutable record of what was routed and what happened per channel | Append-only/first-write-wins |

The outbox `_id` is `${notifyId}:${channel}`. The log `_id` is `${notifyId}:${at}`. Rust must preserve this separation so retries do not rewrite the delivery ledger.

### 6.10 `ai_provider_connections` and `ai_provider_models`

Source: `engine/models/aiProviderConnection.mjs` and `aiProviderModel.mjs`.

- Provider connection metadata includes provider, adapter, base URL, model, enabled state, capabilities, status, and optional Hub connection reference.
- `secretEnc`, `secretMask`, and `secretFingerprint` are legacy secret fields.
- Model identity is `${connectionId}::${model}`.
- A partial unique index permits at most one primary model per connection.

Initial Rust behavior:

1. Read legacy metadata and masked/fingerprint fields without exposing ciphertext.
2. Accept new provider secrets only through the secret-store abstraction.
3. Do not delete or rewrite `secretEnc` during metadata migration.
4. Deactivate a legacy secret reference only after a tested secure-store path exists.

## 7. Event contracts

### 7.1 Existing domain-event envelope

The legacy trading/news domain-event models use the following durable envelope:

```text
eventId
type
entity: { kind, id }
occurredAt
emittedAt
idempotencyKey
correlationId
audit
schemaVersion
```

Rust shared events should preserve these concepts and add explicit producer/input provenance where needed. A domain event is immutable; corrections are new events.

### 7.2 Rust shared evidence fields

The target contract should expose the common fields below where applicable:

```text
source / provider
observedAt
ingestedAt
publishedAt          # news-specific
asset / symbol / timeframe
inputSnapshotId
producerVersion
schemaVersion
qualityState
qualityReasons
sourceUrl            # news/evidence where available
createdAt / updatedAt
```

These fields are additive compatibility metadata. Legacy records without them are read as `unknown`/`unverified`, not filled with fabricated values.

## 8. New Rust operation, Agent, and service contracts

The legacy supervisor stores its state in files and uses a file command inbox. The Rust roadmap intentionally replaces that with MongoDB.

### 8.1 Proposed `operations` collection

> Implemented in `crates/storage/src/mongo_operations.rs` and verified against TLS-enabled MongoDB 8.2. Documents use these field names verbatim; `_id` is the operation UUID string. Timestamps are stored as BSON `DateTime`. Every transition is a compare-and-swap on `fencingToken`.

One document is both the durable operation record and the command/outbox item. This avoids a non-atomic write across separate operation and command collections.

Required fields:

```text
_id / operationId
schemaVersion
idempotencyKey
requestHash
requestedBy
requestedAt
agentId
serviceInstanceId
serviceDefinitionId
action
status
attempt
leaseOwner
leaseExpiresAt
fencingToken
startedAt
finishedAt
expiresAt
resultSummary
redactedError
errorCode
correlationId
```

Required indexes:

```text
unique { requestedBy, idempotencyKey }
{ status, agentId, expiresAt }
{ agentId, leaseExpiresAt, status }
{ serviceInstanceId, requestedAt }
{ correlationId }
```

Claim behavior:

1. Atomically update one record whose status is claimable, target Agent matches, and lease/expiry conditions are satisfied.
2. Increment `attempt` and assign a new fencing token.
3. A worker result with an older fencing token is rejected.
4. Expired unclaimed work becomes `EXPIRED`.
5. Reusing an idempotency key with a different request hash fails explicitly.
6. Terminal records remain queryable; they are not deleted as audit history.

### 8.2 Proposed `service_instances` collection

> Implemented in `crates/storage/src/mongo_services.rs`. `_id` is `serviceInstanceId`. The current read model persists `agentId`, `serviceDefinitionId`, `desiredState`, `observedState`, `readiness`, `processAlive`, `restartCount`, `lastExitCode`, `retryBlocked`, `lastHeartbeatAt`, `updatedAt`, and `schemaVersion = 1`. `generation`, `pid`, `host`, executable version, readiness-check detail, and the output/error counters listed below are not collected yet.

```text
_id / serviceInstanceId
agentId
serviceDefinitionId
generation
desiredState
observedState
readiness
readinessChecks
pid
executableVersion
host
startedAt
lastHeartbeatAt
lastSuccessAt
lastOutputCount
lastError
lastErrorAt
restartCount
nextRestartAt
updatedAt
schemaVersion
```

This is a snapshot/read model. The Agent is authoritative for the current host state; the API reads persisted snapshots.

### 8.3 Proposed `agents` collection

```text
_id / agentId
installationId
displayName
hostOs
hostArchitecture
version
capabilities
status
registeredAt
lastSeenAt
revokedAt
schemaVersion
```

Agent registration and revocation are per host. A static shared Agent secret is not acceptable for remote mode.

### 8.4 Proposed `operation_audit` collection

Append-only audit entries for operation creation, claim, progress, completion, expiry, revocation, and service reconciliation. Entries contain IDs, actor, action, timestamp, and redacted detail only.

### 8.5 Local Agent checkpoint (not MongoDB)

The current local runtime slice persists `agent-state.json` inside the installation-local Agent state directory. The same directory contains `agent.lock`, which enforces one Agent owner per installation. The previous known-good checkpoint is retained as `agent-state.json.bak`; a missing or corrupt primary is restored from that backup.

The checkpoint is deliberately smaller than the proposed MongoDB `service_instances` snapshot:

```text
schemaVersion
+agentId
+services[]
+  serviceDefinitionId
+  serviceInstanceId
+  desiredState
+  observedState
+  readiness
+  processIdentity { pid, executablePath, executableVersion }
+  lastExitCode
+  restartCount
+  nextRestartAt
+  retryBlocked
```

`retryBlocked` defaults to `false` when absent so schema-v1 checkpoints created before deterministic-failure quarantine remain readable. A true value records that the worker was quarantined after a deterministic protocol, schema, or identity rejection and must not be automatically retried.

Its only durable purpose is local restart reconciliation. It contains no durable operation, command, lease, fencing token, readiness evidence details, heartbeat history, log tail, or complete service snapshot. After an Agent restart, a previously desired-running service is reconciled as failed and scheduled for a fresh bounded replacement unless its restart budget is exhausted or it is quarantined; the checkpoint never proves that the old PID is still live or ready. Agent-level shutdown preserves desired `RUNNING` and any pending restart schedule without consuming the restart budget, while an explicit service stop persists desired `STOPPED`.

MongoDB remains the target for durable operation and service-snapshot persistence. When `service_instances` is implemented, its document remains the authoritative persisted service projection read through the Management API; it must not be conflated with or replaced by `agent-state.json`.

## 9. Repository boundaries

The Rust repository layer should expose narrow domain traits rather than leaking BSON into contracts.

Conceptual first traits:

```text
OperationRepository
ServiceInstanceRepository
AgentRepository
```

Later domain traits:

```text
MarketObservationRepository
ResearchEventRepository
EvidenceRepository
WatchlistRepository
NewsRepository
ProviderRepository
NotificationOutboxRepository
```

Contracts use public DTOs. Storage adapters translate DTOs to BSON and preserve legacy field names. A compatibility model may use `serde_json::Value` or a tagged enum for genuinely mixed legacy payloads, but boundary validation must occur before domain use.

The Phase 1 in-memory repository is a deterministic test implementation. It must support atomic claim semantics so tests do not accidentally depend on an easier single-threaded implementation.

## 10. Retention, quality, and backup obligations

- Retention is per collection and explicit; no automatic deletion of important research evidence.
- News and news-event TTLs from legacy models must be verified live before Rust relies on them.
- Append-only revisions, dead letters, operation history, and audit records need retention at least as long as their replay/repair path.
- Every source of truth should expose latest successful update and freshness state.
- A stale value must carry a stale/expired label.
- Backup/restore must be tested against a non-production copy before cutover.
- Retention and backup jobs must report counts and dry-run plans before deletion.

## 11. Open data decisions

| Decision | Status | Reason |
|---|---|---|
| Live collection/index inventory | BLOCKED | Requires target MongoDB access |
| Legacy unscoped-record backfill | BLOCKED | Requires data-owner and meaning audit; not part of initial port |
| New operation collection name | Proposed | `operations` chosen to keep operation and outbox atomic |
| Legacy `intel.data` typed split | Deferred | Known-kind adapters first; preserve raw compatibility value |
| Legacy timestamp normalization | Deferred | Compatibility readers first; write one normalized representation per new domain |
| Legacy secret migration | Deferred | Requires secure store and rollback evidence |
| Retention policy changes | Deferred | Requires live volume/TTL evidence and approval |
| Destructive migrations | Excluded | No initial-port migration |

## 12. Data acceptance gates for the first domain ports

Before a legacy domain is exposed through Rust:

1. A sanitized representative document deserializes successfully.
2. Owner filtering is tested where the collection is owner-scoped.
3. Legacy timestamps map to the correct UTC/observation concepts.
4. Deterministic IDs and unique indexes remain stable.
5. Rust output uses the existing collection and field names.
6. Quality/freshness is explicit for missing legacy metadata.
7. No write occurs unless the worker has an explicit source-of-truth flag.
8. Old/new output is compared using a captured fixture or isolated database.
9. Rollback does not require rewriting already-written records.
10. Live index/TTL evidence is attached before production cutover.
