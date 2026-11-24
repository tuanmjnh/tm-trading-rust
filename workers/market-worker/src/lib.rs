//! Market worker: fetches closed candles from a configured public provider and stores them.
//!
//! The worker is read-only towards the provider: it never holds a trading credential and
//! never reaches an account or order endpoint. It records only bars the provider has
//! already closed, and it never reports a skipped cycle as a success.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use thiserror::Error;
use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};
use tm_trading_market_providers::{MarketDataProvider, ProviderError, timeframe_millis};
use tm_trading_storage::{CandleRepository, RepositoryError};

/// Canonical timeframe used when none is configured.
pub const DEFAULT_TIMEFRAME: &str = "1m";
/// Bars requested per symbol and cycle.
pub const DEFAULT_BARS: u32 = 100;
/// Upper bound on bars requested per symbol and cycle.
pub const MAX_BARS: u32 = 1_000;
/// Upper bound on configured symbols, so one cycle stays bounded.
pub const MAX_SYMBOLS: usize = 50;
/// Default delay between two collection cycles.
pub const DEFAULT_CYCLE_INTERVAL: Duration = Duration::from_secs(60);
/// Shortest accepted cycle delay.
pub const MIN_CYCLE_INTERVAL: Duration = Duration::from_secs(5);
/// Longest accepted cycle delay.
pub const MAX_CYCLE_INTERVAL: Duration = Duration::from_secs(300);
/// Default delay between two heartbeats.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// Shortest accepted heartbeat delay.
pub const MIN_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
/// Longest accepted heartbeat delay.
pub const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(300);
/// Default deadline for one complete collection cycle.
pub const DEFAULT_CYCLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Shortest accepted collection-cycle deadline.
pub const MIN_CYCLE_TIMEOUT: Duration = Duration::from_secs(1);
/// Longest accepted collection-cycle deadline.
pub const MAX_CYCLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Everything the worker needs to collect candles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketWorkerConfig {
    /// Symbols to collect, in the spelling the selected provider expects.
    pub symbols: Vec<String>,
    /// Canonical timeframe, for example `1m` or `4h`.
    pub timeframe: String,
    /// Bars requested per symbol and cycle.
    pub bars: u32,
    /// Delay between two collection cycles.
    pub cycle_interval: Duration,
    /// Delay between two heartbeats.
    pub heartbeat_interval: Duration,
    /// Deadline for one complete collection cycle, including provider fallback and storage.
    pub cycle_timeout: Duration,
    /// Catalog definition to use, for example `binance`. When this is not set, the worker
    /// accepts only an unambiguous enabled provider set.
    pub provider_definition_id: Option<String>,
    /// Whether this installation is the sole writer for the market candle collection.
    ///
    /// The default is `false` so a Rust worker cannot accidentally dual-write with the
    /// legacy collector during cutover.
    pub source_of_truth: bool,
}

impl MarketWorkerConfig {
    /// Reads the configuration from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a value is present but unusable. Missing values fall back to
    /// the documented defaults, and a duration outside its allowed range is clamped.
    pub fn from_env() -> Result<Self, MarketWorkerConfigError> {
        let allow_insecure_dev = std::env::var("TM_STORAGE_ALLOW_INSECURE_DEV")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        let mut symbols =
            parse_symbols(&std::env::var("TM_TRADING_MARKET_SYMBOLS").unwrap_or_default())?;
        if symbols.is_empty() && allow_insecure_dev {
            symbols = vec!["BTCUSDT".to_owned(), "ETHUSDT".to_owned()];
        }
        let timeframe = std::env::var("TM_TRADING_MARKET_TIMEFRAME")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_TIMEFRAME.to_owned());
        timeframe_millis(&timeframe).map_err(|_| MarketWorkerConfigError::InvalidTimeframe)?;
        let bars = parse_bars(&std::env::var("TM_TRADING_MARKET_BARS").unwrap_or_default())?;
        let cycle_interval = parse_duration(
            "TM_TRADING_MARKET_CYCLE_MS",
            DEFAULT_CYCLE_INTERVAL,
            MIN_CYCLE_INTERVAL,
            MAX_CYCLE_INTERVAL,
        )?;
        let heartbeat_interval = parse_duration(
            "TM_TRADING_MARKET_HEARTBEAT_MS",
            DEFAULT_HEARTBEAT_INTERVAL,
            MIN_HEARTBEAT_INTERVAL,
            MAX_HEARTBEAT_INTERVAL,
        )?;
        let cycle_timeout = parse_duration(
            "TM_TRADING_MARKET_CYCLE_TIMEOUT_MS",
            DEFAULT_CYCLE_TIMEOUT,
            MIN_CYCLE_TIMEOUT,
            MAX_CYCLE_TIMEOUT,
        )?;
        let provider_definition_id = std::env::var("TM_TRADING_MARKET_PROVIDER_ID")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let source_of_truth = parse_bool("TM_TRADING_MARKET_SOURCE_OF_TRUTH", allow_insecure_dev)?;

