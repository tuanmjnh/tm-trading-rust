# TM Trading Rust — Repository Audit

- **Repository:** https://github.com/tuanmjnh/tm-trading-rust
- **Branch reviewed:** `main`
- **Reviewed commit:** `33a1f4fc5f8f8981811cc54e5e0244b5fa2ad0bd`
- **Review date:** 2026-11-24
- **Review type:** Static source/configuration review + GitHub Actions evidence review
- **Important limitation:** Source was inspected through GitHub. No local Windows build, app launch, MongoDB connection, Telegram delivery, provider health test, or live deployment was executed during this review.

## 1. Executive summary

The repository has made substantial progress and is no longer an architecture-only scaffold. It contains a Rust Cargo workspace with a Dioxus Desktop app, Axum Management API, host Agent/Supervisor, independent workers, MongoDB repositories, provider registries, AI job/briefing infrastructure, and a Telegram notification path.

The overall direction is coherent for a Desktop-first, service-oriented application. In particular, separating Desktop UI from the Agent/worker lifecycle, using durable operations and leases, distinguishing process state from readiness, and keeping Telegram delivery behind an outbox are strong decisions.

**Current assessment: good technical foundation, but not ready to call a complete/release-ready Desktop product yet.** The highest-priority gaps are the Windows distribution launcher, CI failures/security audit, incomplete configuration-management UI, backup restore completeness, and safe credential handling for any future remote Agent/API deployment.

### Snapshot

| Area | Assessment | Notes |
|---|---|---|
| Rust workspace and module boundaries | Good | 22 workspace packages; clear separation of domain, contracts, storage, API, Agent and workers |
| Desktop application | In progress | Useful dashboard/control panels exist; orchestration is concentrated in a large `main.rs` |
| Service lifecycle | Good foundation | Desired/observed/readiness states, worker protocol, leases, fencing and recovery are represented |
| Market/research/news workers | In progress | Worker implementations and storage contracts exist; live operation was not exercised here |
| AI providers/models | Partial | Registry and health-test APIs exist; Desktop mostly displays and tests, not full CRUD configuration |
| Telegram | Partial but real integration path | Client, outbox, delivery worker and test-send API exist; configuration is environment-based and has edge cases |
| MongoDB/storage | Good foundation | Durable repositories and indexes exist; MongoDB integration test job currently fails |
| Authentication/TM Hub | Local-only | Current API uses static local bearer tokens and grants the local user all current capabilities |
| Web/cloud deployment | Not implemented yet | Current Management API explicitly binds to loopback; roadmap describes later remote deployment |
| Windows packaging | Blocked | Packaged launcher does not start Agent/workers and has an Agent token variable mismatch |
| CI/security | Failing/incomplete | RustSec audit fails; live MongoDB AI job fails; secret scan is skipped after audit failure |

## 2. Findings by priority

## P0 — Fix before distributing a Windows Desktop package

### P0.1 — `start-all-services.cmd` does not actually start all services

**Files:** `scripts/package-desktop-windows.ps1`, `apps/management-api/src/main.rs`, `apps/agent/src/main.rs`

The packaging script creates a `start-all-services.cmd` that:

1. Loads `.env` when present.
2. Starts `tm-trading-management-api.exe`.
3. Waits two seconds.
4. Starts `tm-trading-desktop.exe`.
5. Exits.

It does **not** start `tm-trading-agent.exe`, so no worker fleet is started by this packaged entry point.

There is also a configuration mismatch:

- The generated `.env.example` names the Agent token `TM_TRADING_AGENT_API_TOKEN`.
- `apps/management-api/src/main.rs` requires `TM_TRADING_AGENT_TOKEN`.
- `apps/agent/src/main.rs` also reads `TM_TRADING_AGENT_TOKEN` when `--api-url` is used.
- The generated launcher only supplies a fallback for `TM_TRADING_LOCAL_API_TOKEN`; it does not supply the required Agent token.

Consequently, the packaged API can exit at startup when the required Agent token is absent, and the launcher still does not start the Agent/Workers even if the API starts.

`start-desktop-only.cmd` also does not load `.env`, unlike `start-all-services.cmd`, so a clean packaged install may launch Desktop without the API URL/token environment being loaded.

