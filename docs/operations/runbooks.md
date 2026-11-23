# Operations Runbooks

> Status: Phase 3 local Management API and Agent command channel, Phase 4 Desktop Control Center, Phase 5 market data and watchlists, Phase 6 research events, and Phase 7's core news plus AI provider/model/routing, evidence-bound briefing requests, and the supervised AI Worker are implemented (2026-11-22). Dedicated Anthropic and Gemini adapters remain pending.<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## Build the Agent and sample worker

From the repository root:

```powershell
New-Item -ItemType Directory -Force target/tmp
$env:TMP = (Resolve-Path target/tmp).Path
$env:TEMP = $env:TMP
cargo build -p tm-trading-agent -p tm-trading-sample-worker
```

Expected artifacts:

```text
target/debug/tm-trading-agent.exe
target/debug/tm-trading-sample-worker.exe
```

Repeat the same drills against `target/release` after `cargo build --workspace --release`.

## Graceful-shutdown drill

Run the Agent and sample worker, wait for readiness, then stop the worker through the explicit JSON-line control channel:

```powershell
$stateDir = Join-Path (Resolve-Path target/tmp).Path ("agent-self-test-" + [guid]::NewGuid().ToString("N"))
.\target\debug\tm-trading-agent.exe --self-test --worker-dir .\target\debug --state-dir $stateDir
```

Expected result:

- the worker becomes `READY` while its process is alive;
- shutdown reports `drained: true`;
- shutdown does not require a forced kill (`forced: false`);
- final desired and observed states are `STOPPED` and readiness is `NOT_READY`.

## Crash and restart drill

The drill makes the first worker process exit with code 9. The Agent must observe the failure, wait for bounded backoff, start one replacement instance, observe readiness, and then stop it gracefully:

```powershell
$stateDir = Join-Path (Resolve-Path target/tmp).Path ("agent-crash-test-" + [guid]::NewGuid().ToString("N"))
.\target\debug\tm-trading-agent.exe --crash-after-ms 100 --worker-dir .\target\debug --state-dir $stateDir
```

Expected result:

- `restart_count` increases from the run baseline;
- the replacement worker reaches `RUNNING` and `READY`;
- final shutdown reports `drained: true` and `forced: false`.

## Run the Management API and Agent command channel

The Management API binds to loopback only and requires two separate credentials. The user credential carries exact `trading.*` capabilities; the Agent credential is per installation and may only act as the Agent named in the request.

```powershell
cargo build -p tm-trading-management-api -p tm-trading-agent -p tm-trading-sample-worker

$env:TM_TRADING_LOCAL_API_TOKEN = "local-user-token"
$env:TM_TRADING_AGENT_TOKEN = "local-agent-token"
$env:TM_TRADING_API_BIND = "127.0.0.1:8787"
.\target\debug\tm-trading-management-api.exe
```

In a second terminal, connect the Agent to that API. The Agent credential is read from the environment so it never appears in the process command line:

```powershell
$env:TM_TRADING_AGENT_TOKEN = "local-agent-token"
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local --api-url http://127.0.0.1:8787
```

The Agent heartbeats its service read model and then claims commands addressed to its own Agent identity. Drive it over HTTP:

```powershell
$headers = @{ Authorization = "Bearer local-user-token" }
$services = Invoke-RestMethod -Uri http://127.0.0.1:8787/api/v1/services -Headers $headers
$instance = $services[0].service_instance_id
$body = @{ service_instance_id = $instance; action = "stop" } | ConvertTo-Json
$operation = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8787/api/v1/services/$instance/operations" `
    -Headers ($headers + @{ "Idempotency-Key" = "stop-sample-1" }) -ContentType "application/json" -Body $body
