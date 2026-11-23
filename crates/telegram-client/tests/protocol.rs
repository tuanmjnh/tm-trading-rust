use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde_json::json;
use tm_trading_telegram_client::{TelegramBotToken, TelegramClient};
use tokio::net::TcpListener;
use tokio::sync::watch;

#[derive(Clone)]
struct MockServerState {
    status_code: StatusCode,
    body: serde_json::Value,
}

#[tokio::test]
async fn classifies_accepted_message_and_id() {
    let state = MockServerState {
        status_code: StatusCode::OK,
        body: json!({
            "ok": true,
            "result": {
                "message_id": 8842,
                "chat": { "id": 12345, "type": "private" },
                "text": "test message"
            }
        }),
    };

    let (base_url, _shutdown_tx) = spawn_mock_server(state).await;
    let token = TelegramBotToken::new("12345:TEST_TOKEN").unwrap();
    let client = TelegramClient::new(token, Some(&base_url), Some(Duration::from_secs(5))).unwrap();

    let outcome = client.send_message("12345", "test message").await.unwrap();
    assert!(outcome.ok);
    assert!(outcome.accepted);
    assert_eq!(outcome.message_id.as_deref(), Some("8842"));
    assert!(!outcome.retryable);
}

#[tokio::test]
async fn classifies_429_rate_limiting_as_retryable_with_backoff() {
    let state = MockServerState {
        status_code: StatusCode::TOO_MANY_REQUESTS,
        body: json!({
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests: retry after 7",
            "parameters": {
                "retry_after": 7
            }
        }),
    };

    let (base_url, _shutdown_tx) = spawn_mock_server(state).await;
    let token = TelegramBotToken::new("12345:TEST_TOKEN").unwrap();
    let client = TelegramClient::new(token, Some(&base_url), Some(Duration::from_secs(5))).unwrap();

    let outcome = client.send_message("12345", "test message").await.unwrap();
    assert!(!outcome.ok);
    assert!(!outcome.accepted);
    assert!(outcome.retryable);
    assert_eq!(outcome.retry_after_ms, Some(7000));
    assert_eq!(outcome.error_code, Some(429));
}

#[tokio::test]
async fn classifies_400_bad_request_as_terminal_non_retryable() {
    let state = MockServerState {
        status_code: StatusCode::BAD_REQUEST,
        body: json!({
            "ok": false,
            "error_code": 400,
            "description": "Bad Request: chat not found"
        }),
    };

    let (base_url, _shutdown_tx) = spawn_mock_server(state).await;
    let token = TelegramBotToken::new("12345:TEST_TOKEN").unwrap();
    let client = TelegramClient::new(token, Some(&base_url), Some(Duration::from_secs(5))).unwrap();

    let outcome = client.send_message("99999", "test message").await.unwrap();
    assert!(!outcome.ok);
    assert!(!outcome.accepted);
    assert!(!outcome.retryable);
    assert_eq!(outcome.error_code, Some(400));
}

#[tokio::test]
async fn classifies_502_bad_gateway_as_retryable() {
    let state = MockServerState {
        status_code: StatusCode::BAD_GATEWAY,
        body: json!({
            "ok": false,
            "error_code": 502,
            "description": "Bad Gateway"
        }),
    };

    let (base_url, _shutdown_tx) = spawn_mock_server(state).await;
    let token = TelegramBotToken::new("12345:TEST_TOKEN").unwrap();
    let client = TelegramClient::new(token, Some(&base_url), Some(Duration::from_secs(5))).unwrap();

    let outcome = client.send_message("12345", "test message").await.unwrap();
    assert!(!outcome.ok);
    assert!(!outcome.accepted);
    assert!(outcome.retryable);
    assert_eq!(outcome.error_code, Some(502));
}

async fn mock_handler(
    State(state): State<MockServerState>,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    (state.status_code, Json(state.body))
}

async fn spawn_mock_server(state: MockServerState) -> (String, watch::Sender<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let app = axum::Router::new()
        .route("/bot{token}/sendMessage", post(mock_handler))
        .with_state(state);

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            })
            .await
            .unwrap();
    });

    (base_url, shutdown_tx)
}
