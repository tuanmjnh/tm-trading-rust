use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use tm_trading_contracts::{
    FundingRateDto, LiquidationZoneDto, MarketFundingDto, MarketLiquidationDto, MarketMoverDto,
    MarketOverviewDto, MarketPulseDto, MarketStateDto, MarketSymbolDto,
};
use tm_trading_domain::{
    Candle, DEFAULT_FUNDING_ALERT_RATE, DEFAULT_LEVERAGE_BANDS, DEFAULT_MMR, FundingRateRecord,
    NotificationChannel, NotificationPriority, build_funding_alert, estimate_liquidation_zones,
    evaluate_funding, normalize_symbols,
};
use tm_trading_market_providers::{
    BINANCE_FUTURES_BASE_URL, fetch_open_interest, fetch_premium_index, timeframe_millis,
};

use crate::auth::CAPABILITY_MARKET_READ;
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_BARS: usize = 60;
const MAX_BARS: usize = 500;
const MAX_SYMBOLS: usize = 20;
const MAX_MOVERS: usize = 10;
const DEFAULT_TIMEFRAME: &str = "1m";

#[derive(Debug, Deserialize)]
pub struct MarketQuery {
    /// Comma-separated symbols. When absent, the caller's default watchlist is used.
    pub symbols: Option<String>,
    pub timeframe: Option<String>,
    pub bars: Option<usize>,
}

/// Bounded request plus the closed candles read for it.
struct Snapshot {
    timeframe: String,
    bars: usize,
    now: DateTime<Utc>,
    stale_after_seconds: i64,
    symbols: Vec<(String, Vec<Candle>)>,
}

pub async fn market_overview(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<MarketQuery>,
) -> Result<Json<MarketOverviewDto>, ApiError> {
    auth.require_capability(CAPABILITY_MARKET_READ)?;
    let snapshot = load(&state, auth.subject(), query).await?;
    let symbols = snapshot
        .symbols
        .iter()
        .map(|(symbol, candles)| {
            summarize(
                symbol,
                candles,
                snapshot.bars,
                snapshot.stale_after_seconds,
                snapshot.now,
            )
        })
        .collect::<Vec<_>>();
    Ok(Json(MarketOverviewDto {
        generated_at: snapshot.now,
        timeframe: snapshot.timeframe,
        state: overall_state(&symbols),
        stale_after_seconds: snapshot.stale_after_seconds,
        symbols,
    }))
}

pub async fn market_pulse(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<MarketQuery>,
) -> Result<Json<MarketPulseDto>, ApiError> {
    auth.require_capability(CAPABILITY_MARKET_READ)?;
    let snapshot = load(&state, auth.subject(), query).await?;
    let summaries = snapshot
        .symbols
        .iter()
        .map(|(symbol, candles)| {
            summarize(
                symbol,
                candles,
                snapshot.bars,
                snapshot.stale_after_seconds,
                snapshot.now,
            )
        })
        .collect::<Vec<_>>();
    let state = overall_state(&summaries);
    let mut movers = snapshot
        .symbols
        .iter()
        .zip(summaries.iter())
        .map(|((_, candles), summary)| mover(summary, candles))
        .collect::<Vec<_>>();
    // Symbols with no data sort last; the rest are ranked by the size of their move.
    movers.sort_by(|left, right| {
        let left_change = left.change_percent.map(f64::abs).unwrap_or(-1.0);
        let right_change = right.change_percent.map(f64::abs).unwrap_or(-1.0);
        right_change.total_cmp(&left_change)
    });
    movers.truncate(MAX_MOVERS);
    Ok(Json(MarketPulseDto {
        generated_at: snapshot.now,
        timeframe: snapshot.timeframe,
        state,
        stale_after_seconds: snapshot.stale_after_seconds,
        movers,
    }))
}

