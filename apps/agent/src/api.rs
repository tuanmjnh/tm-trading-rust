use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tm_trading_contracts::{
    AgentCommandDto, AgentHeartbeatRequest, AgentHeartbeatResponse, ClaimCommandRequest,
    ClaimCommandResponse, CompleteCommandRequest, CompleteCommandResponse, ProgressCommandRequest,
    ProgressCommandResponse, ServiceSnapshot,
};

use crate::AgentError;

/// Outbound client for the Management API command channel.
#[derive(Debug, Clone)]
pub struct ManagementApiClient {
    http: Client,
    base_url: String,
    token: String,
    agent_id: String,
}

impl ManagementApiClient {
    pub fn new(base_url: &str, token: &str, agent_id: &str) -> Result<Self, AgentError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| AgentError::Api(format!("failed to build HTTP client: {error}")))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            agent_id: agent_id.to_owned(),
        })
    }

    pub async fn heartbeat(&self, services: Vec<ServiceSnapshot>) -> Result<(), AgentError> {
        let body = AgentHeartbeatRequest {
            agent_id: self.agent_id.clone(),
            agent_version: env!("CARGO_PKG_VERSION").to_owned(),
            host_os: std::env::consts::OS.to_owned(),
            host_architecture: std::env::consts::ARCH.to_owned(),
            services,
        };
        let response: AgentHeartbeatResponse = self.post("/api/v1/agent/heartbeat", &body).await?;
        if !response.accepted {
            return Err(AgentError::Api(
                "management API rejected the heartbeat".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn claim_command(&self) -> Result<Option<AgentCommandDto>, AgentError> {
        let body = ClaimCommandRequest {
            agent_id: self.agent_id.clone(),
        };
        let response: ClaimCommandResponse =
            self.post("/api/v1/agent/commands/claim", &body).await?;
        Ok(response.command)
    }

    pub async fn mark_running(&self, command: &AgentCommandDto) -> Result<(), AgentError> {
        let body = ProgressCommandRequest {
            operation_id: command.operation_id.clone(),
            agent_id: self.agent_id.clone(),
            fencing_token: command.fencing_token,
        };
        let response: ProgressCommandResponse = self
            .post(
                &format!("/api/v1/agent/commands/{}/progress", command.operation_id),
                &body,
            )
            .await?;
        if !response.accepted {
            return Err(AgentError::Api(
                "management API rejected the command progress report".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn complete_command(
        &self,
        command: &AgentCommandDto,
        result: Result<String, AgentError>,
    ) -> Result<(), AgentError> {
        let (succeeded, result_summary, error) = match result {
            Ok(summary) => (true, Some(summary), None),
            Err(error) => (false, None, Some(error)),
        };
        let body = CompleteCommandRequest {
            operation_id: command.operation_id.clone(),
            agent_id: self.agent_id.clone(),
            fencing_token: command.fencing_token,
            succeeded,
            result_summary,
            error_code: error.as_ref().map(|_| "AGENT_COMMAND_FAILED".to_owned()),
            redacted_error: error.map(|error| error.to_string()),
        };
        let response: CompleteCommandResponse = self
            .post(
                &format!("/api/v1/agent/commands/{}/complete", command.operation_id),
                &body,
            )
            .await?;
        if !response.accepted {
            return Err(AgentError::Api(
                "management API rejected the command completion report".to_owned(),
            ));
        }
        Ok(())
    }

    async fn post<TRequest, TResponse>(
        &self,
        path: &str,
        body: &TRequest,
    ) -> Result<TResponse, AgentError>
    where
        TRequest: serde::Serialize + Sync,
        TResponse: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|error| AgentError::Api(format!("request to {path} failed: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::Api(format!(
                "request to {path} returned {status}: {body}"
            )));
        }
        response
            .json()
            .await
            .map_err(|error| AgentError::Api(format!("response from {path} is invalid: {error}")))
    }
}
