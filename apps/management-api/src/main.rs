use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use tm_trading_config::LogFormat;
use tm_trading_management_api::{AppState, AuthConfig, router};
use tm_trading_storage::{
    InMemoryOperationRepository, InMemoryProviderInstanceRepository,
    InMemoryServiceSnapshotRepository, InMemoryWatchlistRepository, MongoConnection,
    MongoOperationRepository, MongoProviderInstanceRepository, MongoServiceSnapshotRepository,
    MongoWatchlistRepository, OperationRepository, ProviderInstanceRepository,
    ServiceSnapshotRepository, WatchlistRepository,
};
use tokio::net::TcpListener;

const DEFAULT_BIND: &str = "127.0.0.1:8787";
const DEFAULT_USER_SUBJECT: &str = "local-operator";
const DEFAULT_AGENT_ID: &str = "local-agent";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("[management-api] {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let log_format = LogFormat::from_env()?;
    tm_trading_observability::init(log_format)?;

    let bind: SocketAddr = std::env::var("TM_TRADING_API_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND.to_owned())
        .parse()?;
    if !bind.ip().is_loopback() {
        return Err("TM_TRADING_API_BIND must be a loopback address in local mode".into());
    }

    let auth = AuthConfig {
        user_token: required_env("TM_TRADING_LOCAL_API_TOKEN")?,
        agent_token: required_env("TM_TRADING_AGENT_TOKEN")?,
        user_subject: std::env::var("TM_TRADING_LOCAL_API_SUBJECT")
            .unwrap_or_else(|_| DEFAULT_USER_SUBJECT.to_owned()),
        agent_id: std::env::var("TM_TRADING_AGENT_ID")
            .unwrap_or_else(|_| DEFAULT_AGENT_ID.to_owned()),
    };

    let (operations, services, watchlists, providers) = stores().await?;
    let state = AppState::with_repositories(auth, operations, services, watchlists, providers);
    let app = router(state);
    let listener = TcpListener::bind(bind).await?;
    tracing::info!(address = %bind, "management api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Builds the operation and service-snapshot stores selected by `TM_TRADING_STORE`.
///
/// Local development defaults to the in-memory stores so a developer does not need a
/// database. Production fails closed instead of losing durable operations on restart.
async fn stores() -> Result<
    (
        Arc<dyn OperationRepository>,
        Arc<dyn ServiceSnapshotRepository>,
        Arc<dyn WatchlistRepository>,
        Arc<dyn ProviderInstanceRepository>,
    ),
    Box<dyn std::error::Error>,
> {
    let store = std::env::var("TM_TRADING_STORE").unwrap_or_else(|_| "memory".to_owned());
    let environment = std::env::var("TM_ENVIRONMENT").unwrap_or_else(|_| "development".to_owned());
    match store.as_str() {
        "memory" if environment == "production" => {
            Err("TM_TRADING_STORE must be 'mongodb' when TM_ENVIRONMENT=production".into())
        }
        "memory" => Ok((
            Arc::new(InMemoryOperationRepository::new()),
            Arc::new(InMemoryServiceSnapshotRepository::new()),
            Arc::new(InMemoryWatchlistRepository::new()),
            Arc::new(InMemoryProviderInstanceRepository::new()),
        )),
        "mongodb" => {
            let uri = std::env::var("TM_DATABASE_URI")?;
            let database_name =
                std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| "tm-trading-rust".to_owned());
            let connection = MongoConnection::connect(&uri, &database_name).await?;
            Ok((
                Arc::new(MongoOperationRepository::new(connection.database()).await?),
                Arc::new(MongoServiceSnapshotRepository::new(connection.database()).await?),
                Arc::new(MongoWatchlistRepository::new(connection.database()).await?),
                Arc::new(MongoProviderInstanceRepository::new(connection.database()).await?),
            ))
        }
        other => {
            Err(format!("TM_TRADING_STORE must be 'mongodb' or 'memory', got '{other}'").into())
        }
    }
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value = std::env::var(name).map_err(|_| format!("{name} must be set"))?;
    if value.trim().is_empty() {
        return Err(format!("{name} must not be empty").into());
    }
    Ok(value)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    {
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };
        tokio::select! {
            () = ctrl_c => {},
            () = terminate => {},
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}