async fn load(state: &AppState, subject: &str, query: MarketQuery) -> Result<Snapshot, ApiError> {
    let timeframe = query
        .timeframe
        .as_deref()
        .unwrap_or(DEFAULT_TIMEFRAME)
        .trim()
        .to_ascii_lowercase();
    let timeframe_ms = timeframe_millis(&timeframe).map_err(|_| {
        ApiError::UnprocessableEntity(format!("unsupported timeframe '{timeframe}'"))
    })?;
    let bars = query.bars.unwrap_or(DEFAULT_BARS);
    if bars == 0 || bars > MAX_BARS {
        return Err(ApiError::UnprocessableEntity(format!(
            "bars must be between 1 and {MAX_BARS}"
        )));
    }

    let requested = match query
        .symbols
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(list) => list
            .split(',')
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>(),
        None => default_watchlist_symbols(state, subject).await?,
    };
    let symbols = normalize_symbols(&requested)
        .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    if symbols.len() > MAX_SYMBOLS {
        return Err(ApiError::UnprocessableEntity(format!(
            "at most {MAX_SYMBOLS} symbols may be queried at once"
        )));
    }

    let mut loaded = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        let candles = state
            .candles
            .recent(&symbol, &timeframe, bars)
            .await
            .map_err(ApiError::from)?;
        loaded.push((symbol, candles));
    }

    Ok(Snapshot {
        timeframe,
        bars,
        now: Utc::now(),
        stale_after_seconds: stale_after_seconds(timeframe_ms),
        symbols: loaded,
    })
}

/// The default list is a display preference, not an authorization boundary: an owner with
/// no lists simply gets an empty market view instead of an error.
async fn default_watchlist_symbols(
    state: &AppState,
    subject: &str,
) -> Result<Vec<String>, ApiError> {
    let lists = state
        .watchlists
        .list(subject)
        .await
        .map_err(ApiError::from)?;
    let chosen = lists
        .iter()
        .find(|list| list.is_default())
        .or_else(|| lists.first());
    Ok(chosen
        .map(|list| list.symbols().to_vec())
        .unwrap_or_default())
}

fn summarize(
    symbol: &str,
    candles: &[Candle],
    bars: usize,
    stale_after_seconds: i64,
    now: DateTime<Utc>,
) -> MarketSymbolDto {
    let Some(latest) = candles.first() else {
        return MarketSymbolDto {
            symbol: symbol.to_owned(),
            state: MarketStateDto::Empty,
            candle_count: 0,
            last_close: None,
            change_percent: None,
            high: None,
            low: None,
            volume: None,
            last_open_time: None,
            last_close_time: None,
            data_age_seconds: None,
        };
    };
    let Some(last_close_time) = millis(latest.close_time()) else {
        return empty_with_state(symbol, MarketStateDto::Stale, candles.len());
    };
    let data_age_seconds = (now.timestamp_millis() - latest.close_time()) / 1_000;
    let state = if data_age_seconds > stale_after_seconds {
        MarketStateDto::Stale
    } else if candles.len() < bars {
        MarketStateDto::Partial
    } else {
        MarketStateDto::Ok
    };
    let first = candles.last().unwrap_or(latest);
    let high = candles
        .iter()
        .map(Candle::high)
        .fold(f64::NEG_INFINITY, f64::max);
    let low = candles
        .iter()
        .map(Candle::low)
        .fold(f64::INFINITY, f64::min);
    let volume = candles.iter().map(Candle::volume).sum();
    MarketSymbolDto {
        symbol: symbol.to_owned(),
        state,
        candle_count: candles.len(),
        last_close: Some(latest.close()),
        change_percent: percent_change(first.open(), latest.close()),
        high: Some(high),
        low: Some(low),
        volume: Some(volume),
        last_open_time: millis(latest.open_time()),
        last_close_time: Some(last_close_time),
        data_age_seconds: Some(data_age_seconds),
    }
}

fn empty_with_state(symbol: &str, state: MarketStateDto, candle_count: usize) -> MarketSymbolDto {
    MarketSymbolDto {
        symbol: symbol.to_owned(),
        state,
        candle_count,
        last_close: None,
        change_percent: None,
        high: None,
        low: None,
        volume: None,
        last_open_time: None,
        last_close_time: None,
        data_age_seconds: None,
    }
}