Invoke-RestMethod -Uri "http://127.0.0.1:8787/api/v1/operations/$($operation.operation_id)" -Headers $headers
```

Expected result:

- the operation is returned as `accepted` with an operation ID;
- the Agent claims it, reports it running, and completes it;
- the final operation status is `succeeded` with a result summary, or `failed` with a redacted error;
- the service read model shows the new desired, observed, and readiness values.

The read model is authoritative per Agent: each heartbeat replaces that Agent's reported instances, so an instance the Agent no longer reports disappears instead of lingering.

## Run the Agent

```powershell
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```
The Agent starts the registered sample worker and remains independent of any Desktop process. The state directory is installation-local: the Agent holds an exclusive `agent.lock` file for its lifetime and writes a minimal `agent-state.json` checkpoint with an `agent-state.json.bak` recovery copy. A second Agent using the same state directory fails closed.

Press `Ctrl+C` to request Agent shutdown; on Unix, `SIGTERM` requests the same shutdown. Shutdown requests are monotonic and cannot be cancelled by a later signal. The Agent then sends the explicit worker shutdown command and waits up to the configured grace period before escalating. After the worker process exits, the Agent collects the final protocol events for at most one second so the reported drain status reflects the worker's own final message. A forced kill is reaped for at most five seconds. Windows console close, logoff, and shutdown events use the same drain path.

Windows console hosting note: `Ctrl+C` is delivered only when the Agent is hosted by an existing console (Windows Terminal, `cmd.exe`, PowerShell). A dedicated console window — Explorer double-click or `cmd /c start` — is created with `CREATE_NEW_CONSOLE`, and Windows disables `Ctrl+C` for every process in that console until the application calls `SetConsoleCtrlHandler(NULL, FALSE)`. The Agent does not call that API because this workspace forbids unsafe code, so in a dedicated console window use the console close action, which uses the same drain path. Verified on Windows: the `CTRL_C_EVENT` is delivered by the operating system but not received by the application in a `CREATE_NEW_CONSOLE` console, and is received after the ignore attribute is cleared.

The worker is spawned detached from the Agent console on Windows, so a console close, logoff, or shutdown event reaches the Agent only. The Agent then drains the worker over the explicit control channel; the worker exits with code `0` instead of being terminated by the console event with `0xC000013A`.

Agent shutdown preserves the worker's desired `RUNNING` state, pending restart backoff, and restart budget. The next Agent launch reconciles that desired state. Only an explicit service stop persists desired `STOPPED` and cancels a pending automatic restart.

## Run the Desktop Control Center

The Desktop app is a view over the Management API. It needs the API running and a user bearer token; it never talks to the Agent or MongoDB. A release build is a Windows GUI binary, so launching it opens only the window, with no console attached; debug builds keep the console for startup logs.

```powershell
$env:TM_TRADING_API_URL = "http://127.0.0.1:8787"
$env:TM_TRADING_LOCAL_API_TOKEN = "local-user-token"
.\target\debug\tm-trading-desktop.exe
```

Expected result:

- the header shows `Connected`;
- the table lists the Agent's reported instances with desired state, observed state, readiness, restart count, and last exit code;
- `Start` is disabled while desired state is `RUNNING`, and `Stop`/`Restart` are disabled while it is `STOPPED`;
- an action shows `Waiting for the Agent to finish…` while the operation is pending and then the terminal operation status with the Agent's result summary;
- the `EN`/`VI` toggle switches every label, and the choice is remembered for the session;
- the Watchlists panel lists the authenticated subject's lists, marks the default one, and lets you create a list, replace its symbols with a comma-separated field, and delete it; the server normalizes symbols, so `btcusdt` is stored as `BTCUSDT` and duplicates collapse;
- the AI Briefing panel lists the authenticated owner's research events, queues one `research_explanation` request for the selected event, shows its queued, claimed, or terminal status, and exposes the published briefing after success;
- closing the window leaves the Agent and its workers running; the worker is spawned detached and is not a child of the Desktop process.

The WebView2 profile lives in `<exe_dir>/data/desktop`. Override it with `TM_TRADING_DESKTOP_DATA_DIR` when the executable directory is read-only. If the window never appears while `msedgewebview2.exe` crash events with `0x80000003` are logged, the profile directory is the first thing to relocate.

Localized UI strings live in `apps/desktop/src/i18n.rs`. Operation result summaries are produced by the Agent and are returned in English; they are not yet routed through the localization layer.

## Run with durable stores

By default the Management API keeps operations and service snapshots in memory. Set `TM_TRADING_STORE=mongodb` to persist both; the connection must use TLS with certificate validation, and `TM_ENVIRONMENT=production` requires this setting.

```powershell
$env:TM_TRADING_LOCAL_API_TOKEN = "local-user-token"
$env:TM_TRADING_AGENT_TOKEN = "local-agent-token"
$env:TM_TRADING_API_BIND = "127.0.0.1:8787"
$env:TM_TRADING_STORE = "mongodb"
$env:TM_DATABASE_URI = "mongodb://db-host:27017/?tls=true&tlsCAFile=D:/certs/ca.pem"
$env:TM_DATABASE_NAME = "tm-trading-rust"
.\target\debug\tm-trading-management-api.exe
```

Expected result:

- startup creates the five `operations` indexes from `docs/migration/data-contract-map.md` section 8.1, the `agentId` index on `service_instances` from section 8.2, the three `candles` indexes, and the two `watchlists` indexes;
- restarting the API keeps every operation readable with its terminal status and result summary;
- replaying the same `Idempotency-Key` after a restart returns the original operation ID;
- restarting the API with the Agent stopped still serves the last reported service snapshot, with its original `updated_at`;
- a new Agent heartbeat replaces that Agent's rows instead of duplicating them;
- a document with an unknown schema version, an unparseable enum, or an operation that violates the domain invariants is reported as unreadable and is never served as valid state.

Live MongoDB tests are separate from the default test run. They create and drop a throwaway database per test:

```powershell
$env:TM_TRADING_TEST_MONGODB_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
cargo test -p tm-trading-storage --test mongo_repository
cargo test -p tm-trading-storage --test mongo_services
cargo test -p tm-trading-storage --test mongo_candles
cargo test -p tm-trading-storage --test mongo_watchlists
cargo test -p tm-trading-storage --test mongo_ai_briefings
cargo test -p tm-trading-storage --test mongo_ai_jobs -- --ignored
```

The URI must be TLS-enabled and validate against the local trust store or the referenced CA file. Any `mongod` started with `--tlsMode requireTLS --tlsCertificateKeyFile <server.pem> --tlsCAFile <ca.pem> --tlsAllowConnectionsWithoutCertificates` works; the certificate must carry `localhost` as a subject alternative name.

Without `TM_TRADING_TEST_MONGODB_URI` the ordinary live-store tests return immediately and print `skipped`, so a green run is not evidence that a store works. The AI job transaction suite is explicitly ignored by default and fails immediately when selected without the URI. The live runs above are the evidence.

## Snapshot projection behaviour

The Agent is authoritative for its own service instances. Each heartbeat upserts the reported instances and then deletes rows owned by that Agent that were not reported, so a removed instance disappears instead of lingering. Upserts happen before the delete: an interrupted heartbeat leaves an extra row until the next beat rather than losing state.

A heartbeat carrying a snapshot for a different Agent is rejected with `422`; the API never writes across ownership boundaries. Rows are kept after an Agent stops reporting, because the persisted row is the last known observed state, not proof that the service is alive. Check `updated_at` and `lastHeartbeatAt` before trusting a row.

## Closed-candle store

Closed candles are keyed by `{symbol}:{timeframe}:{openTime}`, so re-recording the same close updates one row instead of adding a duplicate. Forming bars are never written. Rows written by the legacy recorder remain readable: a missing `volume` reads as `0`, a missing `schemaVersion` as `candle.v1`. A row whose `state` is not `closed`, whose `_id` disagrees with its fields, or that carries an unknown schema stamp is reported as unreadable and is never served as a valid bar. No Management API endpoint serves candles yet.

## Watchlist store

A watchlist is owned by the authenticated subject, never by a caller-supplied owner field. The first list an owner creates becomes that owner's default, and `{ownerId, name}` is unique with case-insensitive collation, so `Core` and `core` collide for one owner but not across owners. Symbols are trimmed, uppercased, de-duplicated, and kept in first-appearance order. Any id belonging to another owner is reported as `404`, not `403`, so the API never confirms that a foreign row exists.

Drive the endpoints with the same user credential:

```powershell
$headers = @{ Authorization = "Bearer local-user-token" }
$body = @{ name = "Core"; symbols = @("btcusdt", "ethusdt") } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:8787/api/v1/watchlists -Headers $headers -ContentType "application/json" -Body $body
Invoke-RestMethod -Uri http://127.0.0.1:8787/api/v1/watchlists -Headers $headers
```

The store persists to `watchlists` with the legacy field names and `ObjectId` `_id`, so rows written by this service remain readable by the legacy application. The Agent credential cannot use the endpoints: it receives `403` because it holds no user capabilities.

## Public market-data adapters

Binance spot and OKX swap adapters live in `tm-trading-market-providers`; see [`docs/providers/provider-catalog.md`](../providers/provider-catalog.md). They only request public bars and one public health endpoint, they never carry credentials, and they drop a still-forming bar instead of recording it. Their live tests reach the internet and run only when `TM_TRADING_LIVE_PROVIDER_TESTS=1` is set:

```powershell
$env:TM_TRADING_LIVE_PROVIDER_TESTS = "1"
cargo test -p tm-trading-market-providers --test live_public
```

## Provider instances

A provider instance is the operator's configuration for one catalog adapter. The catalog is
compiled into the service and listed at `GET /api/v1/providers/catalog`; instances live at
`GET /api/v1/providers`.

```powershell
$headers = @{ Authorization = "Bearer local-user-token" }
$body = @{ provider_definition_id = "binance"; display_name = "Binance Public" } | ConvertTo-Json
$instance = Invoke-RestMethod -Method Post -Uri http://127.0.0.1:8787/api/v1/providers -Headers $headers -ContentType "application/json" -Body $body
Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8787/api/v1/providers/$($instance.id)/test" -Headers $headers
```

The probe sends exactly one small public request through the instance's own base URL and
timeout, then records the outcome on the instance. A provider that fails the probe still
returns `200`; read `health_state` and `last_error_code` for the verdict. The sanitized
error classes are `configuration`, `unavailable`, `timeout`, `http_status`,
`invalid_payload`, and `unsupported_capability`.

A base URL must use `https`, except for a loopback host. A URL with credentials in it, a
query, or a fragment is rejected with `422`. The probe timeout must be between 500 ms and
30 s. Configuration and observation are separate: editing an instance never claims a fresh
probe, and a probe never changes the operator's settings.

The Agent credential cannot use these endpoints: it receives `403` because it holds no user
capabilities.

## Run the Market Worker

The Market Worker is opt-in and runs under the Agent. Build it together with the Agent, then
start the Agent with the write authority explicitly enabled. The database URI is passed only to
that worker; the Agent's other children never receive it.

```powershell
cargo build -p tm-trading-agent -p tm-trading-sample-worker -p tm-trading-market-worker

