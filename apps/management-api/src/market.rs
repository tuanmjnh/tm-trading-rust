use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use tm_trading_contracts::{
    AlertRuleEvaluationDto, ConfluenceScoreDto, CrossScanRowDto, FundingRateDto,
    LiquidationZoneDto, MacroSeriesDto, MarketFundingDto, MarketIntelligenceDto,
    MarketLiquidationDto, MarketMoverDto, MarketOverviewDto, MarketPulseDto, MarketRegimeDto,
    MarketStateDto, MarketSymbolDto, PlatformMaintenanceDto, ReconcileStatusDto, ScannerMoverDto,
};
use tm_trading_domain::{
    Candle, DEFAULT_FUNDING_ALERT_RATE, DEFAULT_LEVERAGE_BANDS, DEFAULT_MMR, FundingRateRecord,
    NewsLevel, NotificationChannel, NotificationOutboxItem, NotificationPriority,
    build_funding_alert, estimate_liquidation_zones, evaluate_funding, normalize_symbols,
};
use tm_trading_market_providers::{
    BINANCE_FUTURES_BASE_URL, fetch_open_interest, fetch_premium_index, timeframe_millis,
};
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::intelligence::{
    AlertCandidateEvent, AlertRuleDirection, AlertRuleLevel, AlertRuleSpec, CROSS_SCAN_WARNING,
    ConfluenceParts, ConfluenceWeights, DEFAULT_PRICE_TOL_PCT, DEFAULT_VOLUME_TOL_PCT,
    MACRO_SERIES_SPECS, NewsExtractCandidate, ScannerTicker, VenueSnapshot, classify_season,
    combine_score, compute_cross_spreads, match_alert_rule, parse_asi_html, parse_fng_entries,
    parse_fred_latest, pick_news_extract_eligible, pick_tickers, reconcile_bars,
    rule_event_dedupe_key, score_funding, score_regime, top_movers,
};
use tm_trading_storage::{NewsItemQuery, ResearchEventQuery};

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

/// The default list is a display preference, not an authorization boundary: when an owner
/// has no saved watchlist yet, fall back to the configured market symbols so the overview
/// and pulse endpoints remain populated out of the box.
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
    if let Some(list) = chosen {
        return Ok(list.symbols().to_vec());
    }
    let fallback = std::env::var("TM_TRADING_MARKET_SYMBOLS")
        .unwrap_or_else(|_| "BTCUSDT,ETHUSDT,SOLUSDT".to_owned());
    Ok(fallback
        .split(',')
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect())
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

