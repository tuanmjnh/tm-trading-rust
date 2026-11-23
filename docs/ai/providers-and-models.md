# AI Providers and Models

> Status: the AI provider profile registry, generic OpenAI-compatible health/generation adapter, model registry, task routing, bounded route execution, owner-scoped evidence assembly with model-output validation, evidence-bound briefing contract/API/UI, the durable `ai_jobs` request queue, the supervised AI Worker, and the Desktop research-event request action are implemented (2026-11-22).

## 1. Scope

AI providers are configuration, not execution. A profile records where an OpenAI-compatible Chat Completions endpoint lives, which model is the default, and how long one request may take. The platform never places orders or reads balances through a provider, and no provider response can trigger a trading action.

Profiles carry no secret. `api_key_env` is the *name* of an environment variable in the dedicated `TM_TRADING_AI_` namespace; the secret value is read from the environment of the Management API process at request time, used for one HTTP request, and never stored, returned, logged, or attached to an error. Names outside that namespace are rejected, so a profile cannot read an unrelated process secret such as `PATH` or a platform token.

## 2. Profile contract

| Field | Meaning |
|---|---|
| `id` | Stable profile identifier |
| `kind` | `openai-compatible`, `openai`, `anthropic`, `gemini`, or `openrouter` |
| `display_name` | Operator-facing label, at most 80 characters |
| `enabled` | Operator enablement; a disabled profile is never tested or used |
| `base_url` | `https` endpoint, or `http` only for loopback |
| `default_model` | Model name passed to the provider |
| `timeout_ms` | Request timeout, 500 to 60000 ms |
| `api_key_env` | Environment variable name holding the API key; `TM_TRADING_AI_` followed by uppercase letters, digits, or underscores |
| `health_state` | `UNKNOWN`, `HEALTHY`, or `UNHEALTHY` |
| `last_health_check_at` | Observation time of the last health test |
| `last_error_code` | Sanitized diagnostic code from the last failed test |

The domain normalizes the display name, base URL, model, and environment variable name. It rejects an empty identifier, model, or key environment name and any key environment name outside the `TM_TRADING_AI_` uppercase namespace. The same rule is enforced when a stored profile is loaded, so a legacy profile that names an arbitrary process variable cannot be probed or used by the worker. Invalid configurations are rejected before storage with `422`.

## 3. Base URL safety

`validate_ai_provider_base_url` rejects:

- a non-`http`/`https` scheme;
- embedded credentials, a query string, or a fragment;
- `http` for any host that is not loopback;
- private, link-local, unspecified, broadcast, and metadata IP literals outside loopback.

Loopback `http` is allowed so a local runtime such as Ollama can be used without TLS. The HTTP client does not follow redirects, so a provider cannot redirect a health request to an internal address.

Known limit: hostnames are validated textually, not by resolved address, so a public hostname that resolves to a private address is not detected here. Upgrade to a resolving check if providers can be configured by untrusted operators.

## 4. Health test

`POST /api/v1/ai-providers/{id}/test` requires the exact `trading.ai.write` capability and:

1. loads the profile and returns `404` when it does not exist;
2. validates the stored base URL;
3. reads the API key from `api_key_env`, failing with `missing_api_key` when unset;
4. sends one bounded Chat Completions request with `max_tokens: 8` and `temperature: 0`;
5. records `HEALTHY` with a cleared error code, or `UNHEALTHY` with one sanitized code.

Failure codes: `invalid_base_url`, `forbidden_host`, `insecure_base_url`, `client_error`, `request_failed`, `timeout`, `auth_failed`, `not_found`, `provider_unavailable`, `http_error`, `invalid_response`, `response_too_large`, `missing_api_key`, and `unsupported_kind`. A response body is capped at 64 KiB.

`anthropic` and `gemini` profiles are stored but report `unsupported_kind` until a dedicated adapter exists; only the OpenAI-compatible protocol is implemented today.

A failed test is still a successful observation: the endpoint returns `200` with the recorded `UNHEALTHY` state instead of failing the request. Provider liveness and platform health stay separate.

## 5. Bounded generation

The OpenAI-compatible adapter also exposes one bounded Chat Completions generation operation. It accepts a system prompt, user prompt, output-token ceiling, and an explicit JSON-mode flag. The combined prompt is capped at 256 KiB, the response body at 256 KiB, temperature is fixed at zero, and redirects remain disabled. Provider usage is returned when the endpoint reports it.

Provider failures are classified as transient only for network failures, timeouts, HTTP 408/429, and HTTP 5xx. Authentication, configuration, invalid input, malformed output, and other 4xx responses are not transient and must not silently switch the task to another model.