$env:TM_TRADING_MARKET_WORKER_ENABLED = "1"
$env:TM_TRADING_MARKET_SYMBOLS = "BTCUSDT,ETHUSDT"
$env:TM_TRADING_MARKET_TIMEFRAME = "1m"
$env:TM_TRADING_MARKET_PROVIDER_ID = "binance"
$env:TM_TRADING_MARKET_SOURCE_OF_TRUTH = "1"
$env:TM_DATABASE_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
$env:TM_DATABASE_NAME = "tm-trading-rust"
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```

Expected result:

- `GET /api/v1/services` lists `market-worker` alongside `sample-worker`;
- `market-worker` stays `NOT_READY` until a real cycle stores closed bars, then becomes `READY`;
- each cycle upserts closed bars into `candles` and updates `healthState` on the provider instance;
- the worker never writes a forming bar, and a cycle that returns no closed bars is reported as skipped, not successful.

With `TM_TRADING_MARKET_SOURCE_OF_TRUTH` unset or `0`, the worker exits with code `78` before
touching a provider or a candle. That is the dual-writer guard, not a runtime failure to retry.

`TM_TRADING_MARKET_PROVIDER_ID` is optional only when exactly one provider definition is
enabled. If both Binance and OKX instances are enabled and no definition is named, startup is
rejected as ambiguous instead of silently picking one.

### Market Worker durability checks

```powershell
$env:TM_TRADING_TEST_MONGODB_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
cargo test -p tm-trading-storage --test mongo_candles
```

A multi-candle batch runs in one transaction. The test forces the second candle of a batch to
fail and asserts the first is rolled back. Transactions require the MongoDB deployment to run as
a replica set; a standalone `mongod` cannot accept the batch path.

### Market Worker failure handling

- Exactly one selected provider is probed before the worker reports capability; every probed instance's health is recorded.
- A provider failure falls back to the next candidate in the bounded chain; the chain never crosses provider definitions.
- A storage failure ends the cycle immediately; it does not fall back to another provider.
- A cycle that exceeds `TM_TRADING_MARKET_CYCLE_TIMEOUT_MS` is cancelled and reported as a timeout.
- An Agent shutdown aborts an in-flight cycle and reports `drained: false`; pending operations also prevent a drained result.

## Run the Research Worker

The Research Worker is opt-in and runs under the Agent. It reads only persisted closed candles, so enable the Market Worker first and wait for fresh `1m` bars. The database URI is passed only to workers that need domain storage.

```powershell
cargo build -p tm-trading-agent -p tm-trading-sample-worker -p tm-trading-market-worker -p tm-trading-research-worker

