//! Research worker core: deterministic scans over stored closed candles.
//!
//! The worker consumes only persisted closed bars, runs the deterministic VSA method,
//! and stores observations with source, input, producer, schema, and quality provenance.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use thiserror::Error;
use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};
use tm_trading_domain::{
    Candle, NewResearchEvent, ResearchEvent, ResearchEventError, ResearchQuality, input_snapshot_id,
};
use tm_trading_market_providers::timeframe_millis;
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::methods::{
    PRICE_ACTION_ID, PriceActionParams, SWEEP_ID, SweepParams, TREND_ID, TrendParams, VSA_ID,
    VsaEventKind, VsaParams, WONDERWHALES_ID, WonderWhalesParams, analyze_price_action,
    analyze_sweep, analyze_trend, analyze_vsa, analyze_wonderwhales,
};
use tm_trading_storage::{CandleRepository, RepositoryError, ResearchEventRepository};

/// Version of the VSA observation contract produced by this worker.
pub const METHOD_VERSION: &str = "1";
/// Canonical timeframe used when none is configured.
pub const DEFAULT_TIMEFRAME: &str = "1m";
/// Closed candles read per symbol and cycle.
pub const DEFAULT_BARS: usize = 120;
/// Upper bound on closed candles read per symbol and cycle.
pub const MAX_BARS: usize = 1_000;
/// Upper bound on configured symbols, so one cycle stays bounded.
pub const MAX_SYMBOLS: usize = 50;
/// Default delay between two scan cycles.
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
/// Default deadline for one complete research scan.
pub const DEFAULT_CYCLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Shortest accepted scan deadline.
pub const MIN_CYCLE_TIMEOUT: Duration = Duration::from_secs(1);
/// Longest accepted scan deadline.
pub const MAX_CYCLE_TIMEOUT: Duration = Duration::from_secs(300);

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
    /// Active research methods to execute during scan (defaults to ["vsa"]).
    pub methods: Vec<String>,
}

impl ResearchWorkerConfig {
    /// Reads the configuration from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a required value is missing or a supplied value is invalid.
    /// Write authority remains off unless it is explicitly enabled.
    pub fn from_env() -> Result<Self, ResearchWorkerConfigError> {
        let allow_insecure_dev = std::env::var("TM_STORAGE_ALLOW_INSECURE_DEV")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        let owner_scope = std::env::var("TM_TRADING_RESEARCH_OWNER_SCOPE").unwrap_or_default();
        let mut owner_scope = owner_scope.trim().to_owned();
        if owner_scope.is_empty() && allow_insecure_dev {
            owner_scope = "default".to_owned();
        }
        if owner_scope.is_empty() {
            return Err(ResearchWorkerConfigError::MissingOwnerScope);
        }
        let mut symbols =
            parse_symbols(&std::env::var("TM_TRADING_RESEARCH_SYMBOLS").unwrap_or_default())?;
        if symbols.is_empty() && allow_insecure_dev {
            symbols = vec!["BTCUSDT".to_owned(), "ETHUSDT".to_owned()];
        }
        if symbols.is_empty() {
            return Err(ResearchWorkerConfigError::NoSymbols);
        }
        let timeframe = std::env::var("TM_TRADING_RESEARCH_TIMEFRAME")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_TIMEFRAME.to_owned());
        timeframe_millis(&timeframe).map_err(|_| ResearchWorkerConfigError::InvalidTimeframe)?;
        let bars = parse_bars(&std::env::var("TM_TRADING_RESEARCH_BARS").unwrap_or_default())?;
        let cycle_interval = parse_duration(
            "TM_TRADING_RESEARCH_CYCLE_MS",
            DEFAULT_CYCLE_INTERVAL,
            MIN_CYCLE_INTERVAL,
            MAX_CYCLE_INTERVAL,
        )?;
        let heartbeat_interval = parse_duration(
            "TM_TRADING_RESEARCH_HEARTBEAT_MS",
            DEFAULT_HEARTBEAT_INTERVAL,
            MIN_HEARTBEAT_INTERVAL,
            MAX_HEARTBEAT_INTERVAL,
        )?;
        let cycle_timeout = parse_duration(
            "TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS",
            DEFAULT_CYCLE_TIMEOUT,
            MIN_CYCLE_TIMEOUT,
            MAX_CYCLE_TIMEOUT,
        )?;
        let source_of_truth =
            parse_bool("TM_TRADING_RESEARCH_SOURCE_OF_TRUTH", allow_insecure_dev)?;
        let methods =
            parse_methods(&std::env::var("TM_TRADING_RESEARCH_METHODS").unwrap_or_default());

