use std::fs;
use std::path::PathBuf;

use tm_trading_agent::Agent;
use tm_trading_supervisor_core::WorkerRegistry;

#[tokio::test]
async fn agent_shutdown_preserves_desired_running_state_for_the_next_launch() {
    let worker_executable = PathBuf::from(env!("CARGO_BIN_EXE_tm-trading-sample-worker"));
    let worker_dir = worker_executable.parent().unwrap().to_path_buf();
    let state_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/agent-integration-tests")
        .join(format!("agent-shutdown-{}", std::process::id()));
    fs::create_dir_all(&state_dir).unwrap();

    let mut agent = Agent::new(
        "local-agent",
        &worker_dir,
        &state_dir,
        WorkerRegistry::sample(),
    )
    .unwrap();
    agent.start_service("sample-worker").await.unwrap();
    let first_instance = agent
        .service_state("sample-worker")
        .unwrap()
        .service_instance_id
        .clone();

    agent.shutdown("integration test").await.unwrap();
    drop(agent);

    let checkpoint: serde_json::Value =
        serde_json::from_slice(&fs::read(state_dir.join("agent-state.json")).unwrap()).unwrap();
    assert_eq!(checkpoint["services"][0]["desiredState"], "RUNNING");
    assert_eq!(checkpoint["services"][0]["restartCount"], 0);

    let mut replacement = Agent::new(
        "local-agent",
        &worker_dir,
        &state_dir,
        WorkerRegistry::sample(),
    )
    .unwrap();
    replacement.poll_once().await.unwrap();
    let replacement_state = replacement.service_state("sample-worker").unwrap();
    assert_ne!(replacement_state.service_instance_id, first_instance);
    assert_eq!(replacement_state.desired_state.as_str(), "RUNNING");
    assert_eq!(replacement_state.observed_state.as_str(), "RUNNING");
    assert_eq!(replacement_state.readiness.as_str(), "READY");

    replacement
        .shutdown("integration test complete")
        .await
        .unwrap();
    drop(replacement);
    let _ = fs::remove_dir_all(state_dir);
}
