use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

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