        Ok(Self {
            owner_scope,
            symbols,
            timeframe,
            bars,
            cycle_interval,
            heartbeat_interval,
            cycle_timeout,
            source_of_truth,
            methods,
        })
    }
}

fn parse_methods(value: &str) -> Vec<String> {
    let mut methods = Vec::new();
    for method in value.split(',') {
        let method = method.trim().to_ascii_lowercase();
        if method.is_empty() {
            continue;
        }
        if !methods.contains(&method) {
            methods.push(method);
        }
    }
    if methods.is_empty() {
        vec![VSA_ID.to_owned()]
    } else {
        methods
    }
}

fn parse_symbols(value: &str) -> Result<Vec<String>, ResearchWorkerConfigError> {
    let mut symbols = Vec::new();
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
        return Err(ResearchWorkerConfigError::TooManySymbols);
    }
    Ok(symbols)
}

fn parse_bars(value: &str) -> Result<usize, ResearchWorkerConfigError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(DEFAULT_BARS);
    }
    let bars: usize = value
        .parse()
        .map_err(|_| ResearchWorkerConfigError::InvalidBars)?;
    if bars == 0 || bars > MAX_BARS {
        return Err(ResearchWorkerConfigError::InvalidBars);
    }
    Ok(bars)
}

fn parse_duration(
    name: &'static str,
    default: Duration,
    minimum: Duration,
    maximum: Duration,
) -> Result<Duration, ResearchWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(default);
    }
    let millis: u64 = value
        .parse()
        .map_err(|_| ResearchWorkerConfigError::InvalidDuration(name))?;
    Ok(Duration::from_millis(millis).clamp(minimum, maximum))
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ResearchWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(default);
    }
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ResearchWorkerConfigError::InvalidBoolean(name)),
    }
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

