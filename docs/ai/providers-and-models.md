# AI Providers and Models

> Status: the AI provider profile registry, generic OpenAI-compatible health adapter, model registry, task routing, and evidence-bound briefing contract/API/UI are implemented (2026-11-22). Bounded generation, task-route fallback execution, and the AI Worker are not implemented yet.

## 1. Scope

AI providers are configuration, not execution. A profile records where an OpenAI-compatible Chat Completions endpoint lives, which model is the default, and how long one request may take. The platform never places orders or reads balances through a provider, and no provider response can trigger a trading action.

Profiles carry no secret. `api_key_env` is the *name* of an environment variable; the secret value is read from the environment of the Management API process at request time, used for one HTTP request, and never stored, returned, logged, or attached to an error.

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
| `api_key_env` | Environment variable name holding the API key |
| `health_state` | `UNKNOWN`, `HEALTHY`, or `UNHEALTHY` |
| `last_health_check_at` | Observation time of the last health test |
| `last_error_code` | Sanitized diagnostic code from the last failed test |

The domain normalizes the display name, base URL, model, and environment variable name, and rejects an empty identifier, model, or key environment name. Invalid configurations are rejected before storage with `422`.

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

## 5. Endpoints and capabilities

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

## 6. Model registry

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

## 7. Task routing

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

Routes live in the `ai_task_routes` collection with an `updatedAt` index. The Desktop AI Provider Center lists each task, its primary and fallback models, and how many providers the route can reach. The AI Worker still owns request execution and the fallback decision, so no route can trigger an AI request by itself.

## 8. Evidence-bound briefings

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

Briefing identifiers use `aibr1_` plus 16 lowercase hexadecimal characters. The identifier covers the complete content and evidence payload, so two distinct briefings generated in the same millisecond cannot overwrite each other.

Briefings live in the new `ai_briefings` collection with an `{ ownerScope, generatedAt }` read index. Publication is insert-once: replaying an identifier preserves the first published content.

| Method | Path | Capability |
|---|---|---|
| `GET` | `/api/v1/ai/briefings` | `trading.ai.read` |
| `GET` | `/api/v1/ai/briefings/{briefing_id}` | `trading.ai.read` |

The list accepts an exact task filter and a limit from 1 to 200, defaulting to 50. A malformed limit returns the typed `400` error contract; an unsupported task or an out-of-range limit returns `422`. Owner scope always comes from the authenticated subject. An unknown or foreign briefing identifier returns `404`, an Agent credential returns `403`, and an anonymous request returns `401`.

The Desktop AI Briefing panel reads the owner-scoped list and shows the task, summary, model/provider, evidence counts, and generation time. Its detail Slideover shows the separated claims, source references, input snapshots, missing/stale disclosures, and complete provenance in EN and VI.

## 9. Pending

- the AI Worker that executes a route, applies bounded fallback, and publishes briefings;
- dedicated OpenAI, Anthropic, Gemini, and OpenRouter adapters where protocol differences require them.
