mod ai_models;
mod ai_providers;
mod ai_task_routes;
mod auth;
mod error;
mod market;
mod news;
mod research;
mod routes;
mod state;

use axum::Router;
use axum::routing::{get, post};
use utoipa::OpenApi;

use tm_trading_contracts::ApiDoc;

pub use auth::{
    AuthContext, CAPABILITY_MARKET_READ, CAPABILITY_NEWS_READ, CAPABILITY_OPERATION_READ,
    CAPABILITY_PROVIDER_READ, CAPABILITY_PROVIDER_WRITE, CAPABILITY_RESEARCH_READ,
    CAPABILITY_SERVICE_CONTROL, CAPABILITY_SERVICE_READ, CAPABILITY_WATCHLIST_READ,
    CAPABILITY_WATCHLIST_WRITE, Principal,
};
pub use error::ApiError;
pub use state::{AppState, AuthConfig, Repositories};

/// Builds the Management API router for the shared `/api/v1` contract.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(routes::health))
        .route("/api/v1/version", get(routes::version))
        .route("/api/v1/openapi.json", get(openapi_document))
        .route("/api/v1/services", get(routes::list_services))
        .route(
            "/api/v1/services/{service_instance_id}",
            get(routes::get_service),
        )
        .route(
            "/api/v1/services/{service_instance_id}/operations",
            post(routes::create_service_operation),
        )
        .route(
            "/api/v1/operations/{operation_id}",
            get(routes::get_operation),
        )
        .route(
            "/api/v1/watchlists",
            get(routes::list_watchlists).post(routes::create_watchlist),
        )
        .route(
            "/api/v1/watchlists/{watchlist_id}",
            axum::routing::delete(routes::delete_watchlist),
        )
        .route(
            "/api/v1/watchlists/{watchlist_id}/symbols",
            axum::routing::put(routes::replace_watchlist_symbols),
        )
        .route(
            "/api/v1/providers/catalog",
            get(routes::list_provider_catalog),
        )
        .route(
            "/api/v1/providers",
            get(routes::list_provider_instances).post(routes::create_provider_instance),
        )
        .route(
            "/api/v1/providers/{provider_instance_id}",
            get(routes::get_provider_instance)
                .patch(routes::update_provider_instance)
                .delete(routes::delete_provider_instance),
        )
        .route(
            "/api/v1/providers/{provider_instance_id}/test",
            post(routes::test_provider_instance),
        )
        .route("/api/v1/market/overview", get(market::market_overview))
        .route("/api/v1/market/pulse", get(market::market_pulse))
        .route(
            "/api/v1/research/events",
            get(research::list_research_events),
        )
        .route(
            "/api/v1/research/events/{event_id}",
            get(research::get_research_event),
        )
        .route("/api/v1/news", get(news::list_news))
        .route("/api/v1/news/sources", get(news::list_news_sources))
        .route("/api/v1/news/clusters", get(news::list_news_clusters))
        .route("/api/v1/news/{news_id}", get(news::get_news_item))
        .route(
            "/api/v1/ai-providers",
            get(ai_providers::list_ai_provider_profiles)
                .post(ai_providers::create_ai_provider_profile),
        )
        .route(
            "/api/v1/ai-providers/{id}",
            get(ai_providers::get_ai_provider_profile)
                .patch(ai_providers::update_ai_provider_profile)
                .delete(ai_providers::delete_ai_provider_profile),
        )
        .route(
            "/api/v1/ai-providers/{id}/test",
            post(ai_providers::test_ai_provider_profile),
        )
        .route(
            "/api/v1/ai-providers/{id}/models",
            post(ai_models::create_ai_model),
        )
        .route("/api/v1/ai-models", get(ai_models::list_ai_models))
        .route(
            "/api/v1/ai-models/{id}/test",
            post(ai_models::test_ai_model),
        )
        .route(
            "/api/v1/ai-models/{id}",
            get(ai_models::get_ai_model)
                .patch(ai_models::update_ai_model)
                .delete(ai_models::delete_ai_model),
        )
        .route(
            "/api/v1/ai-task-routes",
            get(ai_task_routes::list_ai_task_routes),
        )
        .route(
            "/api/v1/ai-task-routes/{task}",
            get(ai_task_routes::get_ai_task_route)
                .patch(ai_task_routes::update_ai_task_route)
                .delete(ai_task_routes::delete_ai_task_route),
        )
        .route(
            "/api/v1/ai-task-routes",
            post(ai_task_routes::create_ai_task_route),
        )
        .route("/api/v1/agent/heartbeat", post(routes::agent_heartbeat))
        .route(
            "/api/v1/agent/commands/claim",
            post(routes::agent_claim_command),
        )
        .route(
            "/api/v1/agent/commands/{operation_id}/progress",
            post(routes::agent_progress_command),
        )
        .route(
            "/api/v1/agent/commands/{operation_id}/complete",
            post(routes::agent_complete_command),
        )
        .with_state(state)
}

async fn openapi_document() -> axum::Json<utoipa::openapi::OpenApi> {
    axum::Json(ApiDoc::openapi())
}
