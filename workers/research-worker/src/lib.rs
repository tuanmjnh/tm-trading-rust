//! Research worker core: deterministic scans over stored closed candles.
//!
//! The worker consumes only persisted closed bars, runs the deterministic VSA method,
//! and stores observations with source, input, producer, schema, and quality provenance.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use thiserror::Error;
use tm_trading_domain::{
    Candle, NewResearchEvent, ResearchEvent, ResearchEventError, ResearchQuality, input_snapshot_id,
};
use tm_trading_market_providers::timeframe_millis;
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::methods::{VSA_ID, VsaEventKind, VsaParams, analyze_vsa};
use tm_trading_storage::{CandleRepository, RepositoryError, ResearchEventRepository};

/// Version of the VSA observation contract produced by this worker.
pub const METHOD_VERSION: &str = "1";

/// Configuration for one deterministic research scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchWorkerConfig {
    /// Owner scope written to every produced observation.
    pub owner_scope: String,
    /// Symbols scanned in each cycle.
    pub symbols: Vec<String>,
    /// Canonical timeframe, for example `1m` or `4h`.
    pub timeframe: String,
    /// Maximum number of closed candles read for each symbol.
    pub bars: usize,
    /// Delay between two scan cycles.
    pub cycle_interval: Duration,
    /// Delay between two heartbeats.
    pub heartbeat_interval: Duration,
    /// Deadline for one complete scan cycle.
    pub cycle_timeout: Duration,
    /// Whether this installation is the sole writer for research events.
    ///
    /// The default is `false` so a Rust worker cannot dual-write with the legacy
    /// producer during cutover.
    pub source_of_truth: bool,
}

/// Result of one complete research scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResearchScanOutcome {
    pub scanned_symbols: usize,
    pub scanned_candles: usize,
    pub detected_events: usize,
    pub inserted_events: usize,
    pub existing_events: usize,
}

