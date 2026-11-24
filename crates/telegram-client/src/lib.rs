use std::fmt;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// Default Telegram Bot API base URL.
pub const DEFAULT_TELEGRAM_BASE_URL: &str = "https://api.telegram.org";

/// Default timeout for outbound Telegram API requests.
pub const DEFAULT_TELEGRAM_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum text length accepted by Telegram sendMessage before splitting.
pub const MAX_TELEGRAM_TEXT_CHARS: usize = 4_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelegramError {
    #[error("bot token cannot be empty")]
    EmptyToken,

    #[error("invalid bot token format")]
    InvalidTokenFormat,

    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(String),

    #[error("chat_id cannot be empty")]
    EmptyChatId,

    #[error("text message cannot be empty")]
    EmptyText,

    #[error("http client error: {0}")]
    Http(String),
}

/// Secret-safe container for a Telegram bot token that redacts itself in logs and debug formats.
#[derive(Clone, PartialEq, Eq)]
pub struct TelegramBotToken(String);

impl TelegramBotToken {
    pub fn new(token: impl Into<String>) -> Result<Self, TelegramError> {
        let t = token.into();
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return Err(TelegramError::EmptyToken);
        }
        // Basic check for Telegram bot token structure: <bot_id>:<secret>
        if !trimmed.contains(':') {
            return Err(TelegramError::InvalidTokenFormat);
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TelegramBotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***REDACTED_TELEGRAM_TOKEN***")
    }
}

impl fmt::Display for TelegramBotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***REDACTED_TELEGRAM_TOKEN***")
    }
}

/// Redacts any instance of the secret token from an arbitrary string.
#[must_use]
pub fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    text.replace(secret, "***REDACTED***")
}

/// Outcome of a delivery attempt according to Telegram evidence (roadmap §7.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramDeliveryOutcome {
    pub ok: bool,
    pub accepted: bool,
    pub message_id: Option<String>,
    pub description: Option<String>,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub error_code: Option<i32>,
}

#[derive(Serialize)]
struct SendMessagePayload<'a> {
    chat_id: &'a str,
    text: &'a str,
    disable_web_page_preview: bool,
}

#[derive(Deserialize)]
struct TelegramApiResponse {
    ok: bool,
    result: Option<TelegramMessageResult>,
    error_code: Option<i32>,
    description: Option<String>,
    parameters: Option<TelegramResponseParameters>,
}

#[derive(Deserialize)]
struct TelegramMessageResult {
    message_id: i64,
}

#[derive(Deserialize)]
struct TelegramResponseParameters {
    retry_after: Option<u64>,
}

/// Telegram Bot API client.
#[derive(Clone)]
pub struct TelegramClient {
    token: TelegramBotToken,
    base_url: Url,
    http_client: Client,
}

impl TelegramClient {
    pub fn new(
        token: TelegramBotToken,
        base_url_override: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Self, TelegramError> {
        let base_str = base_url_override.unwrap_or(DEFAULT_TELEGRAM_BASE_URL);
        let base_url = Url::parse(base_str)
            .map_err(|e| TelegramError::InvalidBaseUrl(format!("{base_str}: {e}")))?;

        let scheme = base_url.scheme();
        let host = base_url.host_str().unwrap_or_default();
        let is_loopback = host == "127.0.0.1"
            || host.eq_ignore_ascii_case("localhost")
            || host == "::1"
            || host == "[::1]";

        if scheme == "http" && !is_loopback {
            return Err(TelegramError::InvalidBaseUrl(format!(
                "remote Telegram base URL '{base_str}' must use HTTPS to protect bot credentials"
            )));
        } else if scheme != "http" && scheme != "https" {
            return Err(TelegramError::InvalidBaseUrl(format!(
                "unsupported scheme '{scheme}' in Telegram base URL"
            )));
        }

        let request_timeout = timeout.unwrap_or(DEFAULT_TELEGRAM_TIMEOUT);
        let http_client = Client::builder()
            .timeout(request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| TelegramError::Http(e.to_string()))?;

        Ok(Self {
            token,
            base_url,
            http_client,
        })
    }

    /// Sends a text message to a specific chat and classifies the outcome.
    ///
    /// Never exposes the bot token in logs, URLs, or return values.
    pub async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
    ) -> Result<TelegramDeliveryOutcome, TelegramError> {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            return Err(TelegramError::EmptyChatId);
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(TelegramError::EmptyText);
        }

        let bounded_text = safe_truncate_chars(text, MAX_TELEGRAM_TEXT_CHARS);

        let secret = self.token.expose_secret();
        let endpoint = format!(
            "{}/bot{}/sendMessage",
            self.base_url.as_str().trim_end_matches('/'),
            secret
        );

        let payload = SendMessagePayload {
            chat_id,
            text: bounded_text,
            disable_web_page_preview: true,
        };

        let response = match self.http_client.post(&endpoint).json(&payload).send().await {
            Ok(resp) => resp,
            Err(e) => {
                let err_str = redact_secret(&e.to_string(), secret);
                tracing::warn!(error = %err_str, "telegram transport failure");
                return Ok(TelegramDeliveryOutcome {
                    ok: false,
                    accepted: false,
                    message_id: None,
                    description: Some(err_str),
                    retryable: true,
                    retry_after_ms: None,
                    error_code: None,
                });
            }
        };

        let http_status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let api_resp: Option<TelegramApiResponse> = serde_json::from_str(&body_text).ok();