$env:TM_TRADING_RESEARCH_WORKER_ENABLED = "1"
$env:TM_TRADING_RESEARCH_OWNER_SCOPE = "local-operator"
$env:TM_TRADING_RESEARCH_SYMBOLS = "BTCUSDT,ETHUSDT"
$env:TM_TRADING_RESEARCH_TIMEFRAME = "1m"
$env:TM_TRADING_RESEARCH_SOURCE_OF_TRUTH = "1"
$env:TM_DATABASE_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
$env:TM_DATABASE_NAME = "tm-trading-rust"
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```

Expected result:

- `GET /api/v1/services` lists `research-worker` alongside the other enabled workers;
- `research-worker` stays `NOT_READY` until one bounded scan completes over fresh closed candles;
- a successful scan upserts deterministic, owner-scoped VSA events into `research_events` with source, input, producer, schema, and quality provenance;
- replaying the same window does not create duplicate events.

With `TM_TRADING_RESEARCH_SOURCE_OF_TRUTH` unset or `0`, the worker exits with code `78` before opening MongoDB. That is the dual-writer guard, not a runtime failure to retry.

### Research Worker failure handling

- A missing candle window is reported as skipped, never as successful.
- A stale or not-yet-closed candle fails the cycle and revokes readiness before any event is written.
- A scan that exceeds `TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS` is cancelled and reported as a timeout.
- A storage failure ends the cycle and revokes readiness.
- An Agent shutdown aborts an in-flight scan and reports `drained: false`.

## Run the News Worker

The News Worker is opt-in and runs under the Agent. It needs at least one enabled, license-verified RSS source in `news_sources`; seeding that registry is operator work, and the worker never invents a source. Sources whose `pollIntervalSec` has not elapsed are skipped until they are due.

```powershell
cargo build -p tm-trading-agent -p tm-trading-sample-worker -p tm-trading-news-worker

