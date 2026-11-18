use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use tm_trading_agent_protocol::{SupervisorCommand, WorkerMessage};

use tm_trading_sample_worker::SampleWorker;

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_CYCLE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_WORKER_INTERVAL: Duration = Duration::from_secs(86_400);
const RUN_ONCE_DURATION: Duration = Duration::from_millis(10);

fn main() {
    if let Err(error) = run() {
        eprintln!("[sample-worker] fatal: {error}");
        std::process::exit(2);
    }
}

fn run() -> io::Result<()> {
    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("sample-worker@local-agent:{pid}"));
    let heartbeat_interval =
        duration_from_env("TM_SAMPLE_WORKER_HEARTBEAT_MS", DEFAULT_HEARTBEAT_INTERVAL)
            .max(Duration::from_millis(100))
            .min(MAX_WORKER_INTERVAL);
    let cycle_interval = duration_from_env("TM_SAMPLE_WORKER_CYCLE_MS", DEFAULT_CYCLE_INTERVAL)
        .min(MAX_WORKER_INTERVAL);
    let crash_after = duration_from_env("TM_SAMPLE_WORKER_CRASH_AFTER_MS", Duration::ZERO);
    let started_at = Instant::now();

    let (command_tx, command_rx) = mpsc::channel();
    thread::spawn(move || read_commands(command_tx));

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = SampleWorker::new(&instance_id);

    send(&mut output, &worker.hello(pid, Utc::now()))?;
    if !worker.verify_capability(Utc::now()) {
        send(
            &mut output,
            &WorkerMessage::Fatal {
                instance_id: instance_id.clone(),
                at: Utc::now(),
                error: "sample capability self-check failed".to_owned(),
            },
        )?;
        return Err(io::Error::other("sample capability self-check failed"));
    }
    send(&mut output, &worker.heartbeat(Utc::now()))?;
    send(&mut output, &worker.ready(Utc::now()))?;

    let mut next_heartbeat = Instant::now() + heartbeat_interval;
    let mut next_cycle = if cycle_interval.is_zero() {
        None
    } else {
        Some(Instant::now() + cycle_interval)
    };

    loop {
        match command_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(SupervisorCommand::Shutdown { reason, .. }) => {
                let at = Utc::now();
                send(&mut output, &worker.stopping(at, &reason))?;
                send(&mut output, &worker.stopped(Utc::now(), &reason, true))?;
                return Ok(());
            }
            Ok(SupervisorCommand::RunOnce { operation_id }) => {
                send(
                    &mut output,
                    &worker.run_once(
                        &operation_id,
                        Utc::now(),
                        u64::try_from(RUN_ONCE_DURATION.as_millis()).unwrap_or(u64::MAX),
                    ),
                )?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                send(
                    &mut output,
                    &WorkerMessage::Fatal {
                        instance_id,
                        at: Utc::now(),
                        error: "supervisor control channel closed".to_owned(),
                    },
                )?;
                return Ok(());
            }
        }

        let now = Instant::now();
        if now >= next_heartbeat {
            send(&mut output, &worker.heartbeat(Utc::now()))?;
            next_heartbeat = now + heartbeat_interval;
        }
        if next_cycle.is_some_and(|deadline| now >= deadline) {
            send(&mut output, &worker.cycle(Utc::now(), 0))?;
            next_cycle = Some(now + cycle_interval);
        }
        if !crash_after.is_zero() && started_at.elapsed() >= crash_after {
            send(
                &mut output,
                &WorkerMessage::Fatal {
                    instance_id,
                    at: Utc::now(),
                    error: "configured crash drill".to_owned(),
                },
            )?;
            std::process::exit(9);
        }
    }
}

fn read_commands(command_tx: mpsc::Sender<SupervisorCommand>) {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[sample-worker] failed to read control command: {error}");
                break;
            }
        };
        match serde_json::from_str::<SupervisorCommand>(&line) {
            Ok(command) => {
                if command_tx.send(command).is_err() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("[sample-worker] ignored invalid control command: {error}");
            }
        }
    }
}

fn duration_from_env(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn send(output: &mut impl Write, message: &WorkerMessage) -> io::Result<()> {
    let line = serde_json::to_string(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writeln!(output, "{line}")?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_intervals_are_bounded() {
        let heartbeat = Duration::from_millis(u64::MAX).min(MAX_WORKER_INTERVAL);
        let cycle = Duration::from_millis(u64::MAX).min(MAX_WORKER_INTERVAL);

        assert_eq!(heartbeat, MAX_WORKER_INTERVAL);
        assert_eq!(cycle, MAX_WORKER_INTERVAL);
    }
}