## 6. Endpoints and capabilities

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai-providers` | `trading.ai.read` |
| `POST` | `/api/v1/ai-providers` | `trading.ai.write` |
| `GET` | `/api/v1/ai-providers/{id}` | `trading.ai.read` |
| `PATCH` | `/api/v1/ai-providers/{id}` | `trading.ai.write` |
| `DELETE` | `/api/v1/ai-providers/{id}` | `trading.ai.write` |
| `POST` | `/api/v1/ai-providers/{id}/test` | `trading.ai.write` |

All paths require a user credential; an Agent credential is always rejected. Profiles use the `ai_provider_profiles` collection in MongoDB and an in-memory repository for tests and the local default.

The Desktop AI Provider Center reads the list through `api-client`, shows each profile's kind, default model, health, last check, and last sanitized error, and offers a bounded Test action per enabled profile.

## 7. Model registry

A model belongs to exactly one provider profile. `provider_id` comes from the path, never from the body. Registering a model against an unknown provider fails with `422`, and deleting a provider also deletes its models, so no model can reference a missing provider.

| Field | Meaning |
|---|---|
| `id` | Stable model-registry identifier |
| `provider_id` | Owning AI provider profile |
| `model_id` | Model name sent to the provider |
| `display_name` | Operator-facing label, at most 80 characters |
| `context_window` | Optional context size, 1 to 2000000 |
| `max_output_tokens` | Optional output ceiling, 1 to 200000 |
| `supports_streaming` | Declared capability |
| `supports_json_mode` | Declared capability |
| `supports_tools` | Declared capability |
| `supports_vision` | Declared capability |
| `enabled` | Operator enablement |
| `health_state` | `UNKNOWN`, `HEALTHY`, or `UNHEALTHY` |
| `last_health_check_at` | Observation time of the last model test |
| `last_error_code` | Sanitized diagnostic code from the last failed test |

Capabilities are declared, not probed. A model is stored with `UNKNOWN` health until `POST /api/v1/ai-models/{id}/test` runs the same bounded OpenAI-compatible probe as the provider test, using the model's own `model_id` and the owning provider's base URL, timeout, and API-key environment variable. A successful probe records `HEALTHY` and clears the error code; a failed probe records `UNHEALTHY` plus the sanitized code. A model whose provider is missing returns `422`.

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai-models` | `trading.ai.read` |
| `POST` | `/api/v1/ai-providers/{id}/models` | `trading.ai.write` |
| `GET` | `/api/v1/ai-models/{id}` | `trading.ai.read` |
| `PATCH` | `/api/v1/ai-models/{id}` | `trading.ai.write` |
| `DELETE` | `/api/v1/ai-models/{id}` | `trading.ai.write` |
| `POST` | `/api/v1/ai-models/{id}/test` | `trading.ai.write` |

Models live in the `ai_models` collection with an `updatedAt` index and a `providerId` index. The Desktop AI Provider Center lists registered models under the provider table.

## 8. Task routing

Each AI task has one route: a primary model, an optional fallback chain, and the budget that bounds a request.

| Task | Purpose |
|---|---|
| `news_classification` | Classify a news item |
| `news_summary` | Summarize a news item |
| `market_briefing` | Brief the market view |
| `research_explanation` | Explain a research event |
| `daily_digest` | Build the daily digest |

A route names its models as `provider_id:model_registry_id`, for example `primary:primary-large`. Every entry must resolve to a registered model that belongs to the named provider profile; otherwise the route is rejected with `422`. The chain may not name the same provider twice, and it may not be longer than the attempt budget, so one failing provider is never tried twice.