$env:TM_TRADING_NEWS_WORKER_ENABLED = "1"
$env:TM_TRADING_NEWS_SOURCE_OF_TRUTH = "1"
$env:TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE = "40"
$env:TM_DATABASE_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
$env:TM_DATABASE_NAME = "tm-trading-rust"
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```

Expected result:

- `GET /api/v1/services` lists `news-worker` alongside the other enabled workers;
- `news-worker` stays `NOT_READY` until one cycle polls at least one due, allowed source successfully;
- a successful poll upserts normalized items into `news_items` with canonical and origin URLs, distinct observation times, and a rights snapshot;
- each poll writes only that source's health fields in `news_sources`.

With `TM_TRADING_NEWS_SOURCE_OF_TRUTH` unset or `0`, the worker exits with code `78` before opening MongoDB. That is the dual-writer guard, not a runtime failure to retry.

### News Worker failure handling

- No due or eligible source is reported as skipped, never as successful.
- A cycle in which every polled source failed is a failure and revokes readiness.
- A single source failure records a sanitized code (`timeout`, `http_error`, `rate_limited`, `permission_denied`, `invalid_feed`, `connection_error`, `unavailable`) and does not fail the other sources.
- A feed larger than 2 MB, a redirect, a private or metadata address, or a non-feed document is rejected before storage.
- A cycle that exceeds `TM_TRADING_NEWS_CYCLE_TIMEOUT_MS` is cancelled and reported as a timeout.
- An Agent shutdown aborts an in-flight cycle and reports `drained: false`.

## Run the AI Worker

The AI Worker is opt-in and runs under the Agent. It requires MongoDB, an enabled
`research_explanation` route, its enabled provider and model, and the provider credential
named by the profile. The credential namespace is forwarded to this worker only. AI job
completion requires MongoDB to run as a replica set (including a single-node replica set)
or a sharded cluster reached through `mongos`; a standalone `mongod` cannot execute the
briefing/job transaction.

```powershell
cargo build -p tm-trading-agent -p tm-trading-ai-worker

