# Operations Runbooks

> Status: Phase 2 local runtime runbook implemented and hardened on 2026-11-18<br>
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

## Run the Agent

```powershell
.\target\debug\tm-trading-agent.exe --worker-dir .\target\debug --state-dir .\target\tmp\agent-local
```

The Agent starts the registered sample worker and remains independent of any Desktop process. The state directory is installation-local: the Agent holds an exclusive `agent.lock` file for its lifetime and writes a minimal `agent-state.json` checkpoint with an `agent-state.json.bak` recovery copy. A second Agent using the same state directory fails closed.

Press `Ctrl+C` to request Agent shutdown; on Unix, `SIGTERM` requests the same shutdown. Shutdown requests are monotonic and cannot be cancelled by a later signal. The Agent then sends the explicit worker shutdown command and waits up to the configured grace period before escalating. After the worker process exits, the Agent collects the final protocol events for at most one second so the reported drain status reflects the worker's own final message. A forced kill is reaped for at most five seconds. Windows console close, logoff, and shutdown events use the same drain path.

Windows console hosting note: `Ctrl+C` is delivered only when the Agent is hosted by an existing console (Windows Terminal, `cmd.exe`, PowerShell). A dedicated console window — Explorer double-click or `cmd /c start` — is created with `CREATE_NEW_CONSOLE`, and Windows disables `Ctrl+C` for every process in that console until the application calls `SetConsoleCtrlHandler(NULL, FALSE)`. The Agent does not call that API because this workspace forbids unsafe code, so in a dedicated console window use the console close action, which uses the same drain path. Verified on Windows: the `CTRL_C_EVENT` is delivered by the operating system but not received by the application in a `CREATE_NEW_CONSOLE` console, and is received after the ignore attribute is cleared.

The worker is spawned detached from the Agent console on Windows, so a console close, logoff, or shutdown event reaches the Agent only. The Agent then drains the worker over the explicit control channel; the worker exits with code `0` instead of being terminated by the console event with `0xC000013A`.

Agent shutdown preserves the worker's desired `RUNNING` state, pending restart backoff, and restart budget. The next Agent launch reconciles that desired state. Only an explicit service stop persists desired `STOPPED` and cancels a pending automatic restart.

## Logs and status

- Agent logs use `tracing`; set `TM_LOG_FORMAT=json` for structured JSON output.
- The worker protocol is written only to the Agent's stdout pipe. Worker diagnostic logs use stderr and are retained in a bounded 100-line tail.
- The current CLI does not yet expose the service snapshot over HTTP.

## Failure handling

- An unexpected worker exit is recorded as `FAILED` and consumes the bounded restart budget; the count survives Agent restart.
- A stop that exceeds the 15-second grace period is escalated to a forced kill and reports `drained: false`.
- A stale heartbeat makes a live worker `NOT_READY`; it does not make the worker `READY`. Heartbeat freshness is evaluated at Agent receipt time.
- Protocol schema, instance-ID, PID, executable-path, or executable-version mismatches are rejected, retained in the bounded log tail, and quarantined from automatic retry. The retry block is persisted in the local checkpoint.
- An automatic replacement uses the same bounded startup deadline as initial startup; a replacement that never becomes ready fails and consumes the bounded restart budget.
- An explicit stop cancels a pending automatic restart. Agent shutdown preserves desired `RUNNING`, pending backoff, and restart budget.
- Each polling pass handles at most 128 worker events from one worker, leaving remaining events for a later pass.
- Windows workers run in a kill-on-close Job Object. Unix workers run in a process group and explicit forced termination targets that group; descendant cleanup after an abrupt Agent loss remains unverified on a Unix runtime.

## Rollback

This slice adds new executables only. Rollback is to stop the Agent, remove the new Agent/sample-worker artifacts, and roll back the corresponding Git commit. It does not modify legacy services, MongoDB collections, indexes, or documents.