/// Invalid Research Worker configuration.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResearchWorkerConfigError {
    #[error("TM_TRADING_RESEARCH_OWNER_SCOPE must not be empty")]
    MissingOwnerScope,
    #[error("TM_TRADING_RESEARCH_SYMBOLS must list at least one symbol")]
    NoSymbols,
    #[error("TM_TRADING_RESEARCH_SYMBOLS lists more than {MAX_SYMBOLS} symbols")]
    TooManySymbols,
    #[error("TM_TRADING_RESEARCH_TIMEFRAME is not a supported timeframe")]
    InvalidTimeframe,
    #[error("TM_TRADING_RESEARCH_BARS must be between 1 and {MAX_BARS}")]
    InvalidBars,
    #[error("{0} is not a valid number of milliseconds")]
    InvalidDuration(&'static str),
    #[error("{0} must be a boolean value")]
    InvalidBoolean(&'static str),
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
    /// A cycle that completed a bounded scan and published its observations.
    #[must_use]
    pub fn success(outcome: ResearchScanOutcome, duration_ms: u64) -> Self {
        Self {
            ok: true,
            skipped: None,
            duration_ms,
            output_count: Some(outcome.detected_events as u64),
            error: None,
            summary: Some(format!(
                "scanned {} candle(s) across {} symbol(s); detected {} research event(s), inserted {} and found {} existing",
                outcome.scanned_candles,
                outcome.scanned_symbols,
                outcome.detected_events,
                outcome.inserted_events,
                outcome.existing_events,
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
            dependencies_reachable: Some(true),
            sink_writable: None,
        }
    }

    /// A cycle whose required input was absent, stale, or invalid.
    #[must_use]
    pub fn input_failure(message: String) -> Self {
        Self {
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: None,
            error: Some(message),
            summary: None,
            dependencies_reachable: Some(true),
            sink_writable: None,
        }
    }

    /// A cycle that could not read or write the store.
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

    /// A cycle rejected before any work was attempted.
    #[must_use]
    pub fn failure(message: String) -> Self {
        Self {
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: None,
            error: Some(message),
            summary: None,
            dependencies_reachable: None,
            sink_writable: None,
        }
    }
}

/// The worker state that produces protocol messages.
#[derive(Debug)]
pub struct ResearchWorker {
    instance_id: String,
    executable_path: String,
    config: ResearchWorkerConfig,
    evidence: WorkerEvidence,
}

impl ResearchWorker {
    #[must_use]
    pub fn new(instance_id: &str, config: ResearchWorkerConfig) -> Self {
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
    pub fn config(&self) -> &ResearchWorkerConfig {
        &self.config
    }

    #[must_use]
    pub fn evidence(&self) -> &WorkerEvidence {
        &self.evidence
    }

    /// Records whether the storage dependency was reachable at startup.
    pub fn record_dependencies_reachable(&mut self, reachable: bool) {
        self.evidence.dependencies_reachable = Some(reachable);
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

    /// Reports readiness only after a successful, non-skipped research scan.
    #[must_use]
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
        } else {
            self.evidence.capability_verified = Some(false);
        }
    }
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
        for method_name in &config.methods {
            match method_name.as_str() {
                VSA_ID => {
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
                }
                PRICE_ACTION_ID => {
                    let analysis = analyze_price_action(&bars, &PriceActionParams::default());
                    let snapshot_id = input_snapshot_id(
                        symbol,
                        &config.timeframe,
                        PRICE_ACTION_ID,
                        METHOD_VERSION,
                        candles.first().expect("window is not empty").open_time(),
                        latest.open_time(),
                        candles.len(),
                    );

                    for event in &analysis.events {
                        let candle = &candles[event.bar_index];
                        let observed_at = timestamp_for(symbol, candle.close_time())?;
                        let source_id = candle.source();
                        let evidence = BTreeMap::from([
                            ("open".to_owned(), candle.open()),
                            ("high".to_owned(), candle.high()),
                            ("low".to_owned(), candle.low()),
                            ("close".to_owned(), candle.close()),
                            ("volume".to_owned(), candle.volume()),
                            ("score".to_owned(), event.score),
                        ]);

                        events.push(ResearchEvent::new(NewResearchEvent {
                            owner_scope: config.owner_scope.clone(),
                            symbol: symbol.clone(),
                            timeframe: config.timeframe.clone(),
                            method: PRICE_ACTION_ID.to_owned(),
                            method_version: METHOD_VERSION.to_owned(),
                            event_type: event.kind.as_str().to_owned(),
                            direction: score_direction(event.score),
                            score: event.score,
                            observed_at,
                            detected_at: observed_at,
                            ingested_at: now,
                            source_id: source_id.to_owned(),
                            provider_method: provider_method(source_id),
                            input_snapshot_id: snapshot_id.clone(),
                            quality_state: ResearchQuality::Complete,
                            quality_reasons: Vec::new(),
                            evidence,
                            context: BTreeMap::from([(
                                "event".to_owned(),
                                event.kind.as_str().to_owned(),
                            )]),
                        })?);
                    }
                }
                TREND_ID => {
                    let analysis = analyze_trend(&bars, &TrendParams::default());
                    let snapshot_id = input_snapshot_id(
                        symbol,
                        &config.timeframe,
                        TREND_ID,
                        METHOD_VERSION,
                        candles.first().expect("window is not empty").open_time(),
                        latest.open_time(),
                        candles.len(),
                    );

                    for event in &analysis.events {
                        let candle = &candles[event.bar_index];
                        let observed_at = timestamp_for(symbol, candle.close_time())?;
                        let source_id = candle.source();
                        let evidence = BTreeMap::from([
                            ("open".to_owned(), candle.open()),
                            ("high".to_owned(), candle.high()),
                            ("low".to_owned(), candle.low()),
                            ("close".to_owned(), candle.close()),
                            ("volume".to_owned(), candle.volume()),
                            ("score".to_owned(), event.score),
                        ]);

                        events.push(ResearchEvent::new(NewResearchEvent {
                            owner_scope: config.owner_scope.clone(),
                            symbol: symbol.clone(),
                            timeframe: config.timeframe.clone(),
                            method: TREND_ID.to_owned(),
                            method_version: METHOD_VERSION.to_owned(),
                            event_type: event.kind.as_str().to_owned(),
                            direction: score_direction(event.score),
                            score: event.score,
                            observed_at,
                            detected_at: observed_at,
                            ingested_at: now,
                            source_id: source_id.to_owned(),
                            provider_method: provider_method(source_id),
                            input_snapshot_id: snapshot_id.clone(),
                            quality_state: ResearchQuality::Complete,
                            quality_reasons: Vec::new(),
                            evidence,
                            context: BTreeMap::from([(
                                "event".to_owned(),
                                event.kind.as_str().to_owned(),
                            )]),
                        })?);
                    }
                }
                WONDERWHALES_ID => {
                    let analysis = analyze_wonderwhales(&bars, &WonderWhalesParams::default());
                    let snapshot_id = input_snapshot_id(
                        symbol,
                        &config.timeframe,
                        WONDERWHALES_ID,
                        METHOD_VERSION,
                        candles.first().expect("window is not empty").open_time(),
                        latest.open_time(),
                        candles.len(),
                    );

                    for event in &analysis.events {
                        let candle = &candles[event.bar_index];
                        let observed_at = timestamp_for(symbol, candle.close_time())?;
                        let source_id = candle.source();
                        let evidence = BTreeMap::from([
                            ("open".to_owned(), candle.open()),
                            ("high".to_owned(), candle.high()),
                            ("low".to_owned(), candle.low()),
                            ("close".to_owned(), candle.close()),
                            ("volume".to_owned(), candle.volume()),
                            ("score".to_owned(), event.score),
                        ]);

                        events.push(ResearchEvent::new(NewResearchEvent {
                            owner_scope: config.owner_scope.clone(),
                            symbol: symbol.clone(),
                            timeframe: config.timeframe.clone(),
                            method: WONDERWHALES_ID.to_owned(),
                            method_version: METHOD_VERSION.to_owned(),
                            event_type: event.kind.as_str().to_owned(),
                            direction: score_direction(event.score),
                            score: event.score,
                            observed_at,
                            detected_at: observed_at,
                            ingested_at: now,
                            source_id: source_id.to_owned(),
                            provider_method: provider_method(source_id),
                            input_snapshot_id: snapshot_id.clone(),
                            quality_state: ResearchQuality::Complete,
                            quality_reasons: Vec::new(),
                            evidence,
                            context: BTreeMap::from([(
                                "event".to_owned(),
                                event.kind.as_str().to_owned(),
                            )]),
                        })?);
                    }
                }
                SWEEP_ID => {
                    let analysis = analyze_sweep(&bars, &SweepParams::default());
                    let snapshot_id = input_snapshot_id(
                        symbol,
                        &config.timeframe,
                        SWEEP_ID,
                        METHOD_VERSION,
                        candles.first().expect("window is not empty").open_time(),
                        latest.open_time(),
                        candles.len(),
                    );

                    for event in &analysis.events {
                        let candle = &candles[event.bar_index];
                        let observed_at = timestamp_for(symbol, candle.close_time())?;
                        let source_id = candle.source();
                        let mut evidence = BTreeMap::from([
                            ("open".to_owned(), candle.open()),
                            ("high".to_owned(), candle.high()),
                            ("low".to_owned(), candle.low()),
                            ("close".to_owned(), candle.close()),
                            ("volume".to_owned(), candle.volume()),
                            ("score".to_owned(), event.score),
                        ]);
                        evidence.insert("swept_level".to_owned(), event.swept_level);

                        events.push(ResearchEvent::new(NewResearchEvent {
                            owner_scope: config.owner_scope.clone(),
                            symbol: symbol.clone(),
                            timeframe: config.timeframe.clone(),
                            method: SWEEP_ID.to_owned(),
                            method_version: METHOD_VERSION.to_owned(),
                            event_type: event.kind.as_str().to_owned(),
                            direction: score_direction(event.score),
                            score: event.score,
                            observed_at,
                            detected_at: observed_at,
                            ingested_at: now,
                            source_id: source_id.to_owned(),
                            provider_method: provider_method(source_id),
                            input_snapshot_id: snapshot_id.clone(),
                            quality_state: ResearchQuality::Complete,
                            quality_reasons: Vec::new(),
                            evidence,
                            context: BTreeMap::from([(
                                "event".to_owned(),
                                event.kind.as_str().to_owned(),
                            )]),
                        })?);
                    }
                }
                _ => {}
            }
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

/// Runs one bounded research cycle and converts every failure into an explicit outcome.
pub async fn run_research_cycle(
    candle_repository: &dyn CandleRepository,
    event_repository: &dyn ResearchEventRepository,
    config: &ResearchWorkerConfig,
    now: DateTime<Utc>,
) -> CycleResult {
    let started = Instant::now();
    match tokio::time::timeout(
        config.cycle_timeout,
        scan_research_events(candle_repository, event_repository, config, now),
    )
    .await
    {
        Ok(Ok(outcome)) => {
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            CycleResult::success(outcome, duration_ms)
        }
        Ok(Err(error)) => {
            let message = error.to_string();
            match error {
                ResearchWorkerError::Storage(error) => {
                    CycleResult::storage_failure(error.to_string())
                }
                ResearchWorkerError::Event(error) => CycleResult::input_failure(error.to_string()),
                ResearchWorkerError::NoClosedCandles { .. } => {
                    CycleResult::skipped("no closed candles are available for the research window")
                }
                ResearchWorkerError::WriteAuthorityDisabled => CycleResult::skipped(
                    "the research worker is not the source of truth for research event writes",
                ),
                ResearchWorkerError::NoSymbols => {
                    CycleResult::skipped("no research symbols are configured")
                }
                ResearchWorkerError::StaleInput { .. }
                | ResearchWorkerError::InputNotClosed { .. }
                | ResearchWorkerError::InvalidTimestamp { .. }
                | ResearchWorkerError::InvalidTimeframe(_) => CycleResult::input_failure(message),
            }
        }
        Err(_) => CycleResult::timeout(format!(
            "research scan cycle exceeded its {} ms deadline",
            u64::try_from(config.cycle_timeout.as_millis()).unwrap_or(u64::MAX)
        )),
    }
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

fn score_direction(score: f64) -> i8 {
    if score > 0.0 {
        1
    } else if score < 0.0 {
        -1
    } else {
        0
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
            methods: vec![VSA_ID.to_owned()],
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

    struct PendingCandleRepository;

    #[async_trait::async_trait]
    impl CandleRepository for PendingCandleRepository {
        async fn upsert_closed(
            &self,
            _candles: &[Candle],
        ) -> Result<tm_trading_storage::CandleUpsertOutcome, RepositoryError> {
            Ok(tm_trading_storage::CandleUpsertOutcome::default())
        }

        async fn recent(
            &self,
            _symbol: &str,
            _timeframe: &str,
            _limit: usize,
        ) -> Result<Vec<Candle>, RepositoryError> {
            std::future::pending().await
        }

        async fn get(&self, _candle_id: &str) -> Result<Option<Candle>, RepositoryError> {
            Ok(None)
        }
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
    async fn multi_method_scan_produces_events_across_methods() {
        let candles = fixture_candles();
        let now = latest_time(&candles) + chrono::Duration::seconds(1);
        let candle_repository = InMemoryCandleRepository::new();
        candle_repository
            .upsert_closed(&candles)
            .await
            .expect("seed candles");
        let event_repository = InMemoryResearchEventRepository::new();

        let mut multi_config = config();
        multi_config.methods = vec![
            VSA_ID.to_owned(),
            PRICE_ACTION_ID.to_owned(),
            TREND_ID.to_owned(),
            WONDERWHALES_ID.to_owned(),
            SWEEP_ID.to_owned(),
        ];

        let outcome =
            scan_research_events(&candle_repository, &event_repository, &multi_config, now)
                .await
                .expect("scan succeeds");

        assert_eq!(outcome.scanned_symbols, 1);
        assert!(
            outcome.detected_events >= 5,
            "multi-method should detect vsa + any additional pattern events"
        );

        let stored = event_repository
            .list(&ResearchEventQuery {
                owner_scope: Some("test-operator".to_owned()),
                ..ResearchEventQuery::default()
            })
            .await
            .expect("list events");

        let vsa_events: Vec<_> = stored.iter().filter(|e| e.method() == "vsa").collect();
        assert_eq!(vsa_events.len(), 5);
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

    #[tokio::test]
    async fn no_symbols_is_not_a_successful_scan() {
        let candle_repository = InMemoryCandleRepository::new();
        let event_repository = InMemoryResearchEventRepository::new();
        let mut settings = config();
        settings.symbols.clear();

        let result =
            scan_research_events(&candle_repository, &event_repository, &settings, Utc::now())
                .await;

        assert!(matches!(result, Err(ResearchWorkerError::NoSymbols)));
    }

    #[tokio::test]
    async fn a_candle_that_is_not_closed_is_rejected() {
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
            latest_time(&candles) - chrono::Duration::seconds(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(ResearchWorkerError::InputNotClosed { symbol }) if symbol == "BTCUSDT"
        ));
    }

    #[tokio::test]
    async fn a_missing_window_is_reported_as_skipped() {
        let candle_repository = InMemoryCandleRepository::new();
        let event_repository = InMemoryResearchEventRepository::new();

        let result =
            run_research_cycle(&candle_repository, &event_repository, &config(), Utc::now()).await;

        assert!(!result.ok);
        assert!(result.skipped.is_some());
        assert!(result.output_count.is_none());
    }

    #[test]
    fn readiness_requires_a_successful_scan() {
        let mut worker = ResearchWorker::new("research-worker@test:1", config());
        worker.record_dependencies_reachable(true);
        assert!(worker.ready_after_success(Utc::now()).is_none());

        let result = CycleResult::success(
            ResearchScanOutcome {
                scanned_symbols: 1,
                scanned_candles: 120,
                detected_events: 5,
                inserted_events: 5,
                existing_events: 0,
            },
            10,
        );
        worker.cycle(Utc::now(), result);

        assert!(worker.ready_after_success(Utc::now()).is_some());
    }

    #[tokio::test]
    async fn an_overdue_scan_is_cancelled_and_reported_as_not_ready() {
        let event_repository = InMemoryResearchEventRepository::new();
        let mut settings = config();
        settings.cycle_timeout = Duration::from_millis(10);

        let result = run_research_cycle(
            &PendingCandleRepository,
            &event_repository,
            &settings,
            Utc::now(),
        )
        .await;

        assert!(!result.ok);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|error| error.contains("deadline"))
        );
        assert_eq!(result.dependencies_reachable, Some(false));
        assert_eq!(result.sink_writable, Some(false));
    }
}
