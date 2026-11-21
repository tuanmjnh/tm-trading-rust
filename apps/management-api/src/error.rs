use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tm_trading_contracts::{ApiErrorBody, ApiErrorCode, ApiErrorResponse};
use tm_trading_domain::OperationError;
use tm_trading_storage::RepositoryError;
use uuid::Uuid;

/// Typed API failure with a stable machine-readable code.
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    UnprocessableEntity(String),
    Internal(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, ApiErrorCode, &str) {
        match self {
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, ApiErrorCode::BadRequest, message)
            }
            Self::Unauthorized(message) => (
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::Unauthorized,
                message,
            ),
            Self::Forbidden(message) => (StatusCode::FORBIDDEN, ApiErrorCode::Forbidden, message),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, ApiErrorCode::NotFound, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, ApiErrorCode::Conflict, message),
            Self::UnprocessableEntity(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiErrorCode::UnprocessableEntity,
                message,
            ),
            Self::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::Internal,
                message,
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        let body = ApiErrorResponse {
            error: ApiErrorBody {
                code,
                message: message.to_owned(),
                correlation_id: Uuid::new_v4().to_string(),
                details: None,
            },
        };
        (status, Json(body)).into_response()
    }
}

impl From<RepositoryError> for ApiError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::IdempotencyConflict { .. } => {
                Self::Conflict("idempotency key was reused with a different request".to_owned())
            }
            RepositoryError::OperationNotFound => {
                Self::NotFound("operation was not found".to_owned())
            }
            RepositoryError::InvalidLeaseDuration | RepositoryError::LeaseDurationOutOfRange => {
                Self::Internal("operation lease configuration is invalid".to_owned())
            }
            RepositoryError::Domain(OperationError::Expired) => {
                Self::Conflict("operation expired before it could be claimed".to_owned())
            }
            RepositoryError::Domain(
                OperationError::InvalidTransition { .. }
                | OperationError::WrongTargetAgent
                | OperationError::LeaseOwnershipMismatch
                | OperationError::NotClaimable,
            ) => Self::Conflict("operation lease is no longer valid".to_owned()),
            RepositoryError::Domain(error) => Self::BadRequest(error.to_string()),
            RepositoryError::LockPoisoned => {
                Self::Internal("operation repository lock was poisoned".to_owned())
            }
            RepositoryError::ConcurrentModification => {
                Self::Conflict("operation was modified concurrently".to_owned())
            }
            RepositoryError::CorruptRecord(_) => {
                Self::Internal("store returned an unreadable record".to_owned())
            }
            RepositoryError::EmptySnapshotField(_) | RepositoryError::SnapshotAgentMismatch => {
                Self::UnprocessableEntity(
                    "heartbeat contains an invalid service snapshot".to_owned(),
                )
            }
            RepositoryError::Candle(_) => {
                Self::UnprocessableEntity("candle failed validation".to_owned())
            }
            RepositoryError::DuplicateWatchlistName => {
                Self::Conflict("a watchlist with this name already exists".to_owned())
            }
            RepositoryError::WatchlistNotFound => {
                Self::NotFound("watchlist was not found".to_owned())
            }
            RepositoryError::Watchlist(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::ResearchEvent(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::ProviderInstanceNotFound => {
                Self::NotFound("provider instance was not found".to_owned())
            }
            RepositoryError::ProviderInstance(error) => {
                Self::UnprocessableEntity(error.to_string())
            }
            RepositoryError::NewsSourceNotFound => {
                Self::NotFound("news source was not found".to_owned())
            }
            RepositoryError::NewsSource(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::NewsItem(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::NewsCluster(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::AiProviderProfile(error) => {
                Self::UnprocessableEntity(error.to_string())
            }
            RepositoryError::AiProviderProfileNotFound => {
                Self::NotFound("ai provider profile was not found".to_owned())
            }
            RepositoryError::AiModelNotFound => Self::NotFound("ai model was not found".to_owned()),
            RepositoryError::AiModel(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::AiTaskRouteNotFound => {
                Self::NotFound("ai task route was not found".to_owned())
            }
            RepositoryError::AiTaskRoute(error) => Self::UnprocessableEntity(error.to_string()),
            RepositoryError::Mongo(_) => {
                Self::Internal("operation store is unavailable".to_owned())
            }
            RepositoryError::TlsRequired
            | RepositoryError::InsecureTls
            | RepositoryError::InvalidDatabaseName => {
                Self::Internal("operation store configuration is invalid".to_owned())
            }
        }
    }
}