pub async fn market_intelligence(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
) -> Result<Json<MarketIntelligenceDto>, ApiError> {
    auth.require_capability(CAPABILITY_MARKET_READ)?;

    const CACHE_TTL_SECONDS: i64 = 45;
    if let Some((cached_at, cached_dto)) = state.intelligence_cache.read().await.as_ref()
        && (Utc::now() - *cached_at).num_seconds() < CACHE_TTL_SECONDS
    {
        return Ok(Json(cached_dto.clone()));
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("tm-trading-rust/0.1")
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let base_url = std::env::var("TM_TRADING_BINANCE_FUTURES_URL")
        .unwrap_or_else(|_| BINANCE_FUTURES_BASE_URL.to_owned());

    let symbols = default_watchlist_symbols(&state, auth.subject())
        .await
        .unwrap_or_default();
    let symbols = normalize_symbols(&symbols).unwrap_or_default();

    let events_query = ResearchEventQuery {
        owner_scope: Some(auth.subject().to_owned()),
        limit: 100,
        ..ResearchEventQuery::default()
    };

    let (fng_res, cg_res, asi_res, ticker_res, funding_res, events_res, okx_btc_res, bybit_btc_res) = tokio::join!(
        http.get("https://api.alternative.me/fng/?limit=30").send(),
        http.get("https://api.coingecko.com/api/v3/global").send(),
        http.get("https://www.blockchaincenter.net/altcoin-season-index/")
            .send(),
        http.get(format!("{base_url}/fapi/v1/ticker/24hr")).send(),
        fetch_premium_index(&http, &base_url),
        state.research_events.list(&events_query),
        http.get("https://www.okx.com/api/v5/market/ticker?instId=BTC-USDT-SWAP")
            .send(),
        http.get("https://api.bybit.com/v5/market/tickers?category=linear&symbol=BTCUSDT")
            .send(),
    );

    let fng = match fng_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|json| {
                let arr = json.get("data")?.as_array()?;
                let owned: Vec<(u32, String)> = arr
                    .iter()
                    .filter_map(|item| {
                        let val = item
                            .get("value")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u32>().ok())?;
                        let cls = item
                            .get("value_classification")
                            .or_else(|| item.get("classification"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_owned();
                        Some((val, cls))
                    })
                    .collect();
                let refs: Vec<(u32, &str)> = owned.iter().map(|(v, s)| (*v, s.as_str())).collect();
                parse_fng_entries(&refs)
            }),
        _ => None,
    };

    let (btc_dominance, eth_dominance) = match cg_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .map(|json| {
                let pct = json
                    .get("data")
                    .and_then(|d| d.get("market_cap_percentage"));
                let btc = pct.and_then(|p| p.get("btc")).and_then(|v| v.as_f64());
                let eth = pct.and_then(|p| p.get("eth")).and_then(|v| v.as_f64());
                (btc, eth)
            })
            .unwrap_or((None, None)),
        _ => (None, None),
    };

    let asi = match asi_res {
        Ok(resp) if resp.status().is_success() => resp
            .text()
            .await
            .ok()
            .and_then(|html| parse_asi_html(&html)),
        _ => None,
    };

    let season = classify_season(asi.and_then(|a| a.d90));
    let regime = MarketRegimeDto {
        season: season.as_str().to_owned(),
        asi_d30: asi.and_then(|a| a.d30),
        asi_d90: asi.and_then(|a| a.d90),
        asi_d365: asi.and_then(|a| a.d365),
        fng_value: fng.as_ref().map(|f| f.value),
        fng_classification: fng.as_ref().map(|f| f.classification.clone()),
        fng_avg30: fng.as_ref().and_then(|f| f.avg30),
        btc_dominance,
        eth_dominance,
    };

    let scanner_movers = match ticker_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Vec<serde_json::Value>>()
            .await
            .ok()
            .map(|rows| {
                let parsed: Vec<ScannerTicker> = rows
                    .into_iter()
                    .filter_map(|r| {
                        let symbol = r.get("symbol")?.as_str()?.to_owned();
                        let pct24h = r
                            .get("priceChangePercent")?
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())?;
                        let quote_volume = r
                            .get("quoteVolume")?
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())?;
                        let last_price = r
                            .get("lastPrice")?
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())?;
                        Some(ScannerTicker {
                            symbol,
                            pct24h,
                            quote_volume,
                            last_price,
                        })
                    })
                    .collect();
                let picked = pick_tickers(parsed, 30_000_000.0);
                top_movers(picked, 8)
                    .into_iter()
                    .map(|t| ScannerMoverDto {
                        symbol: t.symbol,
                        last_price: t.last_price,
                        pct_24h: (t.pct24h * 100.0).round() / 100.0,
                        quote_volume: t.quote_volume,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let funding_records = funding_res.unwrap_or_default();
    let events = events_res.unwrap_or_default();

    let mut confluence: Vec<ConfluenceScoreDto> = symbols
        .iter()
        .map(|symbol| {
            let sym_events: Vec<f64> = events
                .iter()
                .filter(|e| e.symbol() == *symbol)
                .take(12)
                .map(|e| f64::from(e.direction()) * e.score())
                .collect();
            let method_score = if sym_events.is_empty() {
                0.0
            } else {
                (sym_events.iter().sum::<f64>() / (sym_events.len() as f64)).clamp(-1.0, 1.0)
            };
            let regime_score = score_regime(season, regime.fng_value, symbol);
            let funding_score = funding_records
                .iter()
                .find(|f| f.symbol == *symbol)
                .map_or(0.0, |f| score_funding(f.rate));
            let zone_score = 0.0;
            let total = combine_score(
                ConfluenceParts {
                    method: method_score,
                    regime: regime_score,
                    funding: funding_score,
                    zone: zone_score,
                },
                ConfluenceWeights::default(),
            );
            let round2 = |v: f64| (v * 100.0).round() / 100.0;
            let bias = if total > 0.15 {
                "BULL"
            } else if total < -0.15 {
                "BEAR"
            } else {
                "NEUTRAL"
            };
            ConfluenceScoreDto {
                symbol: symbol.clone(),
                total_score: round2(total),
                method_score: round2(method_score),
                regime_score: round2(regime_score),
                funding_score: round2(funding_score),
                zone_score,
                bias: bias.to_owned(),
            }
        })
        .collect();

    confluence.sort_by(|a, b| b.total_score.abs().total_cmp(&a.total_score.abs()));

    // Cross-exchange observed spread for BTC/USDT across Binance, OKX, Bybit
    let binance_btc_price = scanner_movers
        .iter()
        .find(|m| m.symbol == "BTCUSDT")
        .map(|m| m.last_price)
        .or_else(|| {
            funding_records
                .iter()
                .find(|f| f.symbol == "BTCUSDT")
                .map(|f| f.price)
        });
    let okx_btc_price = match okx_btc_res {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await.ok().and_then(|j| {
                j.get("data")?
                    .as_array()?
                    .first()?
                    .get("last")?
                    .as_str()?
                    .parse::<f64>()
                    .ok()
            })
        }
        _ => None,
    };
    let bybit_btc_price = match bybit_btc_res {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await.ok().and_then(|j| {
                j.get("result")?
                    .get("list")?
                    .as_array()?
                    .first()?
                    .get("lastPrice")?
                    .as_str()?
                    .parse::<f64>()
                    .ok()
            })
        }
        _ => None,
    };
    let binance_btc_funding = funding_records
        .iter()
        .find(|f| f.symbol == "BTCUSDT")
        .map(|f| f.rate);
    let cross_venues = vec![
        VenueSnapshot {
            venue: "binance".to_owned(),
            last_price: binance_btc_price,
            funding_rate: binance_btc_funding,
        },
        VenueSnapshot {
            venue: "okx".to_owned(),
            last_price: okx_btc_price,
            funding_rate: None,
        },
        VenueSnapshot {
            venue: "bybit".to_owned(),
            last_price: bybit_btc_price,
            funding_rate: None,
        },
    ];
    let cross_scans: Vec<CrossScanRowDto> = compute_cross_spreads("BTC/USDT", &cross_venues)
        .into_iter()
        .map(|s| CrossScanRowDto {
            instrument: s.instrument,
            pair: s.pair,
            abs_spread: (s.abs_spread * 100.0).round() / 100.0,
            rel_spread_pct: (s.rel_spread_pct * 1000.0).round() / 1000.0,
            funding_spread: s.funding_spread,
            warning: CROSS_SCAN_WARNING.to_owned(),
        })
        .collect();

    // Optional FRED macro observations when FRED_API_KEY is configured
    let mut macro_series = Vec::new();
    if let Ok(fred_key) = std::env::var("FRED_API_KEY")
        && !fred_key.trim().is_empty()
    {
        for spec in MACRO_SERIES_SPECS.iter().take(2) {
            let url = format!(
                "https://api.stlouisfed.org/fred/series/observations?series_id={}&api_key={}&file_type=json&sort_order=desc&limit=5",
                spec.series_id,
                fred_key.trim()
            );
            if let Ok(resp) = http.get(url).send().await
                && resp.status().is_success()
                && let Ok(json) = resp.json::<serde_json::Value>().await
                && let Some(arr) = json.get("observations").and_then(|v| v.as_array())
            {
                let pairs: Vec<(&str, &str)> = arr
                    .iter()
                    .filter_map(|o| Some((o.get("date")?.as_str()?, o.get("value")?.as_str()?)))
                    .collect();
                if let Some(obs) = parse_fred_latest(*spec, &pairs) {
                    macro_series.push(MacroSeriesDto {
                        series_id: obs.series_id,
                        name: obs.name,
                        unit: obs.unit,
                        value: obs.value,
                        date: obs.date,
                    });
                }
            }
        }
    }

    // Evaluate baseline high-conviction alert rule against recent research events
    let default_rule = AlertRuleSpec {
        id: "default-high-signal".to_owned(),
        name: "High Conviction Signal".to_owned(),
        min_level: Some(AlertRuleLevel::Med),
        dirs: vec![AlertRuleDirection::Pos, AlertRuleDirection::Neg],
        symbols: symbols.clone(),
        min_score: Some(0.5),
    };
    let mut matched_events = 0usize;
    let mut latest_match_title = None;
    let mut enqueued_this_cycle = 0usize;
    let now_utc = Utc::now();
    for ev in events.iter() {
        let dir = match ev.direction() {
            1 => Some(AlertRuleDirection::Pos),
            -1 => Some(AlertRuleDirection::Neg),
            _ => Some(AlertRuleDirection::Flat),
        };
        let candidate = AlertCandidateEvent {
            key: ev.id().to_owned(),
            symbol: Some(ev.symbol().to_owned()),
            level: Some(AlertRuleLevel::High),
            direction: dir,
            score: Some(ev.score()),
            title: format!("{} {} ({})", ev.symbol(), ev.event_type(), ev.method()),
        };
        if match_alert_rule(&default_rule, &candidate) {
            matched_events += 1;
            if latest_match_title.is_none() {
                latest_match_title = Some(candidate.title.clone());
            }
            if enqueued_this_cycle < 3
                && state.telegram_client.is_some()
                && state.default_telegram_chat_id.is_some()
            {
                let dedupe_key = rule_event_dedupe_key(&default_rule.id, &candidate.key);
                if let Ok(mut item) = NotificationOutboxItem::new(
                    dedupe_key,
                    NotificationChannel::Telegram,
                    "alert_rule".to_owned(),
                    NotificationPriority::Important,
                    now_utc,
                    now_utc,
                ) {
                    item.symbol = candidate.symbol.clone();
                    item.title = Some(format!("🔔 {}", candidate.title));
                    item.body = Some(format!(
                        "Rule: {} | Score: {:.2}",
                        default_rule.name,
                        candidate.score.unwrap_or(0.0)
                    ));
                    let _ = state.notification_outbox.enqueue(item).await;
                    enqueued_this_cycle += 1;
                }
            }
        }
    }
    let alert_evaluation = AlertRuleEvaluationDto {
        evaluated_rules: 1,
        matched_events,
        latest_match_title,
    };

    // Platform maintenance: candle reconciliation (BTCUSDT 1m) + news extraction eligibility
    let mut reconcile = Vec::new();
    let klines_url = format!("{base_url}/fapi/v1/klines?symbol=BTCUSDT&interval=1m&limit=30");
    let news_query = NewsItemQuery {
        limit: 50,
        ..NewsItemQuery::default()
    };
    let (local_btc_res, venue_btc_res, news_items_res) = tokio::join!(
        state.candles.recent("BTCUSDT", "1m", 30),
        http.get(klines_url).send(),
        state.news_items.list(&news_query),
    );
    if let (Ok(local_candles), Ok(resp)) = (local_btc_res, venue_btc_res)
        && resp.status().is_success()
        && let Ok(raw_rows) = resp.json::<Vec<Vec<serde_json::Value>>>().await
    {
        let local_bars: Vec<Bar> = local_candles
            .iter()
            .map(|c| Bar {
                time: c.open_time(),
                open: c.open(),
                high: c.high(),
                low: c.low(),
                close: c.close(),
                volume: c.volume(),
            })
            .collect();
        let venue_bars: Vec<Bar> = raw_rows
            .iter()
            .filter_map(|row| {
                let time = row.first()?.as_i64()?;
                let open = row.get(1)?.as_str()?.parse::<f64>().ok()?;
                let high = row.get(2)?.as_str()?.parse::<f64>().ok()?;
                let low = row.get(3)?.as_str()?.parse::<f64>().ok()?;
                let close = row.get(4)?.as_str()?.parse::<f64>().ok()?;
                let volume = row.get(5)?.as_str()?.parse::<f64>().ok()?;
                Some(Bar {
                    time,
                    open,
                    high,
                    low,
                    close,
                    volume,
                })
            })
            .collect();
        let report = reconcile_bars(
            &local_bars,
            &venue_bars,
            DEFAULT_PRICE_TOL_PCT,
            DEFAULT_VOLUME_TOL_PCT,
        );
        reconcile.push(ReconcileStatusDto {
            symbol: "BTCUSDT".to_owned(),
            timeframe: "1m".to_owned(),
            verdict: report.verdict.as_str().to_owned(),
            checked: report.checked,
            mismatched: report.mismatched,
            missing_local: report.missing_local,
        });
    }
    let news_extract_eligible = news_items_res
        .map(|items| {
            let candidates: Vec<NewsExtractCandidate> = items
                .iter()
                .map(|it| NewsExtractCandidate {
                    id: it.item_id().to_owned(),
                    is_high_level: it.level() == NewsLevel::High,
                    is_multi_source: false,
                    already_assessed: false,
                })
                .collect();
            pick_news_extract_eligible(&candidates, 10).len()
        })
        .unwrap_or(0);
    let platform = PlatformMaintenanceDto {
        reconcile,
        news_extract_eligible,
        retention_days: 30,
    };

    let dto = MarketIntelligenceDto {
        generated_at: Utc::now(),
        regime,
        scanner_movers,
        confluence,
        cross_scans,
        macro_series,
        alert_evaluation,
        platform,
    };
    *state.intelligence_cache.write().await = Some((Utc::now(), dto.clone()));

    Ok(Json(dto))
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
