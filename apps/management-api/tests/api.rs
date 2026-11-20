use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use tm_trading_domain::{Candle, NewCandle, NewResearchEvent, ResearchEvent, ResearchQuality};
use tm_trading_management_api::{AppState, AuthConfig, router};

const USER_TOKEN: &str = "user-token";
const AGENT_TOKEN: &str = "agent-token";

fn state() -> AppState {
    AppState::new(AuthConfig {
        user_token: USER_TOKEN.to_owned(),
        agent_token: AGENT_TOKEN.to_owned(),
        user_subject: "test-operator".to_owned(),
        agent_id: "local-agent".to_owned(),
    })
}

fn request(
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("request is valid")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body is readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

fn snapshot(instance_id: &str) -> serde_json::Value {
    json!({
        "service_instance_id": instance_id,
        "service_definition_id": "sample-worker",
        "agent_id": "local-agent",
        "desired_state": "STOPPED",
        "observed_state": "STOPPED",
        "readiness": "NOT_READY",
        "process_alive": false,
        "restart_count": 0,
        "last_exit_code": null,
        "retry_blocked": false,
        "last_heartbeat_at": null,
        "updated_at": "2026-11-18T00:00:00Z"
    })
}

#[tokio::test]
async fn user_endpoints_require_a_user_credential() {
    let app = router(state());

    let response = app
        .clone()
        .oneshot(request("GET", "/api/v1/services", None, None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(request("GET", "/api/v1/services", Some(AGENT_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn heartbeat_publishes_the_service_read_model() {
    let app = router(state());

    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/agent/heartbeat",
            Some(AGENT_TOKEN),
            Some(json!({
                "agent_id": "local-agent",
                "agent_version": "0.1.0",
                "host_os": "windows",
                "host_architecture": "x86_64",
                "services": [snapshot("sample-worker@local-agent")]
            })),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(request("GET", "/api/v1/services", Some(USER_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let services = body.as_array().expect("service list");
    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["service_definition_id"], "sample-worker");
    assert_eq!(services[0]["readiness"], "NOT_READY");
}

#[tokio::test]
async fn a_heartbeat_removes_instances_the_agent_no_longer_reports() {
    let app = router(state());

    let first = request(
        "POST",
        "/api/v1/agent/heartbeat",
        Some(AGENT_TOKEN),
        Some(json!({
            "agent_id": "local-agent",
            "agent_version": "0.1.0",
            "host_os": "windows",
            "host_architecture": "x86_64",
            "services": [snapshot("sample-worker@local-agent:old")]
        })),
    );
    let response = app.clone().oneshot(first).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let second = request(
        "POST",
        "/api/v1/agent/heartbeat",
        Some(AGENT_TOKEN),
        Some(json!({
            "agent_id": "local-agent",
            "agent_version": "0.1.0",
            "host_os": "windows",
            "host_architecture": "x86_64",
            "services": [snapshot("sample-worker@local-agent:new")]
        })),
    );
    let response = app.clone().oneshot(second).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(request("GET", "/api/v1/services", Some(USER_TOKEN), None))
        .await
        .expect("response");
    let body = json_body(response).await;
    let services = body.as_array().expect("service list");
    assert_eq!(services.len(), 1);
    assert_eq!(
        services[0]["service_instance_id"],
        "sample-worker@local-agent:new"
    );
}

#[tokio::test]
async fn an_agent_credential_cannot_act_as_another_agent() {
    let app = router(state());

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/agent/heartbeat",
            Some(AGENT_TOKEN),
            Some(json!({
                "agent_id": "other-agent",
                "agent_version": "0.1.0",
                "host_os": "windows",
                "host_architecture": "x86_64",
                "services": []
            })),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_operation_is_durable_idempotent_and_observable() {
    let app = router(state());

    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/agent/heartbeat",
            Some(AGENT_TOKEN),
            Some(json!({
                "agent_id": "local-agent",
                "agent_version": "0.1.0",
                "host_os": "windows",
                "host_architecture": "x86_64",
                "services": [snapshot("sample-worker@local-agent")]
            })),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/services/sample-worker@local-agent/operations")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {USER_TOKEN}"))
        .header("Idempotency-Key", "start-sample-1")
        .body(Body::from(
            json!({
                "service_instance_id": "sample-worker@local-agent",
                "action": "start"
            })
            .to_string(),
        ))
        .expect("request is valid");

    let response = app.clone().oneshot(create).await.expect("response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    let operation_id = body["operation_id"]
        .as_str()
        .expect("operation id")
        .to_owned();
    assert_eq!(body["status"], "accepted");
    assert_eq!(body["target_agent_id"], "local-agent");

    let repeated = Request::builder()
        .method("POST")
        .uri("/api/v1/services/sample-worker@local-agent/operations")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {USER_TOKEN}"))
        .header("Idempotency-Key", "start-sample-1")
        .body(Body::from(
            json!({
                "service_instance_id": "sample-worker@local-agent",
                "action": "start"
            })
            .to_string(),
        ))
        .expect("request is valid");
    let response = app.clone().oneshot(repeated).await.expect("response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(json_body(response).await["operation_id"], operation_id);

    let conflicting = Request::builder()
        .method("POST")
        .uri("/api/v1/services/sample-worker@local-agent/operations")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {USER_TOKEN}"))
        .header("Idempotency-Key", "start-sample-1")
        .body(Body::from(
            json!({
                "service_instance_id": "sample-worker@local-agent",
                "action": "stop"
            })
            .to_string(),
        ))
        .expect("request is valid");
    let response = app.clone().oneshot(conflicting).await.expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let claim = request(
        "POST",
        "/api/v1/agent/commands/claim",
        Some(AGENT_TOKEN),
        Some(json!({ "agent_id": "local-agent" })),
    );
    let response = app.clone().oneshot(claim).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let command = json_body(response).await["command"].clone();
    let command = command.as_object().expect("command is present");
    assert_eq!(command["operation_id"], operation_id);
    assert_eq!(command["action"], "start");
    let fencing_token = command["fencing_token"].as_u64().expect("fencing token");

    let progress = request(
        "POST",
        &format!("/api/v1/agent/commands/{operation_id}/progress"),
        Some(AGENT_TOKEN),
        Some(json!({
            "operation_id": operation_id,
            "agent_id": "local-agent",
            "fencing_token": fencing_token
        })),
    );
    let response = app.clone().oneshot(progress).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let complete = request(
        "POST",
        &format!("/api/v1/agent/commands/{operation_id}/complete"),
        Some(AGENT_TOKEN),
        Some(json!({
            "operation_id": operation_id,
            "agent_id": "local-agent",
            "fencing_token": fencing_token,
            "succeeded": true,
            "result_summary": "service 'sample-worker' started and ready",
            "error_code": null,
            "redacted_error": null
        })),
    );
    let response = app.clone().oneshot(complete).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/operations/{operation_id}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["status"], "succeeded");
    assert_eq!(
        body["result_summary"],
        "service 'sample-worker' started and ready"
    );
    assert_eq!(body["attempt"], 1);
}

#[tokio::test]
async fn an_operation_requires_an_idempotency_key_and_a_known_service() {
    let app = router(state());

    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/services/sample-worker@local-agent/operations",
            Some(USER_TOKEN),
            Some(json!({
                "service_instance_id": "sample-worker@local-agent",
                "action": "start"
            })),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let unknown_service = Request::builder()
        .method("POST")
        .uri("/api/v1/services/unknown-service/operations")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {USER_TOKEN}"))
        .header("Idempotency-Key", "unknown-service-1")
        .body(Body::from(
            json!({
                "service_instance_id": "unknown-service",
                "action": "start"
            })
            .to_string(),
        ))
        .expect("request is valid");
    let response = app
        .clone()
        .oneshot(unknown_service)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn watchlists_are_created_listed_updated_and_deleted() {
    let app = router(state());

    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/watchlists",
            Some(USER_TOKEN),
            Some(json!({ "name": "Core", "symbols": ["btcusdt", "ETHUSDT", "BTCUSDT"] })),
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    let watchlist_id = created["id"].as_str().expect("id").to_owned();
    assert_eq!(created["name"], "Core");
    assert_eq!(created["symbols"], json!(["BTCUSDT", "ETHUSDT"]));
    assert_eq!(created["is_default"], true);

    let listed = app
        .clone()
        .oneshot(request("GET", "/api/v1/watchlists", Some(USER_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await, json!([created]));

    let replaced = app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/api/v1/watchlists/{watchlist_id}/symbols"),
            Some(USER_TOKEN),
            Some(json!({ "symbols": ["ethusdt"] })),
        ))
        .await
        .expect("response");
    assert_eq!(replaced.status(), StatusCode::OK);
    assert_eq!(json_body(replaced).await["symbols"], json!(["ETHUSDT"]));

    let deleted = app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/api/v1/watchlists/{watchlist_id}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let listed = app
        .oneshot(request("GET", "/api/v1/watchlists", Some(USER_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(json_body(listed).await, json!([]));
}

#[tokio::test]
async fn watchlist_errors_map_to_stable_statuses() {
    let app = router(state());

    let first = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/watchlists",
            Some(USER_TOKEN),
            Some(json!({ "name": "Core" })),
        ))
        .await
        .expect("response");
    assert_eq!(first.status(), StatusCode::CREATED);

    let duplicate = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/watchlists",
            Some(USER_TOKEN),
            Some(json!({ "name": "core" })),
        ))
        .await
        .expect("response");
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let invalid = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/watchlists",
            Some(USER_TOKEN),
            Some(json!({ "name": "Bad", "symbols": ["BTC USDT"] })),
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let unknown = app
        .clone()
        .oneshot(request(
            "DELETE",
            "/api/v1/watchlists/unknown",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let agent = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/watchlists",
            Some(AGENT_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(agent.status(), StatusCode::FORBIDDEN);

    let anonymous = app
        .oneshot(request("GET", "/api/v1/watchlists", None, None))
        .await
        .expect("response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

async fn spawn_ping_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().route(
        "/api/v3/ping",
        axum::routing::get(|| async { StatusCode::OK }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the test server");
    let address = listener.local_addr().expect("read the test address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve the test API");
    });
    (format!("http://{address}"), handle)
}

#[tokio::test]
async fn providers_are_configured_probed_and_removed() {
    let (base_url, server) = spawn_ping_server().await;
    let app = router(state());

    let catalog = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/providers/catalog",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(catalog.status(), StatusCode::OK);
    let catalog = json_body(catalog).await;
    let definitions = catalog.as_array().expect("catalog list");
    assert_eq!(definitions.len(), 2);
    assert_eq!(definitions[0]["id"], "binance");
    assert_eq!(
        definitions[0]["default_base_url"],
        "https://api.binance.com"
    );
    assert_eq!(definitions[1]["id"], "okx");

    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/providers",
            Some(USER_TOKEN),
            Some(json!({
                "provider_definition_id": "binance",
                "display_name": "Local Binance",
                "base_url": base_url,
                "timeout_ms": 2_000
            })),
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    let provider_id = created["id"].as_str().expect("id").to_owned();
    assert_eq!(created["enabled"], true);
    assert_eq!(created["health_state"], "UNKNOWN");
    assert_eq!(created["last_health_check_at"], json!(null));

    let probed = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/v1/providers/{provider_id}/test"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(probed.status(), StatusCode::OK);
    let probed = json_body(probed).await;
    assert_eq!(probed["health_state"], "HEALTHY");
    assert_eq!(probed["last_error_code"], json!(null));
    assert!(probed["last_latency_ms"].is_number());
    assert!(probed["last_success_at"].is_string());

    let patched = app
        .clone()
        .oneshot(request(
            "PATCH",
            &format!("/api/v1/providers/{provider_id}"),
            Some(USER_TOKEN),
            Some(json!({ "display_name": "Renamed", "enabled": false })),
        ))
        .await
        .expect("response");
    assert_eq!(patched.status(), StatusCode::OK);
    let patched = json_body(patched).await;
    assert_eq!(patched["display_name"], "Renamed");
    assert_eq!(patched["enabled"], false);
    assert_eq!(patched["health_state"], "HEALTHY");
    assert_eq!(patched["base_url"], base_url);

    let listed = app
        .clone()
        .oneshot(request("GET", "/api/v1/providers", Some(USER_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(json_body(listed).await, json!([patched]));

    let deleted = app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/api/v1/providers/{provider_id}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let listed = app
        .oneshot(request("GET", "/api/v1/providers", Some(USER_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(json_body(listed).await, json!([]));

    server.abort();
}

#[tokio::test]
async fn a_failing_probe_is_recorded_instead_of_failing_the_request() {
    // Bind and drop a listener so the probe targets a port that just became free.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);

    let app = router(state());
    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/providers",
            Some(USER_TOKEN),
            Some(json!({
                "provider_definition_id": "okx",
                "display_name": "Unreachable OKX",
                "base_url": format!("http://{address}"),
                "timeout_ms": 500
            })),
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::CREATED);
    let provider_id = json_body(created).await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let probed = app
        .oneshot(request(
            "POST",
            &format!("/api/v1/providers/{provider_id}/test"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");

    assert_eq!(probed.status(), StatusCode::OK);
    let probed = json_body(probed).await;
    assert_eq!(probed["health_state"], "UNHEALTHY");
    assert!(probed["last_error_code"].is_string());
    assert_eq!(probed["last_latency_ms"], json!(null));
}

#[tokio::test]
async fn provider_errors_map_to_stable_statuses() {
    let app = router(state());

    for body in [
        json!({ "provider_definition_id": "unknown", "display_name": "Unknown" }),
        json!({
            "provider_definition_id": "binance",
            "display_name": "Insecure",
            "base_url": "http://api.binance.com"
        }),
        json!({
            "provider_definition_id": "binance",
            "display_name": "Secret",
            "base_url": "https://user:secret@api.binance.com"
        }),
        json!({
            "provider_definition_id": "binance",
            "display_name": "Untrusted host",
            "base_url": "https://example.com"
        }),
        json!({ "provider_definition_id": "binance" }),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/providers",
                Some(USER_TOKEN),
                Some(body),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    let unknown = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/providers/missing",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let unknown = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/providers/missing/test",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let agent = app
        .clone()
        .oneshot(request("GET", "/api/v1/providers", Some(AGENT_TOKEN), None))
        .await
        .expect("response");
    assert_eq!(agent.status(), StatusCode::FORBIDDEN);

    let anonymous = app
        .oneshot(request("GET", "/api/v1/providers", None, None))
        .await
        .expect("response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

fn test_candle(symbol: &str, open_time: i64, open: f64, close: f64, volume: f64) -> Candle {
    Candle::new(NewCandle {
        symbol: symbol.to_owned(),
        timeframe: "1m".to_owned(),
        market: None,
        source: "test".to_owned(),
        open,
        high: open.max(close) + 1.0,
        low: open.min(close) - 1.0,
        close,
        volume,
        open_time,
        close_time: open_time + 60_000,
        event_time: None,
        ingest_time: None,
    })
    .expect("valid candle")
}

#[tokio::test]
async fn market_overview_reports_empty_partial_stale_and_ranked_movers() {
    let state = state();
    let app = router(state.clone());

    let empty = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/market/overview?symbols=BTCUSDT&bars=2",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(empty.status(), StatusCode::OK);
    let empty = json_body(empty).await;
    assert_eq!(empty["state"], "EMPTY");
    assert_eq!(empty["symbols"][0]["state"], "EMPTY");
    assert_eq!(empty["symbols"][0]["candle_count"], 0);

    let now = chrono::Utc::now().timestamp_millis();
    let open_time = now - 180_000;
    state
        .candles
        .upsert_closed(&[
            test_candle("BTCUSDT", open_time, 100.0, 102.0, 2.0),
            test_candle("BTCUSDT", open_time + 60_000, 102.0, 120.0, 10.0),
        ])
        .await
        .expect("seed candles");

    let partial = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/market/overview?symbols=BTCUSDT&bars=3",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(partial.status(), StatusCode::OK);
    let partial = json_body(partial).await;
    assert_eq!(partial["state"], "PARTIAL");
    assert_eq!(partial["symbols"][0]["last_close"], 120.0);
    assert_eq!(partial["symbols"][0]["change_percent"], 20.0);

    let pulse = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/market/pulse?symbols=BTCUSDT&bars=2",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(pulse.status(), StatusCode::OK);
    let pulse = json_body(pulse).await;
    assert_eq!(pulse["state"], "OK");
    assert_eq!(pulse["movers"][0]["symbol"], "BTCUSDT");
    assert_eq!(pulse["movers"][0]["volume_ratio"], 5.0);

    state
        .candles
        .upsert_closed(&[test_candle("ETHUSDT", 1_600_000_000_000, 10.0, 11.0, 1.0)])
        .await
        .expect("seed stale candle");

    let stale = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/market/overview?symbols=BTCUSDT,ETHUSDT&bars=2",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(stale.status(), StatusCode::OK);
    assert_eq!(json_body(stale).await["state"], "STALE");

    let unsupported = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/market/overview?symbols=BTCUSDT&timeframe=1y",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(unsupported.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let agent = app
        .oneshot(request(
            "GET",
            "/api/v1/market/overview?symbols=BTCUSDT",
            Some(AGENT_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(agent.status(), StatusCode::FORBIDDEN);
}

fn research_event(
    owner_scope: &str,
    symbol: &str,
    event_type: &str,
    detected_at: DateTime<Utc>,
) -> ResearchEvent {
    ResearchEvent::new(NewResearchEvent {
        owner_scope: owner_scope.to_owned(),
        symbol: symbol.to_owned(),
        timeframe: "15m".to_owned(),
        method: "vsa".to_owned(),
        method_version: "1".to_owned(),
        event_type: event_type.to_owned(),
        direction: 1,
        score: 0.75,
        observed_at: detected_at,
        detected_at,
        ingested_at: detected_at,
        source_id: "binance".to_owned(),
        provider_method: "binance.klines".to_owned(),
        input_snapshot_id: "rsnap1_0123456789abcdef".to_owned(),
        quality_state: ResearchQuality::Complete,
        quality_reasons: vec!["fixture".to_owned()],
        evidence: BTreeMap::from([("close".to_owned(), 100.5)]),
        context: BTreeMap::from([("bucket".to_owned(), "TIM".to_owned())]),
    })
    .expect("valid research event")
}

#[tokio::test]
async fn research_events_are_owner_scoped_bounded_and_carry_provenance() {
    let state = state();
    let app = router(state.clone());
    let now = Utc::now();
    let fresh = research_event(
        "test-operator",
        "BTCUSDT",
        "ST LONG",
        now - Duration::minutes(1),
    );
    let stale = research_event("test-operator", "BTCUSDT", "BC", now - Duration::hours(2));
    let foreign = research_event("another-operator", "BTCUSDT", "SV", now);
    state
        .research_events
        .upsert(&[fresh.clone(), stale.clone(), foreign.clone()])
        .await
        .expect("seed research events");

    let list = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/research/events?symbol=BTCUSDT&timeframe=15m&limit=10",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    let list = json_body(list).await;
    assert_eq!(list["count"], 2);
    assert_eq!(list["events"][0]["id"], fresh.id());
    assert_eq!(list["events"][0]["owner_scope"], "test-operator");
    assert_eq!(list["events"][0]["quality_state"], "COMPLETE");
    assert_eq!(list["events"][0]["freshness"], "FRESH");
    assert_eq!(list["events"][0]["evidence"]["close"], 100.5);
    assert_eq!(list["events"][0]["context"]["bucket"], "TIM");

    let stale = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/research/events?event_type=BC",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(stale.status(), StatusCode::OK);
    let stale = json_body(stale).await;
    assert_eq!(stale["count"], 1);
    assert_eq!(stale["events"][0]["freshness"], "STALE");

    let detail = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/research/events/{}", fresh.id()),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(detail.status(), StatusCode::OK);
    assert_eq!(
        json_body(detail).await["input_snapshot_id"],
        "rsnap1_0123456789abcdef"
    );

    let foreign = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/research/events/{}", foreign.id()),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);

    let invalid_limit = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/research/events?limit=0",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_limit.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let agent = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/research/events",
            Some(AGENT_TOKEN),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(agent.status(), StatusCode::FORBIDDEN);

    let anonymous = app
        .oneshot(request("GET", "/api/v1/research/events", None, None))
        .await
        .expect("response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}