        if http_status.is_success() && api_resp.as_ref().is_some_and(|r| r.ok) {
            let message_id = api_resp
                .and_then(|r| r.result)
                .map(|res| res.message_id.to_string());
            let accepted = message_id.is_some();
            return Ok(TelegramDeliveryOutcome {
                ok: true,
                accepted,
                message_id,
                description: None,
                retryable: false,
                retry_after_ms: None,
                error_code: None,
            });
        }

        let error_code = api_resp
            .as_ref()
            .and_then(|r| r.error_code)
            .or_else(|| Some(http_status.as_u16() as i32));

        let description = api_resp
            .as_ref()
            .and_then(|r| r.description.clone())
            .unwrap_or_else(|| format!("HTTP {}", http_status.as_u16()));
        let description = redact_secret(&description, secret);

        let retry_after_sec = api_resp
            .as_ref()
            .and_then(|r| r.parameters.as_ref())
            .and_then(|p| p.retry_after);
        let retry_after_ms = retry_after_sec.map(|sec| sec * 1000);

        // Classify retryable: 429, 5xx, or network failures are retryable.
        // 400, 401, 403, 404 are permanent configuration faults.
        let status_code = http_status.as_u16();
        let retryable = status_code == 429
            || status_code >= 500
            || error_code.is_some_and(|c| c == 429 || c >= 500);

        tracing::warn!(
            status = status_code,
            error_code = ?error_code,
            retryable = retryable,
            description = %description,
            "telegram API reported failure"
        );

        Ok(TelegramDeliveryOutcome {
            ok: false,
            accepted: false,
            message_id: None,
            description: Some(description),
            retryable,
            retry_after_ms,
            error_code,
        })
    }
}

/// Truncates a UTF-8 string at a valid Unicode character boundary up to `max_chars`.
///
/// Prevents panics when slicing multi-byte characters such as Vietnamese diacritics and emojis.
#[must_use]
pub fn safe_truncate_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &text[..byte_idx],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_redacts_itself_in_debug_and_display() {
        let token = TelegramBotToken::new("123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11").unwrap();
        assert_eq!(format!("{token:?}"), "***REDACTED_TELEGRAM_TOKEN***");
        assert_eq!(format!("{token}"), "***REDACTED_TELEGRAM_TOKEN***");
        assert_eq!(
            token.expose_secret(),
            "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
        );
    }

    #[test]
    fn validates_token_structure() {
        assert_eq!(TelegramBotToken::new(""), Err(TelegramError::EmptyToken));
        assert_eq!(
            TelegramBotToken::new("invalid_token_no_colon"),
            Err(TelegramError::InvalidTokenFormat)
        );
        assert!(TelegramBotToken::new("12345:valid_token").is_ok());
    }

    #[test]
    fn redacts_secret_from_arbitrary_string() {
        let secret = "123456:SECRET_TOKEN";
        let raw =
            format!("Failed to request https://api.telegram.org/bot{secret}/sendMessage: timeout");
        let redacted = redact_secret(&raw, secret);
        assert!(!redacted.contains(secret));
        assert_eq!(
            redacted,
            "Failed to request https://api.telegram.org/bot***REDACTED***/sendMessage: timeout"
        );
    }

    #[test]
    fn safe_truncate_chars_handles_vietnamese_and_emojis() {
        let vietnamese = "Hệ thống giao dịch TM Trading: Tín hiệu MUA BTCUSDT tại vùng giá $95,000! 🚀🔥📈";
        // String has multi-byte Vietnamese chars and 4-byte emojis
        assert_eq!(safe_truncate_chars(vietnamese, 10), "Hệ thống g");
        assert_eq!(safe_truncate_chars(vietnamese, 1000), vietnamese);

        // Multi-byte emoji boundary test
        let emoji_str = "🚀📈🔥";
        assert_eq!(safe_truncate_chars(emoji_str, 1), "🚀");
        assert_eq!(safe_truncate_chars(emoji_str, 2), "🚀📈");
        assert_eq!(safe_truncate_chars(emoji_str, 3), "🚀📈🔥");
    }

    #[test]
    fn base_url_enforces_https_for_remote_hosts() {
        let token = TelegramBotToken::new("123456:ABC-DEF").unwrap();

        // Loopback is allowed with HTTP (e.g. mock test server)
        assert!(TelegramClient::new(token.clone(), Some("http://127.0.0.1:9090"), None).is_ok());
        assert!(TelegramClient::new(token.clone(), Some("http://localhost:9090"), None).is_ok());
        assert!(TelegramClient::new(token.clone(), Some("http://[::1]:9090"), None).is_ok());

        // Remote host with HTTPS is allowed
        assert!(TelegramClient::new(token.clone(), Some("https://api.telegram.org"), None).is_ok());
        assert!(TelegramClient::new(token.clone(), Some("https://custom-telegram.proxy.internal"), None).is_ok());

        // Remote host with HTTP is rejected
        let err = TelegramClient::new(token.clone(), Some("http://api.telegram.org"), None).unwrap_err();
        assert!(matches!(err, TelegramError::InvalidBaseUrl(_)));
        assert!(err.to_string().contains("must use HTTPS"));

        // Unsupported scheme is rejected
        let err_scheme = TelegramClient::new(token, Some("ftp://api.telegram.org"), None).unwrap_err();
        assert!(matches!(err_scheme, TelegramError::InvalidBaseUrl(_)));
    }
}