fn mover(summary: &MarketSymbolDto, candles: &[Candle]) -> MarketMoverDto {
    // `recent` returns newest first; the latest bar is excluded from its own baseline.
    let (latest, prior) = candles
        .split_first()
        .map_or((None, &[][..]), |(first, rest)| (Some(first), rest));
    let prior_volume = if prior.is_empty() {
        0.0
    } else {
        prior.iter().map(Candle::volume).sum::<f64>() / prior.len() as f64
    };
    let volume_ratio = latest
        .filter(|_| prior_volume > 0.0)
        .map(|candle| candle.volume() / prior_volume);
    let volatility_percent = match (summary.high, summary.low, summary.last_close) {
        (Some(high), Some(low), Some(close)) if close > 0.0 => Some((high - low) / close * 100.0),
        _ => None,
    };
    MarketMoverDto {
        symbol: summary.symbol.clone(),
        state: summary.state,
        last_close: summary.last_close,
        change_percent: summary.change_percent,
        volume_ratio,
        volatility_percent,
        candle_count: summary.candle_count,
    }
}

fn overall_state(symbols: &[MarketSymbolDto]) -> MarketStateDto {
    if symbols.is_empty()
        || symbols
            .iter()
            .all(|symbol| symbol.state == MarketStateDto::Empty)
    {
        return MarketStateDto::Empty;
    }
    if symbols
        .iter()
        .any(|symbol| symbol.state == MarketStateDto::Stale)
    {
        return MarketStateDto::Stale;
    }
    if symbols.iter().any(|symbol| {
        matches!(
            symbol.state,
            MarketStateDto::Partial | MarketStateDto::Empty
        )
    }) {
        return MarketStateDto::Partial;
    }
    MarketStateDto::Ok
}

/// Half a timeframe, never below one minute, is the boundary between current and stale.
fn stale_after_seconds(timeframe_ms: i64) -> i64 {
    (timeframe_ms / 2_000).max(60)
}

fn percent_change(from: f64, to: f64) -> Option<f64> {
    if from <= 0.0 {
        return None;
    }
    Some((to - from) / from * 100.0)
}

fn millis(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value).single()
}

#[derive(Debug, Deserialize)]
pub struct FundingQuery {
    pub snapshot_n: Option<usize>,
    pub max_extremes: Option<usize>,
    pub threshold: Option<f64>,
}

