use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use tm_trading_agent::{Agent, AgentError};
use tm_trading_config::LogFormat;
use tm_trading_supervisor_core::WorkerRegistry;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
// The Agent and sample worker inherit the same workspace release version.
const SAMPLE_WORKER_EXECUTABLE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("[agent] {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let log_format = LogFormat::from_env()?;
    tm_trading_observability::init(log_format)?;

    let cli = Cli::parse()?;
    let worker_dir = cli.worker_dir.unwrap_or_else(default_worker_dir);
    let state_dir = cli
        .state_dir
        .unwrap_or_else(|| worker_dir.join("data").join("agent"));
    let mut agent = Agent::new(
        &cli.agent_id,
        worker_dir,
        state_dir,
        WorkerRegistry::sample_with_version(SAMPLE_WORKER_EXECUTABLE_VERSION),
    )?;
    agent.spawn_shutdown_listener();
    if let Some(crash_after_ms) = cli.crash_after_ms {
        agent.enable_crash_drill(crash_after_ms);
    }
    let start_result = if cli.self_test || cli.crash_after_ms.is_some() {
        agent.start_service(&cli.service_id).await.map(|_| ())
    } else {
        agent
            .start_service_for_launch(&cli.service_id)
            .await
            .map(|_| ())
    };
    if let Err(error) = start_result {
        if matches!(error, AgentError::ShutdownRequested) {
            agent.shutdown("agent shutdown").await?;
            return Ok(());
        }
        return Err(error.into());
    }

    if cli.self_test {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let outcome = agent.stop_service(&cli.service_id, "self-test").await?;
        let state = agent.service_state(&cli.service_id);
        let summary = serde_json::json!({
            "agent_id": agent.agent_id(),
            "service_definition_id": cli.service_id,
            "desired_state": state.map(|state| state.desired_state.as_str()),
            "observed_state": state.map(|state| state.observed_state.as_str()),
            "readiness": state.map(|state| state.readiness.as_str()),
            "drained": outcome.drained,
            "forced": outcome.forced,
            "exit_code": outcome.exit_code,
        });
        println!("{}", serde_json::to_string(&summary)?);
        return Ok(());
    }

    if let Some(crash_after_ms) = cli.crash_after_ms {
        run_crash_drill(&mut agent, &cli.service_id, crash_after_ms).await?;
        return Ok(());
    }

    agent.run_until_shutdown(DEFAULT_POLL_INTERVAL).await?;
    Ok(())
}

async fn run_crash_drill(
    agent: &mut Agent,
    service_id: &str,
    crash_after_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline_restart_count = agent
        .service_state(service_id)
        .map(|state| state.restart_count)
        .unwrap_or(0);
    let baseline_instance_id = agent
        .service_state(service_id)
        .map(|state| state.service_instance_id.clone());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        agent.poll_once().await?;
        let state = agent.service_state(service_id);
        let restarted = state.is_some_and(|state| {
            state.restart_count > baseline_restart_count
                && baseline_instance_id
                    .as_ref()
                    .is_none_or(|instance_id| *instance_id != state.service_instance_id)
        });
        let ready = state.is_some_and(|state| {
            state.observed_state.as_str() == "RUNNING" && state.readiness.as_str() == "READY"
        });
        if restarted && ready {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("crash drill did not observe a ready automatic restart".into());
        }
        tokio::time::sleep(DEFAULT_POLL_INTERVAL).await;
    }

    let outcome = agent
        .stop_service(service_id, "crash drill complete")
        .await?;
    let state = agent.service_state(service_id);
    let summary = serde_json::json!({
        "agent_id": agent.agent_id(),
        "service_definition_id": service_id,
        "crash_after_ms": crash_after_ms,
        "restart_count": state.map(|state| state.restart_count),
        "desired_state": state.map(|state| state.desired_state.as_str()),
        "observed_state": state.map(|state| state.observed_state.as_str()),
        "readiness": state.map(|state| state.readiness.as_str()),
        "drained": outcome.drained,
        "forced": outcome.forced,
        "exit_code": outcome.exit_code,
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

#[derive(Debug)]
struct Cli {
    agent_id: String,
    worker_dir: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    service_id: String,
    self_test: bool,
    crash_after_ms: Option<u64>,
}

impl Cli {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut agent_id = env::var("TM_AGENT_ID").unwrap_or_else(|_| "local-agent".to_owned());
        let mut worker_dir = env::var("TM_WORKER_DIR").ok().map(PathBuf::from);
        let mut state_dir = env::var("TM_AGENT_STATE_DIR").ok().map(PathBuf::from);
        let mut service_id = "sample-worker".to_owned();
        let mut self_test = false;
        let mut crash_after_ms = None;
        let mut arguments = env::args().skip(1);

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--agent-id" => {
                    agent_id = arguments
                        .next()
                        .ok_or("--agent-id requires a value")?
                        .to_owned();
                }
                "--worker-dir" => {
                    worker_dir = Some(PathBuf::from(
                        arguments.next().ok_or("--worker-dir requires a value")?,
                    ));
                }
                "--state-dir" => {
                    state_dir = Some(PathBuf::from(
                        arguments.next().ok_or("--state-dir requires a value")?,
                    ));
                }
                "--start" => {
                    service_id = arguments
                        .next()
                        .ok_or("--start requires a value")?
                        .to_owned();
                }
                "--self-test" => self_test = true,
                "--crash-after-ms" => {
                    let value = arguments
                        .next()
                        .ok_or("--crash-after-ms requires a value")?
                        .parse::<u64>()?;
                    crash_after_ms = Some(value);
                }
                "--help" | "-h" => {
                    println!(
                        "tm-trading-agent [--agent-id ID] [--worker-dir PATH] [--state-dir PATH] [--start SERVICE] [--self-test] [--crash-after-ms MS]"
                    );
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument: {argument}").into()),
            }
        }

        Ok(Self {
            agent_id,
            worker_dir,
            state_dir,
            service_id,
            self_test,
            crash_after_ms,
        })
    }
}

fn default_worker_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}
