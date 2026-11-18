# Service Catalog

> Status: Phase 2 sample-worker runtime implemented and hardened on 2026-11-18<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

## `sample-worker`

| Field | Value |
|---|---|
| Service definition ID | `sample-worker` |
| Executable | `tm-trading-sample-worker.exe` on Windows; `tm-trading-sample-worker` on other platforms |
| Process owner | The single Host Agent for the host |
| Purpose | Exercises process startup, readiness, heartbeat, restart, command, log, and graceful-shutdown paths without reading or writing market data |
| IPC schema | `worker.v2` |
| Control channel | One JSON object per line over the worker stdin/stdout pipes |
| Shutdown | The Agent sends an explicit `shutdown` command; the worker reports `stopping`, drains, reports `stopped`, and exits |

### State projection

The service keeps the three lifecycle dimensions separate:

- Desired state: `RUNNING` or `STOPPED`.
- Observed state: `STARTING`, `RUNNING`, `STOPPING`, `STOPPED`, `FAILED`, or `UNKNOWN`.
- Readiness: `READY`, `DEGRADED`, `NOT_READY`, or `UNKNOWN`.

A live process is not sufficient for `READY`. The sample worker must also report verified capability and a recent heartbeat.

### Readiness evidence

| Check | Required | Meaning |
|---|---:|---|
| `process-alive` | Yes | The Agent still owns a live child process |
| `capability-verified` | Yes | The worker completed its sample capability self-check |
| `recent-heartbeat` | Yes | A heartbeat was observed within 30 seconds |
| `recent-success` | No | A real, non-skipped cycle succeeded within 60 seconds; stale evidence degrades readiness |

Missing required evidence is `NOT_READY`, never an implicit success. A skipped cycle is not recorded as a successful cycle.

### Restart policy

An unexpected exit is `FAILED` and schedules an exponential restart with jitter. The default budget is five restart attempts, with a maximum backoff of 60 seconds. The restart count and pending schedule survive an Agent restart, so reconciliation does not reset the budget.

Protocol/schema, instance-identity, PID, executable-path, and executable-version failures are deterministic rejection causes. The Agent quarantines those workers as `FAILED`, blocks automatic retry, and persists the block in the local checkpoint. A deterministic failure must not be retried until the worker or its configuration is corrected; the local Agent API exposes an explicit retry-block clear for that operator action, but the Management API does not expose it yet.

### Ownership and local state

The Host Agent is the only process that may launch this worker. It uses a static service registry and clears the inherited child environment before adding an allowlist.

Each installation uses a dedicated Agent state directory. The Agent holds an exclusive `agent.lock` file for the lifetime of the process, so a second Agent fails closed instead of owning the same installation. The directory also contains `agent-state.json`, a minimal local checkpoint used only to reconcile desired state, process identity, restart scheduling, restart budget, and deterministic-failure quarantine after an Agent restart, plus `agent-state.json.bak` as the last known-good recovery copy. It is not an API contract, durable operation record, or service-snapshot repository.

An explicit service stop changes desired state to `STOPPED` and cancels a pending automatic restart. Agent-level shutdown changes only the observed process state: desired `RUNNING`, a pending restart backoff, and the restart budget are preserved so the next Agent launch can reconcile the service correctly.

The `worker.v2` handshake must match the spawned PID, canonical executable path, and executable version. Windows workers run inside a kill-on-close Job Object. Unix workers run inside a process group, and explicit forced termination targets that group; descendant cleanup after an abrupt Agent loss has not yet been verified on a Unix runtime.

### Current limitations

- The sample worker performs no market, research, news, AI, notification, or storage work.
- Its regular cycle is a deterministic readiness signal only.
- The Agent runtime is not yet connected to the Management API, durable operation repository, or service-snapshot persistence.
- Unix runtime shutdown and abrupt-loss descendant-containment drills require a Linux execution environment.