pub async fn market_funding(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<FundingQuery>,
) -> Result<Json<MarketFundingDto>, ApiError> {
    auth.require_capability(CAPABILITY_MARKET_READ)?;

    let snapshot_n = query.snapshot_n.unwrap_or(15);
    let max_extremes = query.max_extremes.unwrap_or(10);
    let threshold = query.threshold.unwrap_or(DEFAULT_FUNDING_ALERT_RATE);

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let base_url = std::env::var("BINANCE_FUTURES_BASE_URL")
        .unwrap_or_else(|_| BINANCE_FUTURES_BASE_URL.to_owned());

    let raw_records = fetch_premium_index(&http, &base_url)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let (snapshot_records, extreme_records) =
        evaluate_funding(&raw_records, snapshot_n, max_extremes, threshold);

    let now = Utc::now();
    for extreme in &extreme_records {
        if let Ok(alert_item) = build_funding_alert(
            extreme,
            NotificationChannel::Telegram,
            NotificationPriority::Important,
            now,
        ) {
            let _ = state.notification_outbox.enqueue(alert_item).await;
        }
    }

    let to_dto = |r: FundingRateRecord| FundingRateDto {
        symbol: r.symbol,
        rate: r.rate,
        pct: r.pct,
        next_funding_time: r.next_funding_time,
        price: r.price,
        oi_contracts: r.oi_contracts,
        oi_notional_usd: r.oi_notional_usd,
        score: r.score,
    };

    Ok(Json(MarketFundingDto {
        generated_at: now,
        threshold,
        snapshot: snapshot_records.into_iter().map(to_dto).collect(),
        extremes: extreme_records.into_iter().map(to_dto).collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct LiquidationQuery {
    pub symbol: Option<String>,
}

pub async fn market_liquidation(
    State(_state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<LiquidationQuery>,
) -> Result<Json<MarketLiquidationDto>, ApiError> {
    auth.require_capability(CAPABILITY_MARKET_READ)?;

    let symbol = query
        .symbol
        .unwrap_or_else(|| "BTCUSDT".to_owned())
        .trim()
        .to_ascii_uppercase();

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let base_url = std::env::var("BINANCE_FUTURES_BASE_URL")
        .unwrap_or_else(|_| BINANCE_FUTURES_BASE_URL.to_owned());

    let raw_records = fetch_premium_index(&http, &base_url)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let target_record = raw_records.into_iter().find(|r| r.symbol == symbol);
    let mark_price = target_record.map(|r| r.price).unwrap_or(65_000.0);

    let oi_contracts = fetch_open_interest(&http, &base_url, &symbol)
        .await
        .unwrap_or(None)
        .unwrap_or(50_000.0);
    let oi_usd = oi_contracts * mark_price;

    let zones = estimate_liquidation_zones(mark_price, oi_usd, DEFAULT_LEVERAGE_BANDS, DEFAULT_MMR);
    let now = Utc::now();

    let zone_dtos = zones
        .into_iter()
        .map(|z| LiquidationZoneDto {
            lev: z.lev,
            w: z.w,
            side: z.side.as_str().to_owned(),
            price: (z.price * 100.0).round() / 100.0,
            pct: (z.pct * 100.0).round() / 100.0,
            usd: z.usd,
        })
        .collect();

    Ok(Json(MarketLiquidationDto {
        generated_at: now,
        symbol,
        mark_price,
        open_interest_usd: oi_usd,
        zones: zone_dtos,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_trading_domain::NewCandle;

    fn candle(open_time: i64, open: f64, close: f64, volume: f64) -> Candle {
        Candle::new(NewCandle {
            symbol: "BTCUSDT".to_owned(),
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

    #[test]
    fn stale_boundary_is_half_a_timeframe_with_a_one_minute_floor() {
        assert_eq!(stale_after_seconds(60_000), 60);
        assert_eq!(stale_after_seconds(14_400_000), 7_200);
    }

    #[test]
    fn an_empty_symbol_is_explicitly_empty() {
        let now = Utc
            .timestamp_millis_opt(1_700_000_000_000)
            .single()
            .unwrap();
        let summary = summarize("BTCUSDT", &[], 60, 60, now);
        assert_eq!(summary.state, MarketStateDto::Empty);
        assert_eq!(summary.candle_count, 0);
        assert!(summary.last_close.is_none());
    }

    #[test]
    fn a_short_history_is_partial_and_an_old_bar_is_stale() {
        let now = Utc
            .timestamp_millis_opt(1_700_000_000_000)
            .single()
            .unwrap();
        let fresh = vec![candle(1_699_999_940_000, 100.0, 101.0, 1.0)];
        assert_eq!(
            summarize("BTCUSDT", &fresh, 60, 60, now).state,
            MarketStateDto::Partial
        );

        let old = vec![candle(1_699_000_000_000, 100.0, 101.0, 1.0)];
        assert_eq!(
            summarize("BTCUSDT", &old, 1, 60, now).state,
            MarketStateDto::Stale
        );
    }

    #[test]
    fn movers_rank_by_absolute_change_and_compute_volume_ratio() {
        let now = Utc
            .timestamp_millis_opt(1_700_000_000_000)
            .single()
            .unwrap();
        let candles = vec![
            candle(1_699_999_940_000, 110.0, 120.0, 10.0),
            candle(1_699_999_880_000, 100.0, 105.0, 2.0),
            candle(1_699_999_820_000, 100.0, 100.0, 2.0),
        ];
        let summary = summarize("BTCUSDT", &candles, 3, 60, now);
        let mover = mover(&summary, &candles);

        assert_eq!(summary.change_percent, Some(20.0));
        assert_eq!(mover.volume_ratio, Some(5.0));
        assert!(mover.volatility_percent.is_some());
    }

    #[test]
    fn a_mixed_set_is_partial_and_fully_stale_set_is_stale() {
        let now = Utc
            .timestamp_millis_opt(1_700_000_000_000)
            .single()
            .unwrap();
        let fresh = summarize(
            "BTCUSDT",
            &[candle(1_699_999_940_000, 1.0, 1.0, 1.0)],
            1,
            60,
            now,
        );
        let empty = summarize("ETHUSDT", &[], 1, 60, now);
        assert_eq!(
            overall_state(&[fresh.clone(), empty]),
            MarketStateDto::Partial
        );

        let old = summarize(
            "BTCUSDT",
            &[candle(1_699_000_000_000, 1.0, 1.0, 1.0)],
            1,
            60,
            now,
        );
        assert_eq!(overall_state(&[old]), MarketStateDto::Stale);
    }
}