        Ok(Self {
            symbols,
            timeframe,
            bars,
            cycle_interval,
            heartbeat_interval,
            cycle_timeout,
            provider_definition_id,
            source_of_truth,
        })
    }
}

fn parse_symbols(value: &str) -> Result<Vec<String>, MarketWorkerConfigError> {
    let mut symbols: Vec<String> = Vec::new();
    for symbol in value.split(',') {
        let symbol = symbol.trim();
        if symbol.is_empty() {
            continue;
        }
        if !symbols.iter().any(|existing| existing == symbol) {
            symbols.push(symbol.to_owned());
        }
    }
    if symbols.len() > MAX_SYMBOLS {
        return Err(MarketWorkerConfigError::TooManySymbols);
    }
    Ok(symbols)
}

fn parse_bars(value: &str) -> Result<u32, MarketWorkerConfigError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(DEFAULT_BARS);
    }
    let bars: u32 = value
        .parse()
        .map_err(|_| MarketWorkerConfigError::InvalidBars)?;
    if bars == 0 || bars > MAX_BARS {
        return Err(MarketWorkerConfigError::InvalidBars);
    }
    Ok(bars)
}

fn parse_duration(
    name: &'static str,
    default: Duration,
    minimum: Duration,
    maximum: Duration,
) -> Result<Duration, MarketWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(default);
    }
    let millis: u64 = value
        .parse()
        .map_err(|_| MarketWorkerConfigError::InvalidDuration(name))?;
    Ok(Duration::from_millis(millis).clamp(minimum, maximum))
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, MarketWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    parse_bool_value(name, default, &value)
}

fn parse_bool_value(
    name: &'static str,
    default: bool,
    value: &str,
) -> Result<bool, MarketWorkerConfigError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(default);
    }
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(MarketWorkerConfigError::InvalidBoolean(name)),
    }
}

/// What one collection cycle produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionOutcome {
    /// Bars returned by the provider.
    pub fetched: u64,
    /// Bars written for the first time.
    pub inserted: u64,
    /// Bars that were already recorded.
    pub existing: u64,
    /// Time spent waiting for the provider.
    pub provider_ms: u64,
    /// Total time of the cycle, including storage.
    pub duration_ms: u64,
}

impl CollectionOutcome {
    /// Bars the cycle handed to the store.
    #[must_use]
    pub fn stored(&self) -> u64 {
        self.inserted + self.existing
    }
}

/// Outcome of one cycle, ready to be reported over the worker protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleResult {
    pub ok: bool,
    pub skipped: Option<String>,
    pub duration_ms: u64,
    pub output_count: Option<u64>,
    pub error: Option<String>,
    pub summary: Option<String>,
    pub dependencies_reachable: Option<bool>,
    pub sink_writable: Option<bool>,
}

impl CycleResult {
    /// A cycle that collected and stored bars.
    #[must_use]
    pub fn success(outcome: CollectionOutcome) -> Self {
        Self {
            ok: true,
            skipped: None,
            duration_ms: outcome.duration_ms,
            output_count: Some(outcome.stored()),
            error: None,
            summary: Some(format!(
                "stored {} bar(s) from {} fetched bar(s)",
                outcome.stored(),
                outcome.fetched
            )),
            dependencies_reachable: Some(true),
            sink_writable: Some(true),
        }
    }