/// Errors that prevent a research scan from completing successfully.
#[derive(Debug, Error)]
pub enum ResearchWorkerError {
    #[error(transparent)]
    Storage(#[from] RepositoryError),
    #[error(transparent)]
    Event(#[from] ResearchEventError),
    #[error("unsupported research timeframe '{0}'")]
    InvalidTimeframe(String),
    #[error("the research worker is not the source of truth for research event writes")]
    WriteAuthorityDisabled,
    #[error("no symbols were configured for the research worker")]
    NoSymbols,
    #[error("no closed candles were available for symbol '{symbol}'")]
    NoClosedCandles { symbol: String },
    #[error("the latest closed candle for symbol '{symbol}' is {age_seconds} seconds old")]
    StaleInput { symbol: String, age_seconds: i64 },
    #[error("symbol '{symbol}' contains a candle that is not closed at the scan time")]
    InputNotClosed { symbol: String },
    #[error("symbol '{symbol}' contains an invalid timestamp")]
    InvalidTimestamp { symbol: String },
}

/// Scans the configured symbols and stores the observations found in their closed candles.
///
/// The complete window is analyzed before anything is written, so one invalid symbol
/// cannot leave a partially published cycle behind.
pub async fn scan_research_events(
    candle_repository: &dyn CandleRepository,
    event_repository: &dyn ResearchEventRepository,
    config: &ResearchWorkerConfig,
    now: DateTime<Utc>,
) -> Result<ResearchScanOutcome, ResearchWorkerError> {
    if !config.source_of_truth {
        return Err(ResearchWorkerError::WriteAuthorityDisabled);
    }
    if config.symbols.is_empty() {
        return Err(ResearchWorkerError::NoSymbols);
    }
    let timeframe_millis = timeframe_millis(&config.timeframe)
        .map_err(|_| ResearchWorkerError::InvalidTimeframe(config.timeframe.clone()))?;
    let max_input_age_seconds = (timeframe_millis / 2_000).max(60);

    let mut outcome = ResearchScanOutcome::default();
    let mut events = Vec::new();

    for symbol in &config.symbols {
        let mut candles = candle_repository
            .recent(symbol, &config.timeframe, config.bars)
            .await?;
        if candles.is_empty() {
            return Err(ResearchWorkerError::NoClosedCandles {
                symbol: symbol.clone(),
            });
        }
        candles.sort_by_key(Candle::open_time);

        if candles
            .iter()
            .any(|candle| candle.close_time() > now.timestamp_millis())
        {
            return Err(ResearchWorkerError::InputNotClosed {
                symbol: symbol.clone(),
            });
        }

        let latest = candles.last().expect("window is not empty");
        let latest_close = timestamp_for(symbol, latest.close_time())?;
        let age_seconds = (now - latest_close).num_seconds();
        if age_seconds > max_input_age_seconds {
            return Err(ResearchWorkerError::StaleInput {
                symbol: symbol.clone(),
                age_seconds,
            });
        }

        let bars: Vec<Bar> = candles
            .iter()
            .map(|candle| Bar {
                time: candle.open_time(),
                open: candle.open(),
                high: candle.high(),
                low: candle.low(),
                close: candle.close(),
                volume: candle.volume(),
            })
            .collect();
        let analysis = analyze_vsa(&bars, &VsaParams::default());
        let snapshot_id = input_snapshot_id(
            symbol,
            &config.timeframe,
            VSA_ID,
            METHOD_VERSION,
            candles.first().expect("window is not empty").open_time(),
            latest.open_time(),
            candles.len(),
        );

        for event in &analysis.events {
            let candle = &candles[event.bar];
            let observed_at = timestamp_for(symbol, candle.close_time())?;
            let source_id = candle.source();
            let mut evidence = BTreeMap::from([
                ("open".to_owned(), candle.open()),
                ("high".to_owned(), candle.high()),
                ("low".to_owned(), candle.low()),
                ("close".to_owned(), candle.close()),
                ("volume".to_owned(), candle.volume()),
                (
                    "volume_ratio".to_owned(),
                    analysis.series.volume_ratio[event.bar],
                ),
                ("score".to_owned(), event.kind.score()),
            ]);
            if let Some(atr) = analysis.series.atr[event.bar] {
                evidence.insert("atr".to_owned(), atr);
            }
            if let Some(average) = analysis.series.volume_average[event.bar] {
                evidence.insert("volume_average".to_owned(), average);
            }

            let mut quality_reasons = Vec::new();
            if analysis.series.atr[event.bar].is_none() {
                quality_reasons.push("atr_unavailable".to_owned());
            }
            if analysis.series.volume_average[event.bar].is_none() {
                quality_reasons.push("volume_average_unavailable".to_owned());
            }
            let quality_state = if quality_reasons.is_empty() {
                ResearchQuality::Complete
            } else {
                ResearchQuality::Partial
            };

            events.push(ResearchEvent::new(NewResearchEvent {
                owner_scope: config.owner_scope.clone(),
                symbol: symbol.clone(),
                timeframe: config.timeframe.clone(),
                method: VSA_ID.to_owned(),
                method_version: METHOD_VERSION.to_owned(),
                event_type: event.kind.label().to_owned(),
                direction: direction(event.kind),
                score: event.kind.score(),
                observed_at,
                detected_at: observed_at,
                ingested_at: now,
                source_id: source_id.to_owned(),
                provider_method: provider_method(source_id),
                input_snapshot_id: snapshot_id.clone(),
                quality_state,
                quality_reasons,
                evidence,
                context: BTreeMap::from([
                    ("event".to_owned(), event.kind.label().to_owned()),
                    (
                        "volume_bucket".to_owned(),
                        analysis.series.bucket[event.bar].label().to_owned(),
                    ),
                ]),
            })?);
        }

        outcome.scanned_symbols += 1;
        outcome.scanned_candles += candles.len();
    }

    outcome.detected_events = events.len();
    let stored = event_repository.upsert(&events).await?;
    outcome.inserted_events = stored.inserted;
    outcome.existing_events = stored.existing;
    Ok(outcome)
}

fn timestamp_for(symbol: &str, millis: i64) -> Result<DateTime<Utc>, ResearchWorkerError> {
    Utc.timestamp_millis_opt(millis)
        .single()
        .ok_or_else(|| ResearchWorkerError::InvalidTimestamp {
            symbol: symbol.to_owned(),
        })
}

fn direction(kind: VsaEventKind) -> i8 {
    match kind {
        VsaEventKind::Sv | VsaEventKind::StLong | VsaEventKind::Ns => 1,
        VsaEventKind::Bc | VsaEventKind::StShort | VsaEventKind::Nd => -1,
    }
}

fn provider_method(source: &str) -> String {
    match source {
        "binance" => "binance.klines".to_owned(),
        "okx" => "okx.market.candles".to_owned(),
        other => format!("{other}.closed_candles"),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::Value;
    use tm_trading_domain::{Candle, NewCandle, ResearchQuality};
    use tm_trading_storage::{
        InMemoryCandleRepository, InMemoryResearchEventRepository, ResearchEventQuery,
    };

    use super::*;

    const VECTORS: &str =
        include_str!("../../../crates/research-engine/tests/fixtures/vsa_vectors.json");

    fn config() -> ResearchWorkerConfig {
        ResearchWorkerConfig {
            owner_scope: "test-operator".to_owned(),
            symbols: vec!["BTCUSDT".to_owned()],
            timeframe: "1m".to_owned(),
            bars: 120,
            cycle_interval: std::time::Duration::from_secs(60),
            heartbeat_interval: std::time::Duration::from_secs(5),
            cycle_timeout: std::time::Duration::from_secs(30),
            source_of_truth: true,
        }
    }

    fn fixture_candles() -> Vec<Candle> {
        let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
        fixture["bars"]
            .as_array()
            .expect("bars")
            .iter()
            .map(|bar| {
                let open_time = bar["time"].as_i64().expect("time");
                let open = bar["open"].as_f64().expect("open");
                let high = bar["high"].as_f64().expect("high");
                let low = bar["low"].as_f64().expect("low");
                let close = bar["close"].as_f64().expect("close");
                Candle::new(NewCandle {
                    symbol: "BTCUSDT".to_owned(),
                    timeframe: "1m".to_owned(),
                    market: Some("spot".to_owned()),
                    source: "binance".to_owned(),
                    open,
                    high: high.max(open).max(close),
                    low: low.min(open).min(close),
                    close,
                    volume: bar["volume"].as_f64().expect("volume"),
                    open_time,
                    close_time: open_time + 59_999,
                    event_time: None,
                    ingest_time: None,
                })
                .expect("valid candle")
            })
            .collect()
    }

    fn latest_time(candles: &[Candle]) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(candles.last().expect("last candle").close_time())
            .single()
            .expect("valid timestamp")
    }

    #[tokio::test]
    async fn a_closed_candle_window_produces_attributed_events() {
        let candles = fixture_candles();
        let now = latest_time(&candles) + chrono::Duration::seconds(1);
        let candle_repository = InMemoryCandleRepository::new();
        candle_repository
            .upsert_closed(&candles)
            .await
            .expect("seed candles");
        let event_repository = InMemoryResearchEventRepository::new();

        let outcome = scan_research_events(&candle_repository, &event_repository, &config(), now)
            .await
            .expect("scan succeeds");

        assert_eq!(outcome.scanned_symbols, 1);
        assert_eq!(outcome.scanned_candles, 120);
        assert_eq!(outcome.detected_events, 5);
        assert_eq!(outcome.inserted_events, 5);
        assert_eq!(outcome.existing_events, 0);

        let stored = event_repository
            .list(&ResearchEventQuery {
                owner_scope: Some("test-operator".to_owned()),
                ..ResearchEventQuery::default()
            })
            .await
            .expect("list events");
        assert_eq!(stored.len(), 5);
        let event = stored
            .iter()
            .find(|event| event.event_type() == "ST LONG")
            .expect("long event");
        assert_eq!(event.method(), "vsa");
        assert_eq!(event.method_version(), "1");
        assert_eq!(event.source_id(), "binance");
        assert_eq!(event.provider_method(), "binance.klines");
        assert!(event.input_snapshot_id().starts_with("rsnap1_"));
        assert!(!event.producer_version().is_empty());
        assert!(!event.schema_version().is_empty());
        assert_eq!(event.quality_state(), ResearchQuality::Complete);
        assert!(event.evidence().contains_key("close"));
        assert!(event.evidence().contains_key("volume_ratio"));
        assert!(event.context().contains_key("volume_bucket"));
    }

    #[tokio::test]
    async fn replaying_the_same_window_does_not_duplicate_events() {
        let candles = fixture_candles();
        let now = latest_time(&candles) + chrono::Duration::seconds(1);
        let candle_repository = InMemoryCandleRepository::new();
        candle_repository
            .upsert_closed(&candles)
            .await
            .expect("seed candles");
        let event_repository = InMemoryResearchEventRepository::new();

        scan_research_events(&candle_repository, &event_repository, &config(), now)
            .await
            .expect("first scan succeeds");
        let second = scan_research_events(&candle_repository, &event_repository, &config(), now)
            .await
            .expect("second scan succeeds");

        assert_eq!(second.detected_events, 5);
        assert_eq!(second.inserted_events, 0);
        assert_eq!(second.existing_events, 5);
        let stored = event_repository
            .list(&ResearchEventQuery {
                owner_scope: Some("test-operator".to_owned()),
                ..ResearchEventQuery::default()
            })
            .await
            .expect("list events");
        assert_eq!(stored.len(), 5);
    }

    #[tokio::test]
    async fn write_authority_is_required_before_any_event_is_stored() {
        let candles = fixture_candles();
        let now = latest_time(&candles) + chrono::Duration::seconds(1);
        let candle_repository = InMemoryCandleRepository::new();
        candle_repository
            .upsert_closed(&candles)
            .await
            .expect("seed candles");
        let event_repository = InMemoryResearchEventRepository::new();
        let mut settings = config();
        settings.source_of_truth = false;

        let result =
            scan_research_events(&candle_repository, &event_repository, &settings, now).await;

        assert!(matches!(
            result,
            Err(ResearchWorkerError::WriteAuthorityDisabled)
        ));
        let stored = event_repository
            .list(&ResearchEventQuery::default())
            .await
            .expect("list events");
        assert!(stored.is_empty());
    }

    #[tokio::test]
    async fn a_missing_window_is_not_a_successful_scan() {
        let candle_repository = InMemoryCandleRepository::new();
        let event_repository = InMemoryResearchEventRepository::new();

        let result =
            scan_research_events(&candle_repository, &event_repository, &config(), Utc::now())
                .await;

        assert!(matches!(
            result,
            Err(ResearchWorkerError::NoClosedCandles { symbol }) if symbol == "BTCUSDT"
        ));
    }

    #[tokio::test]
    async fn stale_input_is_rejected_before_an_event_is_written() {
        let candles = fixture_candles();
        let candle_repository = InMemoryCandleRepository::new();
        candle_repository
            .upsert_closed(&candles)
            .await
            .expect("seed candles");
        let event_repository = InMemoryResearchEventRepository::new();

        let result = scan_research_events(
            &candle_repository,
            &event_repository,
            &config(),
            latest_time(&candles) + chrono::Duration::minutes(5),
        )
        .await;

        assert!(matches!(
            result,
            Err(ResearchWorkerError::StaleInput { .. })
        ));
        let stored = event_repository
            .list(&ResearchEventQuery::default())
            .await
            .expect("list events");
        assert!(stored.is_empty());
    }
}
