use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use url::Url;

use super::{
    AiGenerationRequest, AiGenerationResponse, AiProvider, AiProviderError, AiProviderHealth,
    MAX_GENERATION_RESPONSE_BYTES, MAX_HEALTH_RESPONSE_BYTES, build_client, map_request_error,
    read_bounded_body, validate_ai_provider_base_url, validate_generation_request,
};

/// Default Gemini API root.
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// Dedicated Google Gemini `generateContent` adapter.
#[derive(Clone)]
pub struct GeminiProvider {
    client: Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl GeminiProvider {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
    ) -> Result<Self, AiProviderError> {
        validate_ai_provider_base_url(base_url)?;
        let model = model.trim().trim_start_matches("models/");
        let api_key = api_key.trim();
        if model.is_empty() || api_key.is_empty() {
            return Err(AiProviderError::InvalidRequest);
        }
        Ok(Self {
            client: build_client(timeout)?,
            base_url: base_url.trim().to_owned(),
            model: model.to_owned(),
            api_key: api_key.to_owned(),
        })
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn health(&self) -> Result<AiProviderHealth, AiProviderError> {
        let request = AiGenerationRequest {
            system_prompt: String::new(),
            user_prompt: "Reply with the single word OK.".to_owned(),
            max_output_tokens: 8,
            json_mode: false,
            json_schema: None,
        };
        let body = gemini_generation_body(&request)?;
        let started = Instant::now();
        let mut response = self
            .client
            .post(gemini_generate_content_url(&self.base_url, &self.model)?)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(map_request_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(AiProviderError::HttpStatus(status.as_u16()));
        }
        let bytes = read_bounded_body(&mut response, MAX_HEALTH_RESPONSE_BYTES).await?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| AiProviderError::InvalidResponse)?;
        parse_gemini_content(&value)?;
        Ok(AiProviderHealth {
            model: self.model.clone(),
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    async fn generate(
        &self,
        request: &AiGenerationRequest,
    ) -> Result<AiGenerationResponse, AiProviderError> {
        let body = gemini_generation_body(request)?;
        let started = Instant::now();
        let mut response = self
            .client
            .post(gemini_generate_content_url(&self.base_url, &self.model)?)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(map_request_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(AiProviderError::HttpStatus(status.as_u16()));
        }
        let bytes = read_bounded_body(&mut response, MAX_GENERATION_RESPONSE_BYTES).await?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| AiProviderError::InvalidResponse)?;
        parse_gemini_response(value, &self.model, started.elapsed().as_millis() as u64)
    }
}

fn gemini_generation_body(request: &AiGenerationRequest) -> Result<Value, AiProviderError> {
    validate_generation_request(request)?;
    let mut generation_config = serde_json::json!({
        "temperature": 0,
        "maxOutputTokens": request.max_output_tokens
    });
    if request.json_mode {
        let mut text_format = serde_json::json!({"mimeType": "APPLICATION_JSON"});
        if let Some(schema) = &request.json_schema {
            text_format["schema"] = schema.clone();
        }
        generation_config["responseFormat"] = serde_json::json!({"text": text_format});
    }
    let mut body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": request.user_prompt}]
        }],
        "generationConfig": generation_config
    });
    if !request.system_prompt.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{"text": request.system_prompt}]
        });
    }
    Ok(body)
}

fn gemini_generate_content_url(base_url: &str, model: &str) -> Result<Url, AiProviderError> {
    validate_ai_provider_base_url(base_url)?;
    let mut url = Url::parse(base_url.trim()).map_err(|_| AiProviderError::InvalidBaseUrl)?;
    let has_version = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .is_some_and(|segment| segment == "v1beta");
    let model = model.trim().trim_start_matches("models/");
    if model.is_empty() {
        return Err(AiProviderError::InvalidRequest);
    }
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| AiProviderError::InvalidBaseUrl)?;
        segments.pop_if_empty();
        if !has_version {
            segments.push("v1beta");
        }
        segments.push("models");
        segments.push(&format!("{model}:generateContent"));
    }
    Ok(url)
}

fn parse_gemini_response(
    value: Value,
    model: &str,
    latency_ms: u64,
) -> Result<AiGenerationResponse, AiProviderError> {
    let content = parse_gemini_content(&value)?;
    let usage = value.get("usageMetadata");
    Ok(AiGenerationResponse {
        content,
        model: model.to_owned(),
        prompt_tokens: usage
            .and_then(|usage| usage.get("promptTokenCount"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        completion_tokens: usage
            .and_then(|usage| usage.get("candidatesTokenCount"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        latency_ms,
    })
}

fn parse_gemini_content(value: &Value) -> Result<String, AiProviderError> {
    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .ok_or(AiProviderError::InvalidResponse)?;
    let parts = candidate
        .get("content")
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .ok_or(AiProviderError::InvalidResponse)?;
    let mut content = String::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            content.push_str(text);
        }
    }
    if content.trim().is_empty() {
        return Err(AiProviderError::InvalidResponse);
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> AiGenerationRequest {
        AiGenerationRequest {
            system_prompt: "Return evidence-bound JSON.".to_owned(),
            user_prompt: "Summarize the supplied evidence.".to_owned(),
            max_output_tokens: 512,
            json_mode: true,
            json_schema: Some(json!({
                "type": "object",
                "properties": {
                    "interpretations": {"type": "array", "items": {"type": "string"}},
                    "uncertainties": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["interpretations", "uncertainties"]
            })),
        }
    }

    #[test]
    fn builds_a_generate_content_request_with_system_instruction_and_schema() {
        let body = gemini_generation_body(&request()).unwrap();

        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "Return evidence-bound JSON."
        );
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(
            body["contents"][0]["parts"][0]["text"],
            "Summarize the supplied evidence."
        );
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 512);
        assert_eq!(
            body["generationConfig"]["responseFormat"]["text"]["mimeType"],
            "APPLICATION_JSON"
        );
        assert_eq!(
            body["generationConfig"]["responseFormat"]["text"]["schema"]["required"][0],
            "interpretations"
        );
    }

    #[test]
    fn selects_the_model_generate_content_endpoint_without_duplicating_the_api_version() {
        assert_eq!(
            gemini_generate_content_url("https://generativelanguage.googleapis.com", "gemini-test")
                .unwrap()
                .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-test:generateContent"
        );
        assert_eq!(
            gemini_generate_content_url(
                "https://generativelanguage.googleapis.com/v1beta",
                "gemini-test"
            )
            .unwrap()
            .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-test:generateContent"
        );
    }

    #[test]
    fn parses_text_content_and_usage() {
        let value = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "{\"interpretations\":[]}"}]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "candidatesTokenCount": 8
            }
        });

        let response = parse_gemini_response(value, "gemini-test", 25).unwrap();

        assert_eq!(response.content, "{\"interpretations\":[]}");
        assert_eq!(response.model, "gemini-test");
        assert_eq!(response.prompt_tokens, Some(12));
        assert_eq!(response.completion_tokens, Some(8));
        assert_eq!(response.latency_ms, 25);
    }
}
