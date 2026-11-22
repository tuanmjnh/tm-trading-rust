//! Generic AI provider adapters.
//!
//! The first adapter is OpenAI-compatible Chat Completions. Adapters are bounded,
//! read-only with respect to the platform, and never log or return secret values.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use thiserror::Error;
use url::Url;

/// Largest response body accepted from one provider health request.
pub const MAX_HEALTH_RESPONSE_BYTES: usize = 64 * 1024;

/// Largest combined system and user prompt accepted for one generation request.
pub const MAX_GENERATION_PROMPT_BYTES: usize = 256 * 1024;

/// Largest response body accepted from one generation request.
pub const MAX_GENERATION_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiProviderHealth {
    pub model: String,
    pub latency_ms: u64,
}

/// One bounded text-generation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiGenerationRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_output_tokens: u32,
    pub json_mode: bool,
}

/// One bounded text-generation response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiGenerationResponse {
    pub content: String,
    pub model: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
}

#[derive(Debug, Error)]
pub enum AiProviderError {
    #[error("ai provider base URL is invalid")]
    InvalidBaseUrl,
    #[error("ai provider kind is not supported")]
    UnsupportedKind,
    #[error("ai provider base URL host is not allowed")]
    ForbiddenHost,
    #[error("ai provider base URL must use HTTPS outside loopback")]
    InsecureBaseUrl,
    #[error("ai provider HTTP client could not be created: {0}")]
    Client(String),
    #[error("ai provider request failed: {0}")]
    Request(String),
    #[error("ai provider request timed out")]
    Timeout,
    #[error("ai provider returned HTTP {0}")]
    HttpStatus(u16),
    #[error("ai provider response was invalid")]
    InvalidResponse,
    #[error("ai provider response was too large")]
    ResponseTooLarge,
    #[error("ai provider request is invalid")]
    InvalidRequest,
    #[error("ai provider API key environment variable is not set")]
    MissingApiKey,
}

impl AiProviderError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidBaseUrl => "invalid_base_url",
            Self::UnsupportedKind => "unsupported_kind",
            Self::ForbiddenHost => "forbidden_host",
            Self::InsecureBaseUrl => "insecure_base_url",
            Self::Client(_) => "client_error",
            Self::Request(_) => "request_failed",
            Self::Timeout => "timeout",
            Self::HttpStatus(status) => {
                if *status == 401 || *status == 403 {
                    "auth_failed"
                } else if *status == 404 {
                    "not_found"
                } else if *status >= 500 {
                    "provider_unavailable"
                } else {
                    "http_error"
                }
            }
            Self::InvalidResponse => "invalid_response",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidRequest => "invalid_request",
            Self::MissingApiKey => "missing_api_key",
        }
    }

    /// Whether another model in the same route may be tried after this failure.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Request(_) | Self::Timeout => true,
            Self::HttpStatus(status) => *status == 408 || *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn health(&self) -> Result<AiProviderHealth, AiProviderError>;
    async fn generate(
        &self,
        request: &AiGenerationRequest,
    ) -> Result<AiGenerationResponse, AiProviderError>;
}

/// Generic OpenAI-compatible Chat Completions adapter.
#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    client: Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
    ) -> Result<Self, AiProviderError> {
        validate_ai_provider_base_url(base_url)?;
        let client = Client::builder()
            .timeout(timeout)
            // A provider redirect could otherwise reach an internal address.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AiProviderError::Client(error.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            api_key: api_key.to_owned(),
        })
    }
}

#[async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    async fn health(&self) -> Result<AiProviderHealth, AiProviderError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": "Reply with the single word OK."}],
            "max_tokens": 8,
            "temperature": 0
        });
        let started = Instant::now();
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AiProviderError::Timeout
                } else {
                    AiProviderError::Request(error.to_string())
                }
            })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| AiProviderError::Request(error.to_string()))?;
        if bytes.len() > MAX_HEALTH_RESPONSE_BYTES {
            return Err(AiProviderError::ResponseTooLarge);
        }
        if !status.is_success() {
            return Err(AiProviderError::HttpStatus(status.as_u16()));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| AiProviderError::InvalidResponse)?;
        let content = value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str());
        if content.is_none() {
            return Err(AiProviderError::InvalidResponse);
        }
        Ok(AiProviderHealth {
            model: self.model.clone(),
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    async fn generate(
        &self,
        request: &AiGenerationRequest,
    ) -> Result<AiGenerationResponse, AiProviderError> {
        let body = generation_body(&self.model, request)?;
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AiProviderError::Timeout
                } else {
                    AiProviderError::Request(error.to_string())
                }
            })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| AiProviderError::Request(error.to_string()))?;
        if bytes.len() > MAX_GENERATION_RESPONSE_BYTES {
            return Err(AiProviderError::ResponseTooLarge);
        }
        if !status.is_success() {
            return Err(AiProviderError::HttpStatus(status.as_u16()));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| AiProviderError::InvalidResponse)?;
        parse_generation_response(value, &self.model, started.elapsed().as_millis() as u64)
    }
}

