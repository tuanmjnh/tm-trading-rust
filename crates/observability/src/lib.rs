use thiserror::Error;
use tracing::Span;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use tm_trading_config::LogFormat;

pub fn init(format: LogFormat) -> Result<(), ObservabilityError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match format {
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(ObservabilityError),
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_env_filter(filter)
            .try_init()
            .map_err(ObservabilityError),
    }
}

#[must_use]
pub fn correlation_span(correlation_id: Uuid) -> Span {
    tracing::info_span!("correlation", correlation_id = %correlation_id)
}

#[derive(Debug, Error)]
#[error("failed to initialize the tracing subscriber")]
pub struct ObservabilityError(#[source] Box<dyn std::error::Error + Send + Sync>);