    /// A cycle that did no work; it is not a success.
    #[must_use]
    pub fn skipped(reason: &str) -> Self {
        Self {
            ok: false,
            skipped: Some(reason.to_owned()),
            duration_ms: 0,
            output_count: None,
            error: None,
            summary: None,
            dependencies_reachable: Some(false),
            sink_writable: None,
        }
    }

    /// A cycle that could not reach the provider.
    #[must_use]
    pub fn provider_failure(message: String) -> Self {
        Self {
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: None,
            error: Some(message),
            summary: None,
            dependencies_reachable: Some(false),
            sink_writable: None,
        }
    }

    /// A cycle that could not write to the store.
    #[must_use]
    pub fn storage_failure(message: String) -> Self {
        Self {
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: None,
            error: Some(message),
            summary: None,
            dependencies_reachable: None,
            sink_writable: Some(false),
        }
    }

    /// A cycle that exceeded its overall deadline.
    #[must_use]
    pub fn timeout(message: String) -> Self {
        Self {
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: None,
            error: Some(message),
            summary: None,
            dependencies_reachable: Some(false),
            sink_writable: Some(false),
        }
    }
}

/// The worker state that produces protocol messages.
#[derive(Debug)]
pub struct MarketWorker {
    instance_id: String,
    executable_path: String,
    config: MarketWorkerConfig,
    evidence: WorkerEvidence,
}

impl MarketWorker {
    #[must_use]
    pub fn new(instance_id: &str, config: MarketWorkerConfig) -> Self {
        let executable_path = std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            instance_id: instance_id.to_owned(),
            executable_path,
            config,
            evidence: WorkerEvidence::default(),
        }
    }

    #[must_use]
    pub fn config(&self) -> &MarketWorkerConfig {
        &self.config
    }

    #[must_use]
    pub fn evidence(&self) -> &WorkerEvidence {
        &self.evidence
    }

    /// Records the outcome of the startup self-check.
    pub fn record_capability(&mut self, verified: bool) {
        self.evidence.capability_verified = Some(verified);
        self.evidence.dependencies_reachable = Some(verified);
    }

    #[must_use]
    pub fn hello(&self, pid: u32, at: DateTime<Utc>) -> WorkerMessage {
        WorkerMessage::Hello {
            instance_id: self.instance_id.clone(),
            at,
            pid,
            executable_path: self.executable_path.clone(),
            executable_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: WORKER_SCHEMA_VERSION.to_owned(),
        }
    }

    #[must_use]
    /// Reports readiness only after a successful collection cycle.
    ///
    /// The sample worker can verify a deterministic local capability at startup. The
    /// market worker cannot: its capability depends on a reachable provider and a
    /// writable store, so readiness is withheld until a real cycle succeeds.
    pub fn ready_after_success(&self, at: DateTime<Utc>) -> Option<WorkerMessage> {
        if self.evidence.capability_verified != Some(true)
            || self.evidence.last_success_at.is_none()
        {
            return None;
        }
        Some(WorkerMessage::Ready {
            instance_id: self.instance_id.clone(),
            at,
            evidence: self.evidence.clone(),
        })
    }

    #[must_use]
    pub fn heartbeat(&self, at: DateTime<Utc>) -> WorkerMessage {
        WorkerMessage::Heartbeat {
            instance_id: self.instance_id.clone(),
            at,
            evidence: self.evidence.clone(),
        }
    }

    /// Applies one cycle result to the evidence and reports it.
    pub fn cycle(&mut self, at: DateTime<Utc>, result: CycleResult) -> WorkerMessage {
        self.apply(&result, at);
        WorkerMessage::Cycle {
            instance_id: self.instance_id.clone(),
            at,
            ok: result.ok,
            skipped: result.skipped,
            duration_ms: result.duration_ms,
            output_count: result.output_count,
            evidence: self.evidence.clone(),
            error: result.error,
            summary: result.summary,
        }
    }

    /// Applies one operation result to the evidence and reports it.
    pub fn operation_result(
        &mut self,
        operation_id: &str,
        at: DateTime<Utc>,
        result: CycleResult,
    ) -> WorkerMessage {
        self.apply(&result, at);
        WorkerMessage::OperationResult {
            instance_id: self.instance_id.clone(),
            operation_id: operation_id.to_owned(),
            ok: result.ok,
            result: tm_trading_agent_protocol::WorkerResult {
                duration_ms: result.duration_ms,
                output_count: result.output_count,
                skipped: result.skipped,
                summary: result.summary,
            },
            evidence: self.evidence.clone(),
            error: result.error,
        }
    }

    #[must_use]
    pub fn stopping(&self, at: DateTime<Utc>, reason: &str) -> WorkerMessage {
        WorkerMessage::Stopping {
            instance_id: self.instance_id.clone(),
            at,
            reason: reason.to_owned(),
        }
    }

    #[must_use]
    pub fn stopped(&self, at: DateTime<Utc>, reason: &str, drained: bool) -> WorkerMessage {
        WorkerMessage::Stopped {
            instance_id: self.instance_id.clone(),
            at,
            reason: reason.to_owned(),
            drained,
        }
    }

    fn apply(&mut self, result: &CycleResult, at: DateTime<Utc>) {
        if let Some(reachable) = result.dependencies_reachable {
            self.evidence.dependencies_reachable = Some(reachable);
        }
        if let Some(writable) = result.sink_writable {
            self.evidence.sink_writable = Some(writable);
        }
        if result.ok && result.skipped.is_none() && result.error.is_none() {
            self.evidence.last_success_at = Some(at);
            self.evidence.last_output_count = result.output_count;
            self.evidence.last_event_at = Some(at);
            self.evidence.capability_verified = Some(true);
        } else if result.dependencies_reachable == Some(false)
            || result.sink_writable == Some(false)
        {
            self.evidence.capability_verified = Some(false);
        }
    }
}

