use std::time::Duration;

use reqwest::Client;
use thiserror::Error;
use tm_trading_contracts::{
    CreateOperationRequest, HealthResponse, OperationActionDto, OperationResponse, ServiceSnapshot,
};

/// Typed client for the versioned Management API contract.
#[derive(Debug, Clone)]
pub struct ManagementApiClient {
    http: Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Error)]
pub enum ApiClientError {
    #[error("failed to build the HTTP client: {0}")]
    Build(String),
    #[error("request to {path} failed: {message}")]
    Request { path: String, message: String },
    #[error("request to {path} returned {status}: {message}")]
    Status {
        path: String,
        status: u16,
        message: String,
    },
    #[error("response from {path} is not valid: {message}")]
    Decode { path: String, message: String },
}

impl ManagementApiClient {
    /// Builds a client for the given base URL and bearer credential.
    pub fn new(base_url: &str, token: &str) -> Result<Self, ApiClientError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| ApiClientError::Build(error.to_string()))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        })
    }

    pub async fn health(&self) -> Result<HealthResponse, ApiClientError> {
        let path = "/api/v1/health";
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| ApiClientError::Request {
                path: path.to_owned(),
                message: error.to_string(),
            })?;
        Self::decode(response, path).await
    }

    pub async fn list_services(&self) -> Result<Vec<ServiceSnapshot>, ApiClientError> {
        let path = "/api/v1/services";
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| ApiClientError::Request {
                path: path.to_owned(),
                message: error.to_string(),
            })?;
        Self::decode(response, path).await
    }

    /// Creates a durable operation for a service instance.
    pub async fn create_operation(
        &self,
        service_instance_id: &str,
        action: OperationActionDto,
        idempotency_key: &str,
    ) -> Result<OperationResponse, ApiClientError> {
        let path = format!("/api/v1/services/{service_instance_id}/operations");
        let body = CreateOperationRequest {
            service_instance_id: service_instance_id.to_owned(),
            action,
        };
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("Idempotency-Key", idempotency_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ApiClientError::Request {
                path: path.clone(),
                message: error.to_string(),
            })?;
        Self::decode(response, &path).await
    }

    pub async fn get_operation(
        &self,
        operation_id: &str,
    ) -> Result<OperationResponse, ApiClientError> {
        let path = format!("/api/v1/operations/{operation_id}");
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| ApiClientError::Request {
                path: path.clone(),
                message: error.to_string(),
            })?;
        Self::decode(response, &path).await
    }

    async fn decode<TResponse>(
        response: reqwest::Response,
        path: &str,
    ) -> Result<TResponse, ApiClientError>
    where
        TResponse: for<'de> serde::Deserialize<'de>,
    {
        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(ApiClientError::Status {
                path: path.to_owned(),
                status: status.as_u16(),
                message,
            });
        }
        response
            .json()
            .await
            .map_err(|error| ApiClientError::Decode {
                path: path.to_owned(),
                message: error.to_string(),
            })
    }
}

/// Generates an idempotency key for one user-initiated operation.
#[must_use]
pub fn new_idempotency_key() -> String {
    uuid::Uuid::new_v4().to_string()
}
