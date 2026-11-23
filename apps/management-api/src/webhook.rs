//! Inbound webhook handler for TradingView alerts.
//!
//! Ported faithfully from legacy `server/webhook.mjs`.
//! Validates action, price, SL, TP, and timestamp, performs deduplication, and
//! enqueues alerts into [`NotificationOutboxRepository`] for dispatch by the notification worker.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use serde::Deserialize;
use tm_trading_contracts::{TradingViewAlertPayload, WebhookResponse};
use tm_trading_domain::{
    NotificationChannel, NotificationOutboxItem, NotificationPriority,
};
use tm_trading_storage::EnqueueNotificationOutcome;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WebhookQuery {
    pub token: Option<String>,
}

const VALID_ACTIONS: &[&str] = &["ENTRY", "TAKE_PROFIT", "STOP_LOSS", "TIME_CLOSE"];
const VALID_SIDES: &[&str] = &["BUY", "SELL"];

/// Authenticates the webhook token via path parameter, query parameter, or header.
fn verify_webhook_token(
    path_token: Option<&str>,
    query_token: Option<&str>,
    headers: &HeaderMap,
    expected_token: &str,
) -> bool {
    if expected_token.is_empty() {
        return true; // No token configured in local dev
    }

    if let Some(token) = path_token
        && token == expected_token
    {
        return true;
    }

    if let Some(token) = query_token
        && token == expected_token
    {
        return true;
    }

    if let Some(auth) = headers.get("authorization").and_then(|h| h.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
        && token.trim() == expected_token
    {
        return true;
    }

    if let Some(x_token) = headers.get("x-tm-token").and_then(|h| h.to_str().ok())
        && x_token.trim() == expected_token
    {
        return true;
    }

    false
}

/// Validates TradingView alert payload fields and SL/TP logic.
fn validate_payload(payload: &TradingViewAlertPayload) -> Result<(), &'static str> {
    if payload.v != 1 {
        return Err("unsupported schema version (expected v=1)");
    }
    if !VALID_ACTIONS.contains(&payload.action.as_str()) {
        return Err("invalid action (expected ENTRY, TAKE_PROFIT, STOP_LOSS, TIME_CLOSE)");
    }
    if !VALID_SIDES.contains(&payload.side.as_str()) {
        return Err("invalid side (expected BUY or SELL)");
    }
    if payload.price <= 0.0 || !payload.price.is_finite() {
        return Err("invalid entry price");
    }
    if !payload.sl.is_finite() {
        return Err("invalid stop loss price");
    }
    if payload.tps.is_empty() {
        return Err("tps array cannot be empty");
    }

    // Directional stop-loss check
    if payload.side == "BUY" && payload.sl >= payload.price {
        return Err("BUY order requires stop-loss below entry price");
    }
    if payload.side == "SELL" && payload.sl <= payload.price {
        return Err("SELL order requires stop-loss above entry price");
    }

    // Directional take-profit checks
    for tp in &payload.tps {
        if !tp.is_finite() {
            return Err("take-profit value must be finite");
        }
        if payload.side == "BUY" && *tp <= payload.price {
            return Err("BUY take-profit must be above entry price");
        }
        if payload.side == "SELL" && *tp >= payload.price {
            return Err("SELL take-profit must be below entry price");
        }
    }

    Ok(())
}

/// Inbound TradingView webhook intake route (`POST /api/v1/webhooks/tradingview` and `/tm-alert`).
pub async fn handle_tradingview_webhook(
    State(state): State<AppState>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    Json(payload): Json<TradingViewAlertPayload>,
) -> Result<(StatusCode, Json<WebhookResponse>), ApiError> {
    process_webhook(state, None, query.token.as_deref(), headers, payload).await
}

/// Inbound TradingView webhook intake route with path token (`POST /tm-alert/{token}`).
pub async fn handle_tradingview_webhook_with_token(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    Json(payload): Json<TradingViewAlertPayload>,
) -> Result<(StatusCode, Json<WebhookResponse>), ApiError> {
    process_webhook(state, Some(&token), query.token.as_deref(), headers, payload).await
}

