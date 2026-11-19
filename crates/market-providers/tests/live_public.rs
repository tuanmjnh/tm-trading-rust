//! Live public-endpoint tests for the market-data adapters.
//!
//! These tests reach the public internet and run only when
//! `TM_TRADING_LIVE_PROVIDER_TESTS=1` is set. They are read-only: one health
//! request and one small candle request per provider.

use std::time::Duration;

use tm_trading_market_providers::{
    BINANCE_SPOT_BASE_URL, BinanceProvider, MarketDataProvider, OKX_BASE_URL, OkxProvider,
};

fn live() -> bool {
    std::env::var("TM_TRADING_LIVE_PROVIDER_TESTS").is_ok_and(|value| value.trim() == "1")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tokio::test]
async fn binance_returns_closed_bars_and_answers_a_health_request() {
    if !live() {
        eprintln!("skipped: TM_TRADING_LIVE_PROVIDER_TESTS is not set");
        return;
    }
    let provider = BinanceProvider::new(BINANCE_SPOT_BASE_URL, Duration::from_secs(8)).unwrap();

    let health = provider.health().await.expect("binance health");
    let candles = provider
        .closed_candles("BTCUSDT", "1m", 3, now_ms())
        .await
        .expect("binance candles");

    assert!(health.latency_ms < 8_000);
    assert_eq!(candles.len(), 3);
    assert!(candles.iter().all(|candle| candle.close_time() < now_ms()));
}

#[tokio::test]
async fn okx_returns_closed_bars_and_answers_a_health_request() {
    if !live() {
        eprintln!("skipped: TM_TRADING_LIVE_PROVIDER_TESTS is not set");
        return;
    }
    let provider = OkxProvider::new(OKX_BASE_URL, Duration::from_secs(8)).unwrap();

    let health = provider.health().await.expect("okx health");
    let candles = provider
        .closed_candles("BTC-USDT-SWAP", "1m", 3, now_ms())
        .await
        .expect("okx candles");

    assert!(health.latency_ms < 8_000);
    assert_eq!(candles.len(), 3);
    assert!(candles.iter().all(|candle| candle.close_time() < now_ms()));
}
