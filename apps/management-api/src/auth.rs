use axum::extract::FromRequestParts;
use axum::http::{header, request::Parts};

use crate::error::ApiError;
use crate::state::AppState;

pub const CAPABILITY_SERVICE_READ: &str = "trading.service.read";
pub const CAPABILITY_SERVICE_CONTROL: &str = "trading.service.control";
pub const CAPABILITY_OPERATION_READ: &str = "trading.operation.read";
pub const CAPABILITY_WATCHLIST_READ: &str = "trading.watchlist.read";
pub const CAPABILITY_WATCHLIST_WRITE: &str = "trading.watchlist.write";
pub const CAPABILITY_PROVIDER_READ: &str = "trading.provider.read";
pub const CAPABILITY_PROVIDER_WRITE: &str = "trading.provider.write";
pub const CAPABILITY_MARKET_READ: &str = "trading.market.read";
pub const CAPABILITY_RESEARCH_READ: &str = "trading.research.read";

/// Identity resolved from the bearer token.
#[derive(Debug, Clone)]
pub enum Principal {
    User {
        subject: String,
        capabilities: Vec<String>,
    },
    Agent {
        agent_id: String,
    },
}

/// Authenticated caller.
#[derive(Debug, Clone)]
pub struct AuthContext {
    principal: Principal,
}

impl AuthContext {
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn subject(&self) -> &str {
        match &self.principal {
            Principal::User { subject, .. } => subject,
            Principal::Agent { agent_id } => agent_id,
        }
    }

    /// Requires an exact `trading.*` capability grant.
    pub fn require_capability(&self, capability: &str) -> Result<(), ApiError> {
        match &self.principal {
            Principal::User {
                subject,
                capabilities,
            } => {
                if capabilities.iter().any(|granted| granted == capability) {
                    Ok(())
                } else {
                    Err(ApiError::Forbidden(format!(
                        "principal '{subject}' does not have capability '{capability}'"
                    )))
                }
            }
            Principal::Agent { agent_id } => Err(ApiError::Forbidden(format!(
                "agent '{agent_id}' cannot use user capability '{capability}'"
            ))),
        }
    }

    /// Requires the caller to be the Agent identified by its installation credential.
    pub fn require_agent(&self, agent_id: &str) -> Result<(), ApiError> {
        match &self.principal {
            Principal::Agent {
                agent_id: authenticated,
            } if authenticated == agent_id => Ok(()),
            Principal::Agent {
                agent_id: authenticated,
            } => Err(ApiError::Forbidden(format!(
                "agent credential for '{authenticated}' cannot act as '{agent_id}'"
            ))),
            Principal::User { subject, .. } => Err(ApiError::Forbidden(format!(
                "principal '{subject}' is not an agent"
            ))),
        }
    }
}

impl FromRequestParts<AppState> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .ok_or_else(|| ApiError::Unauthorized("missing authorization header".to_owned()))?;
        let value = header_value
            .to_str()
            .map_err(|_| ApiError::Unauthorized("malformed authorization header".to_owned()))?;
        let token = value
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("expected a bearer token".to_owned()))?
            .trim();
        if token.is_empty() {
            return Err(ApiError::Unauthorized("bearer token is empty".to_owned()));
        }

        if token == state.auth.agent_token {
            return Ok(Self {
                principal: Principal::Agent {
                    agent_id: state.auth.agent_id.clone(),
                },
            });
        }

        if token == state.auth.user_token {
            return Ok(Self {
                principal: Principal::User {
                    subject: state.auth.user_subject.clone(),
                    capabilities: vec![
                        CAPABILITY_SERVICE_READ.to_owned(),
                        CAPABILITY_SERVICE_CONTROL.to_owned(),
                        CAPABILITY_OPERATION_READ.to_owned(),
                        CAPABILITY_WATCHLIST_READ.to_owned(),
                        CAPABILITY_WATCHLIST_WRITE.to_owned(),
                        CAPABILITY_PROVIDER_READ.to_owned(),
                        CAPABILITY_PROVIDER_WRITE.to_owned(),
                        CAPABILITY_MARKET_READ.to_owned(),
                        CAPABILITY_RESEARCH_READ.to_owned(),
                    ],
                },
            });
        }

        Err(ApiError::Unauthorized(
            "bearer token is not valid".to_owned(),
        ))
    }
}
