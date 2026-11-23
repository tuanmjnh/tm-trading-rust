use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

use tm_trading_ai_providers::{AiProviderError, build_provider};
use tm_trading_domain::AiProviderKind;

struct TestServer {
    base_url: String,
    request: Receiver<String>,
}

impl TestServer {
    fn start(status: &str, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let base_url = format!("http://{}", listener.local_addr().expect("server address"));
        let (request_sender, request) = channel();
        let status = status.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).expect("read request");
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
                if request_is_complete(&bytes) {
                    break;
                }
            }
            request_sender
                .send(String::from_utf8_lossy(&bytes).into_owned())
                .expect("capture request");
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        Self { base_url, request }
    }

    fn request(&self) -> String {
        self.request
            .recv_timeout(Duration::from_secs(2))
            .expect("captured request")
    }
}

fn request_is_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    bytes.len() >= header_end + 4 + content_length
}

fn request_path(request: &str) -> &str {
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("request target")
}

fn request_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

#[tokio::test]
async fn build_provider_routes_anthropic_and_gemini_protocols() {
    let anthropic = TestServer::start(
        "200 OK",
        r#"{"content":[{"type":"text","text":"OK"}]}"#.to_owned(),
    );
    let provider = build_provider(
        AiProviderKind::Anthropic,
        &anthropic.base_url,
        "claude-test",
        "anthropic-secret",
        Duration::from_secs(2),
    )
    .expect("anthropic provider");
    assert_eq!(
        provider.health().await.expect("health").model,
        "claude-test"
    );
    let request = anthropic.request();
    assert_eq!(request_path(&request), "/v1/messages");
    assert_eq!(
        request_header(&request, "x-api-key"),
        Some("anthropic-secret")
    );
    assert_eq!(
        request_header(&request, "anthropic-version"),
        Some("2026-10-21")
    );
    assert!(request_header(&request, "authorization").is_none());

    let gemini = TestServer::start(
        "200 OK",
        r#"{"candidates":[{"content":{"parts":[{"text":"OK"}]}}]}"#.to_owned(),
    );
    let provider = build_provider(
        AiProviderKind::Gemini,
        &gemini.base_url,
        "gemini-test",
        "gemini-secret",
        Duration::from_secs(2),
    )
    .expect("gemini provider");
    assert_eq!(
        provider.health().await.expect("health").model,
        "gemini-test"
    );
    let request = gemini.request();
    assert_eq!(
        request_path(&request),
        "/v1beta/models/gemini-test:generateContent"
    );
    assert_eq!(
        request_header(&request, "x-goog-api-key"),
        Some("gemini-secret")
    );
    assert!(request_header(&request, "authorization").is_none());
}

#[tokio::test]
async fn dedicated_adapters_reject_invalid_and_oversized_responses() {
    let invalid = TestServer::start("200 OK", r#"{"content":[]}"#.to_owned());
    let provider = build_provider(
        AiProviderKind::Anthropic,
        &invalid.base_url,
        "claude-test",
        "anthropic-secret",
        Duration::from_secs(2),
    )
    .expect("anthropic provider");
    assert!(matches!(
        provider.health().await,
        Err(AiProviderError::InvalidResponse)
    ));

    let oversized = TestServer::start("200 OK", "x".repeat(64 * 1024 + 1));
    let provider = build_provider(
        AiProviderKind::Gemini,
        &oversized.base_url,
        "gemini-test",
        "gemini-secret",
        Duration::from_secs(2),
    )
    .expect("gemini provider");
    assert!(matches!(
        provider.health().await,
        Err(AiProviderError::ResponseTooLarge)
    ));
}

#[tokio::test]
async fn authentication_failures_do_not_expose_credentials() {
    let server = TestServer::start("401 Unauthorized", "anthropic-secret".to_owned());
    let provider = build_provider(
        AiProviderKind::Anthropic,
        &server.base_url,
        "claude-test",
        "anthropic-secret",
        Duration::from_secs(2),
    )
    .expect("anthropic provider");

    let error = provider.health().await.expect_err("authentication failure");

    assert!(matches!(error, AiProviderError::HttpStatus(401)));
    assert_eq!(error.code(), "auth_failed");
    assert!(!error.to_string().contains("anthropic-secret"));
}