| Field | Meaning |
|---|---|
| `task` | Task identifier |
| `primary` | First model to try |
| `fallbacks` | Ordered fallback models |
| `enabled` | Operator enablement |
| `timeout_ms` | Per-attempt timeout, 500 to 60000 ms |
| `max_attempts` | How many providers a request may try, 1 to 3 |
| `concurrency` | How many requests the task may run at once, 1 to 4 |
| `max_output_tokens` | Optional output ceiling, 1 to 200000 |
| `providers_in_reach` | How many providers the current settings can actually try |

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai-task-routes` | `trading.ai.read` |
| `POST` | `/api/v1/ai-task-routes` | `trading.ai.write` |
| `GET` | `/api/v1/ai-task-routes/{task}` | `trading.ai.read` |
| `PATCH` | `/api/v1/ai-task-routes/{task}` | `trading.ai.write` |
| `DELETE` | `/api/v1/ai-task-routes/{task}` | `trading.ai.write` |

Routes live in the `ai_task_routes` collection with an `updatedAt` index. The Desktop AI Provider Center lists each task, its primary and fallback models, and how many providers the route can reach. No route can trigger an AI request by itself.

The AI Worker crate's route executor owns request execution. It rejects disabled routes, providers, and models; checks that each model belongs to the named provider and declares JSON mode; caps output at the smallest of route, model, and request limits; applies the smaller of route and provider timeouts per attempt; bounds concurrency per task; and stops at the route attempt budget. Only a transient provider failure moves to the next configured fallback. Invalid configuration, missing credentials, permission failures, invalid requests, and malformed responses stop at the current provider.

## 9. Evidence-bound briefings

A briefing is an immutable, owner-scoped record of one model response. It separates reported facts, model interpretations, and uncertainty instead of presenting them as equivalent claims.

| Field | Meaning |
|---|---|
| `id` | Stable `aibr1_` identifier |
| `owner_scope` | Owner authenticated by the Management API |
| `task` | AI task that produced the briefing |
| `provider_id` / `model_registry_id` / `model_id` | Exact provider and model provenance |
| `prompt_version` | Version of the evidence prompt/assembly |
| `facts` | Statements reported from the supplied evidence |
| `interpretations` | Model inference, kept separate from facts |
| `uncertainties` | Explicit caveats and missing confirmation |
| `input_snapshot_ids` | Exact research input snapshots supplied to the model |
| `references` | Typed research-event identifiers, news-item identifiers, or source URLs |
| `missing_inputs` / `stale_inputs` | Explicit evidence disclosures |
| `generated_at` | UTC generation time |
| `producer_version` / `schema_version` | Record producer and schema provenance |

The domain rejects a briefing with no content or no evidence. Reference values must match their kind: `research_event` values use the exact `rev1_` identifier shape, `news_item` values use the exact `news_` identifier shape, and `source_url` values are HTTP(S) URLs without userinfo, query parameters, or fragments so token-bearing links cannot be persisted. Input snapshot identifiers use the exact `rsnap1_` identifier shape.

Briefing identifiers use `aibr1_` plus 16 lowercase hexadecimal characters. The identifier covers the complete content and evidence payload, including producer and schema stamps, so two distinct briefings generated in the same millisecond cannot overwrite each other.

Briefings live in the new `ai_briefings` collection with an `{ ownerScope, generatedAt }` read index. Publication is insert-once: replaying an identifier preserves the first published content.

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai/briefings` | `trading.ai.read` |
| `GET` | `/api/v1/ai/briefings/{briefing_id}` | `trading.ai.read` |

The list accepts an exact task filter and a limit from 1 to 200, defaulting to 50. A malformed limit returns the typed `400` error contract; an unsupported task or an out-of-range limit returns `422`. Owner scope always comes from the authenticated subject. An unknown or foreign briefing identifier returns `404`, an Agent credential returns `403`, and an anonymous request returns `401`.

The Desktop AI Briefing panel reads the owner-scoped list and shows the task, summary, model/provider, evidence counts, and generation time. Its detail Slideover shows the separated claims, source references, input snapshots, missing/stale disclosures, and complete provenance in EN and VI.

The AI Worker assembles evidence before any provider call. It accepts a bounded set of research-event and news identifiers, reads each record, and keeps only research events whose owner scope matches the request. A foreign event is disclosed exactly like a missing one, so the worker never confirms another owner's record exists. News is used only when its rights snapshot allows both display and summarization; a restricted item is disclosed instead. Evidence older than the configured age is still used but listed in `stale_inputs`, and every unusable input is listed in `missing_inputs`. Duplicate identifiers are collapsed, and a request with no usable input fails before a billable provider request.

Facts, references, input snapshots, disclosures, and provenance are server-owned. The model receives only the assembled facts plus the disclosure lines, and must answer with a JSON object that has only `interpretations` and `uncertainties`. Any other field, including model-authored facts or references, is rejected, so a hallucinated citation cannot enter a briefing. The system prompt, schema, and output bound are fixed by the producer version.

## 10. Durable AI job queue

A briefing is produced by the AI Worker from a durable request, not from an ad-hoc call.
One `ai_jobs` document is one owner-scoped request and is also the work item the worker
claims.