/// Fetches the closed bars for every configured symbol and stores them.
///
/// A provider failure stops the cycle: a partially stored cycle would look like a
/// successful collection to the reader.
pub async fn collect_closed_candles(
    provider: &dyn MarketDataProvider,
    candles: &dyn CandleRepository,
    config: &MarketWorkerConfig,
    now_ms: i64,
) -> Result<CollectionOutcome, MarketWorkerError> {
    if !config.source_of_truth {
        return Err(MarketWorkerError::WriteAuthorityDisabled);
    }

    let started = Instant::now();
    let mut fetched = 0_u64;
    let mut provider_ms = 0_u64;
    let mut pending = Vec::new();

    // Fetch every symbol before writing anything, so a later provider failure cannot
    // leave a partially recorded cycle that looks complete.
    for symbol in &config.symbols {
        let call_started = Instant::now();
        let bars = provider
            .closed_candles(symbol, &config.timeframe, config.bars, now_ms)
            .await?;
        provider_ms += u64::try_from(call_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if bars.is_empty() {
            return Err(MarketWorkerError::EmptyResponse {
                symbol: symbol.clone(),
            });
        }
        fetched += bars.len() as u64;
        pending.extend(bars);
    }

    let stored = candles.upsert_closed(&pending).await?;

    Ok(CollectionOutcome {
        fetched,
        inserted: stored.inserted as u64,
        existing: stored.existing as u64,
        provider_ms,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[derive(Debug, Error)]
pub enum MarketWorkerError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
    #[error("the market worker is not the source of truth for candle writes")]
    WriteAuthorityDisabled,
    #[error("provider returned no closed candles for symbol '{symbol}'")]
    EmptyResponse { symbol: String },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MarketWorkerConfigError {
    #[error("TM_TRADING_MARKET_SYMBOLS lists more than {MAX_SYMBOLS} symbols")]
    TooManySymbols,
    #[error("TM_TRADING_MARKET_TIMEFRAME is not a supported timeframe")]
    InvalidTimeframe,
    #[error("TM_TRADING_MARKET_BARS must be between 1 and {MAX_BARS}")]
    InvalidBars,
    #[error("{0} is not a valid number of milliseconds")]
    InvalidDuration(&'static str),
    #[error("{0} must be a boolean value")]
    InvalidBoolean(&'static str),
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::TimeZone;
    use tm_trading_domain::{Candle, NewCandle};
    use tm_trading_market_providers::{ProviderHealth, ProviderId};
    use tm_trading_storage::InMemoryCandleRepository;

    use super::*;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
    }

    fn config(symbols: &[&str]) -> MarketWorkerConfig {
        MarketWorkerConfig {
            symbols: symbols.iter().map(|symbol| (*symbol).to_owned()).collect(),
            timeframe: DEFAULT_TIMEFRAME.to_owned(),
            bars: DEFAULT_BARS,
            cycle_interval: DEFAULT_CYCLE_INTERVAL,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            cycle_timeout: DEFAULT_CYCLE_TIMEOUT,
            provider_definition_id: None,
            source_of_truth: true,
        }
    }

    fn candle(symbol: &str, open_time: i64) -> Candle {
        Candle::new(NewCandle {
            symbol: symbol.to_owned(),
            timeframe: DEFAULT_TIMEFRAME.to_owned(),
            market: Some("spot".to_owned()),
            source: "binance".to_owned(),
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 12.5,
            open_time,
            close_time: open_time + 59_999,
            event_time: None,
            ingest_time: None,
        })
        .expect("valid candle")
    }

    struct StubProvider {
        candles: Vec<Candle>,
        fail: bool,
        fail_symbol: Option<String>,
    }

    #[async_trait]
    impl MarketDataProvider for StubProvider {
        fn id(&self) -> ProviderId {
            ProviderId::Binance
        }

        async fn closed_candles(
            &self,
            symbol: &str,
            _timeframe: &str,
            _limit: u32,
            _now_ms: i64,
        ) -> Result<Vec<Candle>, ProviderError> {
            if self.fail
                || self
                    .fail_symbol
                    .as_deref()
                    .is_some_and(|failed| failed == symbol)
            {
                return Err(ProviderError::Timeout {
                    provider: "binance",
                });
            }
            Ok(self
                .candles
                .iter()
                .filter(|candle| candle.symbol() == symbol)
                .cloned()
                .collect())
        }

        async fn health(&self) -> Result<ProviderHealth, ProviderError> {
            Ok(ProviderHealth {
                provider: ProviderId::Binance,
                latency_ms: 1,
            })
        }
    }

    #[tokio::test]
    async fn a_cycle_stores_every_fetched_closed_bar() {
        let provider = StubProvider {
            candles: vec![
                candle("BTCUSDT", 1_700_000_000_000),
                candle("BTCUSDT", 1_700_000_060_000),
            ],
            fail: false,
            fail_symbol: None,
        };
        let candles = InMemoryCandleRepository::new();

        let outcome = collect_closed_candles(
            &provider,
            &candles,
            &config(&["BTCUSDT"]),
            1_700_000_120_000,
        )
        .await
        .unwrap();

        assert_eq!(outcome.fetched, 2);
        assert_eq!(outcome.inserted, 2);
        assert_eq!(outcome.existing, 0);
        assert_eq!(outcome.stored(), 2);
        assert_eq!(candles.recent("BTCUSDT", "1m", 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn re_recording_the_same_close_keeps_one_row() {
        let provider = StubProvider {
            candles: vec![candle("BTCUSDT", 1_700_000_000_000)],
            fail: false,
            fail_symbol: None,
        };
        let candles = InMemoryCandleRepository::new();
        let settings = config(&["BTCUSDT"]);

        collect_closed_candles(&provider, &candles, &settings, 1_700_000_120_000)
            .await
            .unwrap();
        let second = collect_closed_candles(&provider, &candles, &settings, 1_700_000_120_000)
            .await
            .unwrap();

        assert_eq!(second.inserted, 0);
        assert_eq!(second.existing, 1);
        assert_eq!(candles.recent("BTCUSDT", "1m", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_provider_failure_stores_nothing() {
        let provider = StubProvider {
            candles: vec![candle("BTCUSDT", 1_700_000_000_000)],
            fail: true,
            fail_symbol: None,
        };
        let candles = InMemoryCandleRepository::new();

        let result = collect_closed_candles(
            &provider,
            &candles,
            &config(&["BTCUSDT"]),
            1_700_000_120_000,
        )
        .await;

        assert!(matches!(result, Err(MarketWorkerError::Provider(_))));
        assert!(
            candles
                .recent("BTCUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_later_symbol_failure_does_not_leave_a_partial_cycle_written() {
        let provider = StubProvider {
            candles: vec![
                candle("BTCUSDT", 1_700_000_000_000),
                candle("ETHUSDT", 1_700_000_000_000),
            ],
            fail: false,
            fail_symbol: Some("ETHUSDT".to_owned()),
        };
        let candles = InMemoryCandleRepository::new();

        let result = collect_closed_candles(
            &provider,
            &candles,
            &config(&["BTCUSDT", "ETHUSDT"]),
            1_700_000_120_000,
        )
        .await;

        assert!(matches!(result, Err(MarketWorkerError::Provider(_))));
        assert!(
            candles
                .recent("BTCUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            candles
                .recent("ETHUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn an_empty_provider_response_is_not_a_successful_no_op() {
        let provider = StubProvider {
            candles: Vec::new(),
            fail: false,
            fail_symbol: None,
        };
        let candles = InMemoryCandleRepository::new();

        let result = collect_closed_candles(
            &provider,
            &candles,
            &config(&["BTCUSDT"]),
            1_700_000_120_000,
        )
        .await;

        assert!(matches!(
            result,
            Err(MarketWorkerError::EmptyResponse { symbol }) if symbol == "BTCUSDT"
        ));
        assert!(
            candles
                .recent("BTCUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn write_authority_is_required_before_any_candle_is_stored() {
        let provider = StubProvider {
            candles: vec![candle("BTCUSDT", 1_700_000_000_000)],
            fail: false,
            fail_symbol: None,
        };
        let candles = InMemoryCandleRepository::new();
        let mut settings = config(&["BTCUSDT"]);
        settings.source_of_truth = false;

        let result =
            collect_closed_candles(&provider, &candles, &settings, 1_700_000_120_000).await;

        assert!(matches!(
            result,
            Err(MarketWorkerError::WriteAuthorityDisabled)
        ));
        assert!(
            candles
                .recent("BTCUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn symbols_are_trimmed_and_de_duplicated() {
        let symbols = parse_symbols(" BTCUSDT , ETHUSDT ,BTCUSDT, ").unwrap();

        assert_eq!(symbols, ["BTCUSDT", "ETHUSDT"]);
    }

    #[test]
    fn too_many_symbols_are_rejected() {
        let value = (0..=MAX_SYMBOLS)
            .map(|index| format!("S{index}"))
            .collect::<Vec<_>>()
            .join(",");

        assert_eq!(
            parse_symbols(&value),
            Err(MarketWorkerConfigError::TooManySymbols)
        );
    }

    #[test]
    fn bars_are_bounded() {
        assert_eq!(parse_bars("").unwrap(), DEFAULT_BARS);
        assert_eq!(parse_bars("25").unwrap(), 25);
        assert_eq!(parse_bars("0"), Err(MarketWorkerConfigError::InvalidBars));
        assert_eq!(
            parse_bars(&(MAX_BARS + 1).to_string()),
            Err(MarketWorkerConfigError::InvalidBars)
        );
        assert_eq!(
            parse_bars("many"),
            Err(MarketWorkerConfigError::InvalidBars)
        );
    }

    #[test]
    fn boolean_configuration_is_explicit() {
        assert!(!parse_bool_value("TM_TRADING_MARKET_SOURCE_OF_TRUTH", false, "").unwrap());
        assert!(parse_bool_value("TM_TRADING_MARKET_SOURCE_OF_TRUTH", false, "true").unwrap());
        assert_eq!(
            parse_bool_value("TM_TRADING_MARKET_SOURCE_OF_TRUTH", false, "maybe"),
            Err(MarketWorkerConfigError::InvalidBoolean(
                "TM_TRADING_MARKET_SOURCE_OF_TRUTH"
            ))
        );
    }

    #[test]
    fn a_skipped_cycle_is_not_a_success() {
        let result = CycleResult::skipped("no market symbols are configured");

        assert!(!result.ok);
        assert_eq!(
            result.skipped.as_deref(),
            Some("no market symbols are configured")
        );
        assert_eq!(result.output_count, None);
        assert_eq!(result.dependencies_reachable, Some(false));
    }

    #[test]
    fn readiness_is_withheld_until_a_real_cycle_succeeds() {
        let mut worker = MarketWorker::new("market-worker@local-agent:1", config(&["BTCUSDT"]));
        worker.record_capability(true);

        assert!(worker.ready_after_success(at()).is_none());

        let result = CycleResult::success(CollectionOutcome {
            fetched: 1,
            inserted: 1,
            existing: 0,
            provider_ms: 10,
            duration_ms: 12,
        });
        let WorkerMessage::Cycle { ok, .. } = worker.cycle(at(), result) else {
            panic!("expected cycle message");
        };
        assert!(ok);
        assert!(worker.ready_after_success(at()).is_some());
    }

    #[test]
    fn a_successful_cycle_records_the_evidence() {
        let mut worker = MarketWorker::new(
            "market-worker@local-agent:1",
            MarketWorkerConfig {
                symbols: vec!["BTCUSDT".to_owned()],
                timeframe: DEFAULT_TIMEFRAME.to_owned(),
                bars: DEFAULT_BARS,
                cycle_interval: DEFAULT_CYCLE_INTERVAL,
                heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
                cycle_timeout: DEFAULT_CYCLE_TIMEOUT,
                provider_definition_id: None,
                source_of_truth: true,
            },
        );

        let result = CycleResult::success(CollectionOutcome {
            fetched: 10,
            inserted: 8,
            existing: 2,
            provider_ms: 120,
            duration_ms: 160,
        });
        let WorkerMessage::Cycle { ok, evidence, .. } = worker.cycle(at(), result) else {
            panic!("expected cycle message");
        };

        assert!(ok);
        assert_eq!(evidence.last_success_at, Some(at()));
        assert_eq!(evidence.last_output_count, Some(10));
        assert_eq!(evidence.capability_verified, Some(true));
        assert_eq!(evidence.dependencies_reachable, Some(true));
        assert_eq!(evidence.sink_writable, Some(true));
    }

    #[test]
    fn a_provider_failure_revokes_readiness_after_an_earlier_success() {
        let mut worker = MarketWorker::new("market-worker@local-agent:1", config(&["BTCUSDT"]));
        worker.record_capability(true);
        worker.cycle(
            at(),
            CycleResult::success(CollectionOutcome {
                fetched: 1,
                inserted: 1,
                existing: 0,
                provider_ms: 10,
                duration_ms: 12,
            }),
        );
        assert!(worker.ready_after_success(at()).is_some());

        worker.cycle(
            at() + chrono::Duration::seconds(1),
            CycleResult::provider_failure("timeout".to_owned()),
        );

        assert!(
            worker
                .ready_after_success(at() + chrono::Duration::seconds(1))
                .is_none()
        );
        assert_eq!(worker.evidence().capability_verified, Some(false));
    }

    #[test]
    fn a_provider_failure_does_not_claim_a_success() {
        let mut worker = MarketWorker::new(
            "market-worker@local-agent:1",
            MarketWorkerConfig {
                symbols: Vec::new(),
                timeframe: DEFAULT_TIMEFRAME.to_owned(),
                bars: DEFAULT_BARS,
                cycle_interval: DEFAULT_CYCLE_INTERVAL,
                heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
                cycle_timeout: DEFAULT_CYCLE_TIMEOUT,
                provider_definition_id: None,
                source_of_truth: true,
            },
        );

        let WorkerMessage::Cycle {
            ok,
            evidence,
            error,
            ..
        } = worker.cycle(at(), CycleResult::provider_failure("timeout".to_owned()))
        else {
            panic!("expected cycle message");
        };

        assert!(!ok);
        assert_eq!(error.as_deref(), Some("timeout"));
        assert_eq!(evidence.last_success_at, None);
        assert_eq!(evidence.dependencies_reachable, Some(false));
        assert!(worker.ready_after_success(at()).is_none());
    }
}