**Required fix:** create one canonical environment loader and use it from both launch scripts; correct the Agent variable name; generate secure per-install local credentials rather than ship fixed defaults; start API, Agent and Desktop in a controlled order; wait for health/readiness rather than a fixed delay; retain process handles/logs and provide a matching stop script. Add a packaging smoke test that expands the ZIP in a clean Windows environment and verifies API + Agent + workers + Desktop lifecycle.

## P1 — Fix before calling the system reliable or exposing it remotely

### P1.1 — CI currently has failing jobs

**Latest observed workflow:** [CI run #5](https://github.com/tuanmjnh/tm-trading-rust/actions/runs/45343792530)

At the time of review:

- Rust workspace: `cargo fmt` passed, Clippy passed, standard workspace tests passed; release build was still in progress and OpenAPI drift check had not run yet.
- Dependency audit/secret scan job: failed at RustSec audit; the secret-scan step was skipped.
- Live MongoDB AI job: failed.
- The overall workflow had not completed yet.

Prior run #4 also showed the workspace job passing formatting, Clippy, tests and release build but failing OpenAPI drift. Run #3 passed the workspace job including OpenAPI drift, while the live MongoDB and dependency/security jobs still failed. Check the newest run before treating any of these states as current after this audit date.

**Required fix:** do not merge/release based only on the green workspace steps. Make all required jobs pass, and run the secret scanner independently (or with an `always()` condition) so a RustSec failure cannot skip secret scanning.

### P1.2 — Two live MongoDB AI job tests fail because the test records violate a domain invariant

**Files:** `crates/storage/tests/mongo_ai_jobs.rs`, `crates/domain/src/ai_job.rs`

The live MongoDB job fails these two tests:

- `the_database_clock_expires_a_queued_job`
- `a_stale_worker_clock_cannot_claim_an_expired_job`

Both fail with `AiJob(InvalidExpiry)`.

The test changes the stored `expiresAt` to a time in the past while leaving `requestedAt` at the current time. `NormalizedAiJob::new` (also used by `AiJob::restore`) requires `expires_at > requested_at`. The mutated record is therefore invalid before the intended expiry/lease behaviour can be tested.

This points to a **test fixture/design issue**, not proof that newly-created production jobs are invalid: the normal API creates `expiresAt` after `requestedAt`.

**Required fix:** construct an already-old but valid job (`requestedAt < expiresAt < database now`) or use a controllable clock/test seam. Preserve the domain invariant; do not weaken validation merely to make the test green. Ensure the tests still prove that the database/server clock, rather than a stale worker-supplied clock, decides expiry.

### P1.3 — Dependency audit detects two advisories and several maintenance warnings

**Source:** latest observed RustSec audit in CI run #5; `Cargo.toml` pins `url = "=2.5.8"`.

Detected advisories:

| Advisory | Package | Locked version | Assessment |
|---|---|---:|---|
| RUSTSEC-2026-0592 | `url` | 2.5.8 | Stack overflow through deeply nested `blob:` URLs when `Url::origin()` is called on untrusted input. The advisory identifies 2.5.9 as fixed. This is a direct workspace pin and should be upgraded. |
| RUSTSEC-2026-0594 | `cookie` | 0.18.2 | Nonce reuse in a specific Unix process-fork/current-thread-runtime scenario. The advisory says normal Axum operation and Windows processes are not affected under the usual conditions; trace the dependency path and document applicability rather than blindly suppressing it. |

The audit also reported unmaintained dependencies including `chrono`, `chrono-tz`, `fxhash`, `paste` and `proc-macro-error`, plus unsoundness advisories for transitive `glib` and an older `rand` line. These are not all equivalent to exploitable vulnerabilities in this app; classify direct/transitive usage, target platform and affected API before replacing dependencies or adding a narrowly justified audit exception.

**Required fix:** update the direct `url` pin and lockfile; trace `cookie`/`glib`/`rand` dependency paths; remove or replace unnecessary unmaintained dependencies where practical; document any narrowly-scoped accepted advisory. Do not blanket-ignore the audit.

### P1.4 — Future remote Agent connections do not enforce HTTPS

**File:** `apps/agent/src/api.rs`

`ManagementApiClient::new` accepts an arbitrary `base_url`, and `post()` sends the Agent bearer token to it. There is no visible scheme guard requiring HTTPS for non-loopback destinations.

The Management API currently rejects non-loopback bind addresses, which limits the present local deployment. But the roadmap intends to connect cloud Agents over outbound HTTPS. If remote mode is introduced without a URL policy, an `http://` management URL could transmit the Agent bearer token in clear text.

**Required fix:** permit HTTP only for explicit loopback development addresses; require HTTPS for every remote endpoint; validate host/URL before creating the client; add tests for rejected remote HTTP and accepted local loopback HTTP. Apply the same policy to the Desktop API client when remote connections are introduced.

### P1.5 — Telegram custom base URL can send the bot token over plain HTTP

**File:** `crates/telegram-client/src/lib.rs`

The Telegram Bot API token is embedded in the request path (`/bot<TOKEN>/sendMessage`). The default host is HTTPS, but `base_url_override` currently only parses the URL; it does not reject a remote `http://` URL. A misconfigured custom endpoint could therefore receive the token over clear-text HTTP.

**Required fix:** require HTTPS outside loopback; permit HTTP only for loopback test servers; disable redirects or explicitly constrain redirect hosts so a credential-bearing path cannot be forwarded unexpectedly. Add tests for the URL policy.

### P1.6 — Backup restore does not restore all data that export includes

**Files:** `apps/management-api/src/system.rs`, `crates/contracts/src/backup.rs`

`BackupPayloadDto` exports:

- watchlists
- AI provider profiles
- AI task routes

However, `restore_backup` restores watchlists and AI task routes only. It does not restore the exported AI provider profiles and returns `ai_provider_profiles_restored: 0`. The archive payload also omits the AI model registry, even though task routes refer to registered provider/model entries.

This means a successful checksum verification and restore response does not recreate the full AI configuration needed to use the restored routes.

**Required fix:** define the complete backup scope explicitly. Include AI models if they are part of configuration state; restore providers before models and task routes; validate references before applying; make restore transactional or use a staged/rollback-safe procedure; return accurate per-category counts. Keep secrets out of the archive—restore secret references only, then require credentials to be provisioned separately.

## P2 — Product completeness, reliability and cloud readiness

### P2.1 — AI Provider Center is currently view/test-oriented, not a full configuration manager

**Files:** `apps/desktop/src/ai_providers.rs`, `apps/desktop/src/main.rs`, `crates/api-client/src/lib.rs`, `apps/management-api/src/ai_providers.rs`, `apps/management-api/src/ai_models.rs`, `apps/management-api/src/ai_task_routes.rs`

The Management API contains provider profile/model/task-route CRUD handlers. The Desktop panel currently renders provider profiles, models and routes, with health-test buttons. Its callbacks and API client expose listing and tests, but not create/update/delete configuration actions. The task routes are displayed as a table, not an editable routing form.

**Required fix:** add proper Modal/Slideover forms for provider profile CRUD, model CRUD, task-route primary/fallback editing, enable/disable, validation and health checks. Keep provider secrets out of normal DTOs and logs. A visible “configured” flag must not be mistaken for a successful connection test.

### P2.2 — Telegram setup is environment-only; the app does not provide a complete setup flow

**Files:** `apps/management-api/src/main.rs`, `apps/management-api/src/notifications.rs`, `workers/notification-worker/src/main.rs`, `apps/desktop/src/notifications.rs`

The code has a real Telegram delivery path: a secret-safe Telegram client, durable notification outbox, delivery worker, log history and a Desktop test-send panel. But the Bot token and default Chat ID are loaded from environment variables. The Desktop panel lets the user provide a test chat ID/message; it does not provide a full secure bot credential setup/edit flow.

The notification worker explicitly marks `WebPush` and `Electron` channels as unsupported in this release. Telegram is the supported delivery channel in the current implementation.

**Required fix:** provide a guided Telegram setup flow, credential-presence status, destination configuration, bounded test send, and safe rotation/removal. Store secrets in an OS credential store for local Desktop deployments or a server-side secret manager for cloud deployments. Be explicit that non-Telegram channels are not supported yet.

### P2.3 — Telegram truncation can panic on multibyte text

**File:** `crates/telegram-client/src/lib.rs`

`MAX_TELEGRAM_TEXT_CHARS` is named as a character limit, but the implementation uses `text.len()` (UTF-8 bytes) and slices `&text[..4000]`. If byte offset 4000 is inside a Vietnamese character or emoji, Rust string slicing can panic. This is particularly relevant because alert messages can contain Vietnamese text and emoji.

**Required fix:** truncate by Unicode scalar values or at a valid UTF-8 character boundary; add tests with Vietnamese and emoji content just across the limit. Consider splitting long Telegram messages instead of silently truncating them.

### P2.4 — Health endpoint always reports `HEALTHY`

**File:** `apps/management-api/src/routes.rs`

The current `health()` handler returns `HealthStatus::Healthy` unconditionally. It does not check MongoDB availability, Agent connectivity, worker freshness or storage readiness.

**Required fix:** distinguish liveness from readiness. Keep a cheap liveness endpoint for process availability, and add readiness diagnostics that verify the configured storage path and critical dependencies. A healthy API process must not imply that the system's workers/data pipeline are ready.

### P2.5 — Desktop refresh loop makes many sequential API calls every two seconds

**File:** `apps/desktop/src/main.rs`

The Desktop coroutine uses a two-second refresh interval and then performs a long sequence of reads: services, watchlists, market overview/pulse, research events/details, news/items/sources/clusters, AI briefings/jobs/providers/models/routes, notifications and diagnostics. Many are awaited sequentially.

This is simple for a single local user, but it can increase API/DB load and delay freshness as panels and datasets grow. It will be more problematic when Desktop and Web clients both connect to cloud services.

**Required fix:** refresh only active/visible panels; use differentiated intervals (fast service state, slower news/provider/AI lists); parallelize independent read calls with bounded concurrency; cancel stale requests; add SSE/WSS event updates with polling fallback; paginate and cache stable configuration lists.

### P2.6 — Current API authentication is local compatibility auth, not TM Hub/cloud auth

**Files:** `apps/management-api/src/auth.rs`, `apps/management-api/src/main.rs`, `docs/migration/tm-hub-integration-map.md`

Current `AuthContext` compares the bearer token to one configured local user token or one Agent token. The local user token is granted every current `trading.*` capability. There is no visible TM Hub JWT/JWKS verification in this API implementation. The API also rejects non-loopback bind addresses.

This can be acceptable for an isolated local-first milestone, but it is **not** a multi-user/cloud authentication design. The integration map itself describes future Hub token verification and capability mapping.

**Required fix before remote exposure:** integrate TM Hub-issued identity/permissions, validate signature/issuer/audience/expiry, map actual Hub capabilities, scope data by owner/tenant, use distinct per-Agent credentials with rotation/revocation, and keep the local static-token mode explicitly limited to loopback development. Do not expose this local mode on a public interface.

### P2.7 — Production storage configuration must fail closed explicitly

**File:** `apps/management-api/src/main.rs`

`TM_TRADING_STORE` defaults to `memory`, while `TM_ENVIRONMENT` defaults to `development`. Memory storage is rejected only when `TM_ENVIRONMENT=production`. A future container deployment with a missing/incorrect environment variable could start in a non-durable mode instead of failing clearly.

**Required fix:** make deployment mode explicit; require `TM_TRADING_STORE=mongodb` and all durable connection/configuration settings for any remote/production mode. Log the selected storage kind without leaking the URI. Do not let missing environment variables silently downgrade a production deployment to memory.

## 3. What is already implemented well

These are positive findings from source inspection, not claims of live runtime validation:

- Cargo workspace separates domain, contracts, storage, providers, Agent protocol, Supervisor core, API client, Desktop, Management API and workers.
- Agent owns the worker process lifecycle rather than the Dioxus UI. The worker protocol has handshake, heartbeat, readiness, cycle, operation result and graceful-stop messages.
- Worker state separates Desired, Observed and Readiness; persisted Agent state is reconciled on restart rather than treating an old PID as proof that a process is alive.
- Worker environment is filtered by responsibility. AI provider secret environment variables are forwarded only to the AI Worker; Telegram credentials are forwarded only to the Notification Worker.
- Service operations use durable repositories, idempotency keys, operation IDs, leases and fencing tokens.
- AI jobs are durable and owner-scoped; model routing validates references and limits attempts/timeouts/concurrency. The AI Worker stores evidence references and separates provider output from the research source of truth.
- Market, News, AI and Notification functions are separated into worker crates; not all loops are coupled to the Desktop UI.
- Telegram delivery is built around a durable outbox and delivery logs, with retry classification for network errors/429/5xx and non-retryable configuration errors.
- Provider adapters set request timeouts and response-size bounds, and validate provider URLs more strictly than the Telegram override currently does.
- The active product intent remains information-first/read-only; no order-entry path is needed for the current V5 scope.

## 4. Deployment and architecture status

### Desktop/local

The repo has a Dioxus Desktop executable, Management API, Agent and worker binaries. The Management API is explicitly local-only at present because it rejects non-loopback bind addresses. That is a sensible first security boundary, but the packaged launch scripts must be corrected before a clean end-user install can be considered functional.

### Cloud services

There is no completed remote Management API/Agent deployment path in the current source. The current Agent client sends outbound HTTP requests and should enforce HTTPS for non-loopback endpoints before remote deployment. The current API auth model is local static-token auth and must not be used as cloud multi-user auth.

### Web Dashboard

A Dioxus Web app is a roadmap target, not a current application in the workspace. The shared contracts/API-client foundation is a good start, but Web should be added only after the API/auth/Agent boundaries are ready for remote use.

### Recommended future split

- **Desktop/Web frontend:** static WebAssembly/web assets hosted independently (e.g. Cloudflare Pages or another static host).
- **Management API:** Axum on a persistent Rust-capable container/VM with durable MongoDB storage; do not deploy it as a short-lived edge function if it needs durable command handling/realtime connections.
- **Agents/Workers:** Linux VM or container host for persistent market/news/AI/notification workers. Free-tier services that sleep or scale to zero are suitable only for bounded/periodic tasks, not continuous feeds.
- **Database:** continue using MongoDB initially; choose a durable managed deployment and secure network path.
- **Telegram:** continue using the Notification Worker/outbox model; provision its token in a server-side secret store.

These are deployment recommendations, not verified deployments.

## 5. Recommended remediation order

1. Fix Windows package scripts and run a clean-install Windows smoke test: API starts, Agent authenticates, workers become READY, Desktop connects, graceful stop works.
2. Upgrade `url` to a fixed version; review `cookie` and the unmaintained/unsound transitive dependencies; separate secret scanning from dependency audit so it always runs.
3. Fix MongoDB AI job integration test fixtures without weakening `requestedAt < expiresAt` validation; make the live MongoDB workflow green.
4. Fix backup/restore completeness for providers, models and AI task routes; add restore tests for partial failure and accurate result counts.
5. Enforce HTTPS for all non-loopback Agent/API and Telegram endpoints; add security tests. Keep local HTTP strictly loopback-only.
6. Add complete AI Provider/Model/Task Route configuration UI and secure Telegram credential setup.
7. Fix UTF-8-safe Telegram message truncation and add Unicode tests.
8. Add real readiness reporting and reduce Desktop polling load.
9. Complete TM Hub authentication/permission integration before any remote/cloud mode.
10. Only then implement remote Agent deployment and the Dioxus Web client.

## 6. Verification checklist for the coding agent

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
cargo run -p tm-trading-management-api
cargo run -p tm-trading-agent -- --api-url http://127.0.0.1:8787 --start all
```

Additionally:

- Run the ignored MongoDB integration tests against a TLS replica set.
- Run dependency audit and secret scan as independent CI jobs.
- Run OpenAPI drift check.
- Test packaging from a clean Windows account/VM with no developer environment variables.
- Verify Telegram against a mock server for HTTP/HTTPS policy, 429 Retry-After, 5xx retry, invalid token, Unicode truncation and long-message behaviour.
- Verify AI Provider/Model/Task Route create, update, delete, health-test and fallback selection from the actual Desktop UI.
- Verify export → fresh install → restore, including every exported configuration category, with missing-secret re-provisioning.
- Verify local HTTP is loopback-only and remote Agent traffic refuses plain HTTP.
- Verify service READY only after dependency/evidence checks, not just PID/heartbeat.

Do not mark a check PASS unless the command/test was actually run and its output recorded.

## 7. Review conclusion

The project has a solid architecture foundation and the main Rust workspace is progressing well. However, the current repository should be treated as **development / pre-release**, not as a completed Desktop package or cloud-ready multi-client platform.

The first gate is not adding more workers or a Web app. It is getting a reliable Windows local install working end-to-end, making CI consistently green, closing credential transport issues, and completing the provider/Telegram configuration and backup/restore workflows. Once those are stable, the existing API/Agent separation gives a reasonable path to remote services and a future Web Dashboard.