| Field | Meaning |
|---|---|
| `id` | Stable `aijob1_` identifier |
| `owner_scope` | Owner the request belongs to |
| `idempotency_key` / `request_hash` | Replay identity and request fingerprint |
| `correlation_id` | Server-generated end-to-end correlation handle |
| `task` | AI task to run |
| `research_event_ids` / `news_item_ids` | Requested evidence identifiers |
| `status` | `QUEUED`, `CLAIMED`, `SUCCEEDED`, `FAILED`, `EXPIRED`, or `OUTCOME_UNKNOWN` |
| `attempt` | How many times the job was claimed |
| `lease_owner` / `lease_expires_at` | Current claim holder and lease deadline |
| `fencing_token` | Increments on every claim; guards stale writers |
| `requested_at` / `expires_at` | Request time and bounded lifetime |
| `finished_at` | Terminal transition time |
| `briefing_id` | Briefing published by a successful job |
| `error_code` / `redacted_error` | Sanitized failure diagnostics |

A job is created once per `{ ownerScope, idempotencyKey }`. Replaying the same key with
the same request returns the stored job; replaying it with a different request is an
idempotency conflict. A claim is an atomic transition that increments the fencing token
and takes a bounded lease. Completion and failure require the current lease holder and
the current fencing token, so a worker that lost its lease cannot overwrite a newer
state.

A queued job that passes its expiry becomes `EXPIRED`. A claimed job whose lease lapses
becomes `OUTCOME_UNKNOWN`, never `FAILED`: an unobserved provider call is not evidence of
a failed or successful run. Worker claims and owner-scoped API job reads trigger expiry;
MongoDB evaluates the expiry predicates with the database clock.

The Management API is the owner-scoped request source:

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai/jobs` | `trading.ai.read` |
| `POST` | `/api/v1/ai/jobs` | `trading.ai.write` |
| `GET` | `/api/v1/ai/jobs/{job_id}` | `trading.ai.read` |

A request names one AI task and the exact research-event or news identifiers to use as
evidence. Only `research_explanation` can be queued today. The request must contain at
least one valid input and at most 50 combined inputs. The owner scope always comes from
the authenticated subject; it is never accepted from the request body.

Without an explicit idempotency key, the request fingerprint is the replay key, so an
identical request returns the stored job. A client-supplied key is limited to 200
characters. Replaying a key with the same request returns `200` and the stored job;
reusing it with a different request returns `409`. A new job returns `201` and expires
after one hour unless it is claimed sooner.

The list accepts an exact task filter and a limit from 1 to 200, defaulting to 50. A
malformed limit returns the typed `400` error contract; an unsupported task or an
out-of-range limit returns `422`. An unknown or foreign job identifier returns `404`, an
Agent credential returns `403`, and an anonymous request returns `401`.

The Desktop AI Briefing panel is the current job producer. It selects one research event
from the authenticated owner's research-event list, queues a `research_explanation` job
with that exact identifier, and never supplies owner scope. It polls the owner-scoped job
list and shows the latest queued, claimed, or terminal status in EN/VI. A successful job
exposes its published briefing in the same panel; failed, expired, and unknown-outcome
jobs remain visible instead of being reported as successful.

## 11. AI Worker

The AI Worker executable is a supervised process. It claims one durable job at a time,
assembles owner-scoped evidence, runs the configured route, validates the model output,
and asks storage to publish the briefing and complete the job in one MongoDB transaction.

| Variable | Meaning |
|---|---|
| `TM_TRADING_AI_WORKER_ENABLED` | `1`/`true` allows the worker to publish briefings; defaults off |
| `TM_TRADING_AI_CLAIM_MS` | Delay between two claim attempts, default `5000` |
| `TM_TRADING_AI_HEARTBEAT_MS` | Heartbeat interval, default `5000` |
| `TM_TRADING_AI_LEASE_MS` | Lease taken on a claimed job, default `300000` |

The Agent registers `ai-worker` in its static registry only when
`TM_TRADING_AI_WORKER_ENABLED=1`, and it forwards the `TM_TRADING_AI_` credential
namespace to that worker alone. The worker is not given a shell command or an arbitrary
executable.

Before reporting readiness the worker verifies that the `research_explanation` route, its
provider profile, and its model are enabled and that the provider credential resolves.
The probe never calls the provider, so readiness never costs a billable request.

A job failure is recorded on the job with a sanitized code (`no_usable_evidence`,
`invalid_request`, `evidence_too_large`, `invalid_model_output`, `provider_failed`,
`route_unavailable`, `publish_failed`, or `unsupported_task`). The raw provider response
is never stored.

## 12. Pending

- dedicated Anthropic and Gemini adapters. OpenAI and OpenRouter currently use the OpenAI-compatible adapter and need dedicated adapters only if their protocol differences require them.