fn generation_body(
    model: &str,
    request: &AiGenerationRequest,
) -> Result<serde_json::Value, AiProviderError> {
    if request.max_output_tokens == 0
        || request.system_prompt.len() + request.user_prompt.len() > MAX_GENERATION_PROMPT_BYTES
    {
        return Err(AiProviderError::InvalidRequest);
    }
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": request.system_prompt},
            {"role": "user", "content": request.user_prompt}
        ],
        "max_tokens": request.max_output_tokens,
        "temperature": 0,
        "stream": false
    });
    if request.json_mode {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }
    Ok(body)
}

fn parse_generation_response(
    value: serde_json::Value,
    model: &str,
    latency_ms: u64,
) -> Result<AiGenerationResponse, AiProviderError> {
    let content = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .filter(|content| !content.trim().is_empty())
        .ok_or(AiProviderError::InvalidResponse)?;
    let usage = value.get("usage");
    Ok(AiGenerationResponse {
        content: content.to_owned(),
        model: model.to_owned(),
        prompt_tokens: usage
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        completion_tokens: usage
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        latency_ms,
    })
}

pub fn validate_ai_provider_base_url(value: &str) -> Result<(), AiProviderError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiProviderError::InvalidBaseUrl);
    }
    let url = Url::parse(value).map_err(|_| AiProviderError::InvalidBaseUrl)?;
    let host = url.host_str().ok_or(AiProviderError::InvalidBaseUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AiProviderError::InvalidBaseUrl);
    }
    let loopback = is_loopback_host(host);
    if loopback {
        return Ok(());
    }
    if url.scheme() != "https" {
        return Err(AiProviderError::InsecureBaseUrl);
    }
    if is_private_or_reserved_host(host) {
        return Err(AiProviderError::ForbiddenHost);
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn is_private_or_reserved_host(host: &str) -> bool {
    let Ok(address) = host.parse::<std::net::IpAddr>() else {
        return false;
    };
    match address {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_private()
                || ipv4.is_link_local()
                || ipv4.is_loopback()
                || ipv4.is_unspecified()
                || ipv4.is_broadcast()
                || (ipv4.octets()[0] == 169 && ipv4.octets()[1] == 254)
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_unique_local()
                || ipv6.is_unicast_link_local()
                || ipv6.is_loopback()
                || ipv6.is_unspecified()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_bounded_json_generation_request() {
        let request = AiGenerationRequest {
            system_prompt: "Return evidence-bound JSON.".to_owned(),
            user_prompt: "Summarize the supplied evidence.".to_owned(),
            max_output_tokens: 512,
            json_mode: true,
        };

        let body = generation_body("model-a", &request).unwrap();

        assert_eq!(body["model"], "model-a");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn rejects_an_invalid_generation_budget() {
        let request = AiGenerationRequest {
            system_prompt: "system".to_owned(),
            user_prompt: "user".to_owned(),
            max_output_tokens: 0,
            json_mode: true,
        };

        assert!(matches!(
            generation_body("model-a", &request).unwrap_err(),
            AiProviderError::InvalidRequest
        ));
    }

    #[test]
    fn parses_generation_content_and_usage() {
        let value = serde_json::json!({
            "choices": [{
                "message": {"content": "{\"facts\":[]}"}
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8
            }
        });

        let response = parse_generation_response(value, "model-a", 25).unwrap();

        assert_eq!(response.content, "{\"facts\":[]}");
        assert_eq!(response.model, "model-a");
        assert_eq!(response.prompt_tokens, Some(12));
        assert_eq!(response.completion_tokens, Some(8));
        assert_eq!(response.latency_ms, 25);
    }

    #[test]
    fn classifies_only_retryable_provider_failures_as_transient() {
        assert!(AiProviderError::Timeout.is_transient());
        assert!(AiProviderError::Request("network".to_owned()).is_transient());
        assert!(AiProviderError::HttpStatus(429).is_transient());
        assert!(AiProviderError::HttpStatus(503).is_transient());
        assert!(!AiProviderError::HttpStatus(401).is_transient());
        assert!(!AiProviderError::InvalidResponse.is_transient());
        assert!(!AiProviderError::MissingApiKey.is_transient());
    }

    #[test]
    fn validates_loopback_http() {
        assert!(validate_ai_provider_base_url("http://localhost:11434/v1").is_ok());
    }

    #[test]
    fn rejects_private_non_loopback_http() {
        assert!(matches!(
            validate_ai_provider_base_url("http://10.0.0.1/v1"),
            Err(AiProviderError::InsecureBaseUrl)
        ));
    }

    #[test]
    fn rejects_metadata_host() {
        assert!(matches!(
            validate_ai_provider_base_url("https://169.254.169.254/v1"),
            Err(AiProviderError::ForbiddenHost)
        ));
    }
}
