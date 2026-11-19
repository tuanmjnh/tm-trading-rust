//! Live MongoDB tests for the durable service snapshot projection.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set; see
//! `mongo_repository.rs` for the requirement. A throwaway database is created and
//! dropped per test.

use chrono::{DateTime, Utc};
use tm_trading_contracts::{DesiredStateDto, ObservedStateDto, ReadinessDto, ServiceSnapshot};
use tm_trading_storage::{
    MongoConnection, MongoServiceSnapshotRepository, RepositoryError, ServiceSnapshotRepository,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoServiceSnapshotRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoServiceSnapshotRepository::new(connection.database())
        .await
        .expect("create the snapshot repository");
    Some((connection, repository))
}

macro_rules! harness_or_skip {
    () => {
        match harness().await {
            Some(value) => value,
            None => {
                eprintln!("skipped: TM_TRADING_TEST_MONGODB_URI is not set");
                return;
            }
        }
    };
}

fn snapshot(agent_id: &str, instance: &str, observed: ObservedStateDto) -> ServiceSnapshot {
    let updated_at: DateTime<Utc> = "2026-11-19T00:00:00Z".parse().unwrap();
    ServiceSnapshot {
        service_instance_id: instance.to_owned(),
        service_definition_id: "sample-worker".to_owned(),
        agent_id: agent_id.to_owned(),
        desired_state: DesiredStateDto::Running,
        observed_state: observed,
        readiness: ReadinessDto::Ready,
        process_alive: Some(true),
        restart_count: 2,
        last_exit_code: Some(0),
        retry_blocked: false,
        last_heartbeat_at: Some(updated_at),
        updated_at,
    }
}

#[tokio::test]
async fn snapshots_survive_a_new_repository_instance() {
    let (connection, repository) = harness_or_skip!();
    repository
        .replace_agent_snapshots(
            "agent-1",
            vec![snapshot(
                "agent-1",
                "worker@agent-1",
                ObservedStateDto::Running,
            )],
        )
        .await
        .unwrap();

    let second_connection = MongoConnection::connect(&uri().unwrap(), connection.database().name())
        .await
        .expect("reconnect to the test MongoDB");
    let second_repository = MongoServiceSnapshotRepository::new(second_connection.database())
        .await
        .expect("create a second repository");
    let reloaded = second_repository.list().await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(
        reloaded,
        vec![snapshot(
            "agent-1",
            "worker@agent-1",
            ObservedStateDto::Running
        )]
    );
}

#[tokio::test]
async fn heartbeat_removes_instances_the_agent_no_longer_reports() {
    let (connection, repository) = harness_or_skip!();
    repository
        .replace_agent_snapshots(
            "agent-1",
            vec![
                snapshot("agent-1", "worker-a@agent-1", ObservedStateDto::Running),
                snapshot("agent-1", "worker-b@agent-1", ObservedStateDto::Running),
            ],
        )
        .await
        .unwrap();
    repository
        .replace_agent_snapshots(
            "agent-1",
            vec![snapshot(
                "agent-1",
                "worker-a@agent-1",
                ObservedStateDto::Stopped,
            )],
        )
        .await
        .unwrap();

    let stored = repository.list().await.unwrap();
    let other_agent = repository.get("worker-b@agent-1").await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].observed_state, ObservedStateDto::Stopped);
    assert!(other_agent.is_none());
}

#[tokio::test]
async fn one_agent_cannot_replace_another_agents_rows() {
    let (connection, repository) = harness_or_skip!();
    repository
        .replace_agent_snapshots(
            "agent-1",
            vec![snapshot(
                "agent-1",
                "worker@agent-1",
                ObservedStateDto::Running,
            )],
        )
        .await
        .unwrap();
    let result = repository
        .replace_agent_snapshots(
            "agent-2",
            vec![snapshot(
                "agent-1",
                "worker@agent-1",
                ObservedStateDto::Stopped,
            )],
        )
        .await;

    let stored = repository.get("worker@agent-1").await.unwrap();
    connection.database().drop().await.unwrap();

    assert!(matches!(
        result,
        Err(RepositoryError::SnapshotAgentMismatch)
    ));
    assert_eq!(
        stored.map(|snapshot| snapshot.observed_state),
        Some(ObservedStateDto::Running)
    );
}

#[tokio::test]
async fn an_empty_heartbeat_clears_that_agents_rows_only() {
    let (connection, repository) = harness_or_skip!();
    repository
        .replace_agent_snapshots(
            "agent-1",
            vec![snapshot(
                "agent-1",
                "worker@agent-1",
                ObservedStateDto::Running,
            )],
        )
        .await
        .unwrap();
    repository
        .replace_agent_snapshots(
            "agent-2",
            vec![snapshot(
                "agent-2",
                "worker@agent-2",
                ObservedStateDto::Running,
            )],
        )
        .await
        .unwrap();
    repository
        .replace_agent_snapshots("agent-1", Vec::new())
        .await
        .unwrap();

    let stored = repository.list().await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(
        stored
            .iter()
            .map(|snapshot| snapshot.service_instance_id.as_str())
            .collect::<Vec<_>>(),
        vec!["worker@agent-2"]
    );
}