async fn process_webhook(
    state: AppState,
    path_token: Option<&str>,
    query_token: Option<&str>,
    headers: HeaderMap,
    payload: TradingViewAlertPayload,
) -> Result<(StatusCode, Json<WebhookResponse>), ApiError> {
    let expected_token = std::env::var("TM_TOKEN").unwrap_or_default();
    if !verify_webhook_token(path_token, query_token, &headers, &expected_token) {
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(WebhookResponse {
                ok: false,
                deduplicated: false,
                notify_id: None,
                error: Some("invalid or missing webhook authentication token".to_owned()),
            }),
        ));
    }

    if let Err(reason) = validate_payload(&payload) {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(WebhookResponse {
                ok: false,
                deduplicated: false,
                notify_id: None,
                error: Some(reason.to_owned()),
            }),
        ));
    }

    let now = Utc::now();
    let notify_id = format!(
        "tv:{}:{}:{}",
        payload.symbol,
        payload.action,
        payload.ts.replace(':', "-")
    );

    let priority = match payload.action.as_str() {
        "STOP_LOSS" => NotificationPriority::Critical,
        "TAKE_PROFIT" => NotificationPriority::Important,
        _ => NotificationPriority::Normal,
    };

    let mut item = match NotificationOutboxItem::new(
        notify_id.clone(),
        NotificationChannel::Telegram,
        "tradingview_alert".to_owned(),
        priority,
        now,
        now,
    ) {
        Ok(item) => item,
        Err(err) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(WebhookResponse {
                    ok: false,
                    deduplicated: false,
                    notify_id: None,
                    error: Some(err.to_string()),
                }),
            ));
        }
    };

    let tps_str = payload
        .tps
        .iter()
        .map(|t| format!("${:.2}", t))
        .collect::<Vec<_>>()
        .join(", ");

    item.symbol = Some(payload.symbol.clone());
    item.title = Some(format!(
        "🎯 TradingView Alert [{} - {}]",
        payload.symbol, payload.action
    ));
    item.body = Some(format!(
        "• Symbol: {}\n• Action: {} ({})\n• Entry Price: ${:.2}\n• Stop Loss: ${:.2}\n• Take Profit: {}\n• Strategy: {}\n• Timestamp: {}",
        payload.symbol,
        payload.action,
        payload.side,
        payload.price,
        payload.sl,
        tps_str,
        payload.strategy.as_deref().unwrap_or("TradingView PineScript"),
        payload.ts
    ));

    let enqueue_outcome = state
        .notification_outbox
        .enqueue(item)
        .await
        .map_err(ApiError::from)?;

    let (deduplicated, status) = match enqueue_outcome {
        EnqueueNotificationOutcome::Enqueued(_) => (false, StatusCode::OK),
        EnqueueNotificationOutcome::Existing(_) => (true, StatusCode::OK),
    };

    Ok((
        status,
        Json(WebhookResponse {
            ok: true,
            deduplicated,
            notify_id: Some(notify_id),
            error: None,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_correct_tradingview_payload() {
        let payload = TradingViewAlertPayload {
            v: 1,
            action: "ENTRY".to_owned(),
            side: "BUY".to_owned(),
            price: 65000.0,
            sl: 63000.0,
            tps: vec![68000.0, 72000.0],
            ts: "2026-10-01T03:17:21Z".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: Some("1h".to_owned()),
            strategy: Some("SMC-Breakout".to_owned()),
        };

        assert!(validate_payload(&payload).is_ok());
    }

    #[test]
    fn rejects_invalid_sl_side() {
        let payload = TradingViewAlertPayload {
            v: 1,
            action: "ENTRY".to_owned(),
            side: "BUY".to_owned(),
            price: 65000.0,
            sl: 66000.0, // Invalid: BUY stop loss above entry price
            tps: vec![68000.0],
            ts: "2026-10-01T03:17:21Z".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: None,
            strategy: None,
        };

        assert!(validate_payload(&payload).is_err());
    }
}