$env:TM_TRADING_AI_WORKER_ENABLED = "1"
$env:TM_DATABASE_URI = "mongodb://localhost:27018/?tls=true&tlsCAFile=D:/certs/ca.pem"
$env:TM_DATABASE_NAME = "tm-trading-rust"
$env:TM_TRADING_AI_SECRET_PRIMARY = "<provider-secret>"
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```

In this example, the configured provider profile names
`TM_TRADING_AI_SECRET_PRIMARY`; the secret value is never stored in the profile,
briefing, job, logs, or Desktop state.

Expected result:

- `GET /api/v1/services` lists `ai-worker` alongside the other enabled workers;
- `ai-worker` becomes `READY` only after transaction-capable MongoDB and the route, provider, model, and credential resolve;
- a request from the Desktop or `POST /api/v1/ai/jobs` moves from `QUEUED` to `CLAIMED` and then to a terminal status;
- a successful job atomically publishes its briefing and changes to `SUCCEEDED`, so the returned briefing ID is readable;
- a failed request records a sanitized error code and never publishes a partial briefing;
- a lease that lapses during execution becomes `OUTCOME_UNKNOWN`, not `SUCCEEDED` or `FAILED`.

With `TM_TRADING_AI_WORKER_ENABLED` unset or `0`, the worker exits with code `78` before opening MongoDB. The Agent does not register the worker unless that flag is enabled.

## Market read model
The Management API serves the closed candles written by the Market Worker:

```powershell
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/market/overview?symbols=BTCUSDT,ETHUSDT&timeframe=1m&bars=60"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/market/pulse?symbols=BTCUSDT,ETHUSDT&timeframe=1m&bars=60"
```

- `symbols` is optional; without it the caller's default watchlist is used. At most 20 symbols and 500 bars are accepted.
- A symbol is `EMPTY` with no closed bars, `PARTIAL` with fewer than the requested bars, `STALE` when its newest close is older than half a timeframe (minimum 60 seconds), and `OK` otherwise.
- The panel state is the worst symbol state, so missing data is never shown as current.
- Both endpoints require `trading.market.read`; the Agent credential is rejected with `403`.
- The Desktop Market Overview and Market Pulse panels call the same two endpoints and show the symbol state beside each row.

## Research event read model

The Management API serves the events written by the Research Worker:

```powershell
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/research/events?symbol=BTCUSDT&timeframe=1m&limit=50"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/research/events/rev1_..."
```

- `symbol`, `timeframe`, `event_type`, and `since` are optional filters; `limit` is 1-200 and defaults to 50.
- The owner scope is never a request parameter: it is the authenticated subject, and a foreign event ID is `404` rather than `403`.
- Both endpoints require the exact `trading.research.read` capability; the Agent credential is rejected with `403`.
- `freshness` and `age_seconds` are computed at read time from `detected_at`: an event is `FRESH` up to half the timeframe, with a one-minute floor. An unknown timeframe fails closed as `STALE`.
- Each event returns the full evidence: source, provider method, input snapshot, producer and schema versions, quality state and reasons, evidence values, context, and the observation, detection, and ingestion times.

The Desktop Signal Center panel reads the list every refresh cycle and shows one row per event with asset, event type, direction label, score, timeframe, context, quality, detected time, source, and freshness, plus an event and fresh count in the panel header. It has independent loading, empty, error, and stale states, so an API failure shows its own banner and keeps the last loaded rows instead of blanking the Dashboard.

Selecting an asset opens the research-event detail Slideover, which fetches that event by ID and shows:

- the event, direction, quality, freshness, score, age, and detection time;
- full provenance: event ID, owner scope, method and version, source, provider method, input snapshot, producer, schema, observed time, and ingested time;
- the numeric evidence and string context maps;
- the quality reasons.

The Slideover declares `role="dialog"`, `aria-modal`, and a labelled title, focuses its close button, and closes with that button, the backdrop, or `Escape`. Only one overlay is open at a time, and the detail request is re-issued by the same refresh cycle while it stays open, so an aging observation re-renders as `STALE` instead of keeping an old freshness label.

If the list request fails, the panel keeps the error banner and its last good rows. If the detail request fails, the Slideover shows a localized message with the sanitized API error instead of inventing event values.

## News read model

The Management API serves the items the News Worker stored:

```powershell
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/news?market_group=crypto&limit=50"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/news/news_..."
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/news/sources"
```

- `source_id`, `market_group`, `level`, and `since` are optional filters; `limit` is 1-200 and defaults to 50.
- Only items whose persisted rights snapshot allows display are returned, from both the list and the detail read; a story that cannot be displayed answers `404` rather than leaking its metadata.
- `freshness` and `age_seconds` are computed at read time from `ingested_at`; an item ingested more than 3,600 seconds ago is `STALE`. The list also reports `stale_after_seconds`.
- `url` is the citation link exactly as the publisher exposed it, `published_at` is `null` when the feed carried no usable date, and `first_seen_at` and `ingested_at` are preserved separately.
- The sources read returns each source's transport, trust tier, configured market groups and languages, poll interval, and sanitized health: state, last fetch/ok/item times, the bounded error code, and the consecutive error count. A raw provider error body is never returned.
- All three endpoints require the exact `trading.news.read` capability; the Agent credential is rejected with `403`.

The Desktop News & Events panel reads both lists every refresh cycle and shows one row per story with headline, publisher, publication and ingestion times, market group, topics, and freshness, plus a story, fresh, and source count in the panel header. It has independent loading, empty, error, and stale states for the story list and the source registry, so an API failure keeps the last loaded rows behind its own banner.

Selecting a headline opens the news-story detail Slideover, which fetches that story by ID and shows its excerpt, level, direction, and freshness, a link to the original article, and full provenance: news ID, source, language, market group, event type, publication, first-seen and ingestion times, and the article URL. Opening a news story closes an open research-event Slideover and the reverse, so only one overlay is open at a time.

## AI briefing and job request API

The Management API serves evidence-bound briefing records and the durable request source
used by the AI Worker:

```powershell
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/ai/briefings?task=market_briefing&limit=50"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/ai/briefings/aibr1_..."
curl -X POST `
  -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  -H "Content-Type: application/json" `
  --data '{"task":"research_explanation","research_event_ids":["rev1_0123456789abcdef"],"news_item_ids":[]}' `
  "http://127.0.0.1:8787/api/v1/ai/jobs"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/ai/jobs?task=research_explanation&limit=50"
curl -H "Authorization: Bearer $env:TM_TRADING_LOCAL_API_TOKEN" `
  "http://127.0.0.1:8787/api/v1/ai/jobs/aijob1_..."
