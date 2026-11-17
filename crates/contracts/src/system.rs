use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{API_VERSION, CONTRACT_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApiVersionInfo {
    pub api_version: String,
    pub contract_version: String,
    pub service_version: String,
}

impl ApiVersionInfo {
    #[must_use]
    pub fn current() -> Self {
        Self {
            api_version: API_VERSION.to_owned(),
            contract_version: CONTRACT_VERSION.to_owned(),
            service_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub version: ApiVersionInfo,
}