```

- `task` is an optional exact AI task filter; `limit` is 1-200 and defaults to 50. Malformed limits return the typed `400` error contract, while unsupported tasks and out-of-range limits return `422`.
- The owner scope is always the authenticated subject. An unknown or foreign briefing or job ID is `404`, so the API does not confirm that another owner's record exists.
- Briefing reads require the exact `trading.ai.read` capability. Job reads use the same capability; queueing a job requires `trading.ai.write`. The Agent credential is rejected with `403`, and an anonymous request is `401`.
- Each briefing response separates facts, model interpretations, and uncertainties and includes typed evidence references, input snapshot IDs, missing/stale disclosures, provider/model/prompt provenance, producer/schema stamps, and the UTC generation time.
- A job request must name at least one valid research-event or news identifier and at most 50 combined inputs. Only `research_explanation` can be queued today. The owner scope is never accepted from the request body.
- The AI job request schema is closed: malformed JSON returns `400`; a syntactically valid but contract-invalid body, including one with any unknown field such as `owner_scope`, returns `422`.
- Job list and detail reads trigger due-job expiry, so a disabled AI Worker does not leave queued jobs permanently visible as runnable. MongoDB evaluates expiry and terminal lease predicates with the database clock.
- A new job returns `201`; an identical replay returns `200` with the stored job. Reusing an idempotency key with a different request returns `409`. Without an explicit key, the request fingerprint is the replay key. The job carries the server-generated correlation ID used to follow the request through execution.
- The Desktop AI Briefing panel keeps its own loading, empty, and error state and does not blank the Dashboard when this API call fails. It selects one owner-scoped research event, queues the request without an owner field, and shows the latest job through `QUEUED`, `CLAIMED`, `SUCCEEDED`, `FAILED`, `EXPIRED`, or `OUTCOME_UNKNOWN`. A successful job exposes its briefing in the panel. Its detail Slideover shows the complete evidence and provenance and ignores a late response for a briefing that is no longer selected.

Briefings are stored in `ai_briefings` with camelCase fields and the `{ ownerScope, generatedAt }` read index. Publication is insert-once: replaying an identifier preserves the first published content. Jobs are stored in `ai_jobs`; one document is both the owner-scoped request and the work item the worker claims. The MongoDB repository publishes a briefing and completes its job in one transaction, so a successful job always has a readable briefing and a conflicting publication cannot complete the job.

## Logs and status

- Agent logs use `tracing`; set `TM_LOG_FORMAT=json` for structured JSON output.
- The worker protocol is written only to the Agent's stdout pipe. Worker diagnostic logs use stderr and are retained in a bounded 100-line tail.
- Service state is available from the Management API at `GET /api/v1/services` and from the Desktop Control Center. The Desktop shows the last successful refresh time and turns the connection indicator to `Disconnected` when a refresh fails; it keeps the last known rows visible instead of blanking them.

## Failure handling

- An unexpected worker exit is recorded as `FAILED` and consumes the bounded restart budget; the count survives Agent restart.
- A stop that exceeds the 15-second grace period is escalated to a forced kill and reports `drained: false`.
- A stale heartbeat makes a live worker `NOT_READY`; it does not make the worker `READY`. Heartbeat freshness is evaluated at Agent receipt time.
- Protocol schema, instance-ID, PID, executable-path, or executable-version mismatches are rejected, retained in the bounded log tail, and quarantined from automatic retry. The retry block is persisted in the local checkpoint.
- An automatic replacement uses the same bounded startup deadline as initial startup; a replacement that never becomes ready fails and consumes the bounded restart budget.
- An explicit stop cancels a pending automatic restart. Agent shutdown preserves desired `RUNNING`, pending backoff, and restart budget.
- Each polling pass handles at most 128 worker events from one worker, leaving remaining events for a later pass.
- Windows workers run in a kill-on-close Job Object. Unix workers run in a process group and explicit forced termination targets that group; descendant cleanup after an abrupt Agent loss remains unverified on a Unix runtime.
- The Management API never spawns a process. A command that cannot be executed is reported as a failed operation with a redacted error; an unknown operation, an expired lease, or a fencing-token mismatch is reported as a conflict and never as a success.
- A missing or invalid bearer credential is rejected with `401`; an Agent credential used on a user endpoint, or a user credential used on an Agent endpoint, is rejected with `403`. An operation without an `Idempotency-Key` header is rejected with `400`.
- The service read model is rebuilt from Agent heartbeats. When no Agent heartbeat has been received, the service list is empty rather than assumed healthy.

## Rollback

This slice adds new executables only. Rollback is to stop the Agent, remove the new Agent/sample-worker/Management API/Desktop artifacts, and roll back the corresponding Git commit. It does not modify legacy services, MongoDB collections, indexes, or documents.
