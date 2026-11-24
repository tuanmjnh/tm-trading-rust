//! News worker core: bounded RSS/Atom collection into the news item store.
//!
//! One cycle reads the source registry, polls the sources that are due and
//! pass the fail-closed license gate, stores the rights-safe items, and records
//! per-source health. A source that fails is reported instead of being treated
//! as healthy, and a cycle without a single successful poll is not a success.

use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};
use tm_trading_domain::{
    NewNewsItem, NewsDirection, NewsItem, NewsItemError, NewsItemRights, NewsLevel,
    NewsLicenseGate, NewsSource, NewsTransport, significant_tokens,
};
use tm_trading_news_providers::{NewsFeedFetcher, RawFeedItem};
use tm_trading_storage::{
    NewsClusterRepository, NewsItemRepository, NewsSourceRepository, RepositoryError,
};

/// Default number of items kept from one source and cycle.
pub const DEFAULT_MAX_ITEMS_PER_SOURCE: usize = 40;
/// Upper bound on items kept from one source and cycle.
pub const MAX_ITEMS_PER_SOURCE: usize = 200;
/// Default number of sources polled per cycle.
pub const DEFAULT_MAX_SOURCES_PER_CYCLE: usize = 25;
/// Upper bound on sources polled per cycle.
pub const MAX_SOURCES_PER_CYCLE: usize = 100;
/// Default delay between two collection cycles.
pub const DEFAULT_CYCLE_INTERVAL: Duration = Duration::from_secs(60);
/// Shortest accepted cycle delay.
pub const MIN_CYCLE_INTERVAL: Duration = Duration::from_secs(10);
/// Longest accepted cycle delay.
pub const MAX_CYCLE_INTERVAL: Duration = Duration::from_secs(3_600);
/// Default delay between two heartbeats.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// Shortest accepted heartbeat delay.
pub const MIN_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
/// Longest accepted heartbeat delay.
pub const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(300);
/// Default deadline for one complete collection cycle.
pub const DEFAULT_CYCLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Shortest accepted cycle deadline.
pub const MIN_CYCLE_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest accepted cycle deadline.
pub const MAX_CYCLE_TIMEOUT: Duration = Duration::from_secs(600);

/// Configuration for one news collection cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsWorkerConfig {
    /// Maximum number of items kept from one source and cycle.
    pub max_items_per_source: usize,
    /// Maximum number of sources polled in one cycle.
    pub max_sources_per_cycle: usize,
    pub cycle_interval: Duration,
    pub heartbeat_interval: Duration,
    pub cycle_timeout: Duration,
    /// Whether this installation is the sole writer for news items.
    ///
    /// The default is `false` so a Rust worker cannot dual-write with the legacy
    /// collector during cutover.
    pub source_of_truth: bool,
}

impl NewsWorkerConfig {
    /// Reads the configuration from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a supplied value is invalid. Write authority
    /// remains off unless it is explicitly enabled.
    pub fn from_env() -> Result<Self, NewsWorkerConfigError> {
        let max_items_per_source = parse_count(
            "TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE",
            DEFAULT_MAX_ITEMS_PER_SOURCE,
            MAX_ITEMS_PER_SOURCE,
        )?;
        let max_sources_per_cycle = parse_count(
            "TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE",
            DEFAULT_MAX_SOURCES_PER_CYCLE,
            MAX_SOURCES_PER_CYCLE,
        )?;
        let cycle_interval = parse_duration(
            "TM_TRADING_NEWS_CYCLE_MS",
            DEFAULT_CYCLE_INTERVAL,
            MIN_CYCLE_INTERVAL,
            MAX_CYCLE_INTERVAL,
        )?;
        let heartbeat_interval = parse_duration(
            "TM_TRADING_NEWS_HEARTBEAT_MS",
            DEFAULT_HEARTBEAT_INTERVAL,
            MIN_HEARTBEAT_INTERVAL,
            MAX_HEARTBEAT_INTERVAL,
        )?;
        let cycle_timeout = parse_duration(
            "TM_TRADING_NEWS_CYCLE_TIMEOUT_MS",
            DEFAULT_CYCLE_TIMEOUT,
            MIN_CYCLE_TIMEOUT,
            MAX_CYCLE_TIMEOUT,
        )?;
        let allow_insecure_dev = std::env::var("TM_STORAGE_ALLOW_INSECURE_DEV")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        let source_of_truth = parse_bool("TM_TRADING_NEWS_SOURCE_OF_TRUTH", allow_insecure_dev)?;
        Ok(Self {
            max_items_per_source,
            max_sources_per_cycle,
            cycle_interval,
            heartbeat_interval,
            cycle_timeout,
            source_of_truth,
        })
    }
}

fn parse_count(
    name: &'static str,
    default: usize,
    maximum: usize,
) -> Result<usize, NewsWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(default);
    }
    let count: usize = value
        .parse()
        .map_err(|_| NewsWorkerConfigError::InvalidCount(name))?;
    if count == 0 || count > maximum {
        return Err(NewsWorkerConfigError::InvalidCount(name));
    }
    Ok(count)
}

fn parse_duration(
    name: &'static str,
    default: Duration,
    minimum: Duration,
    maximum: Duration,
) -> Result<Duration, NewsWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(default);
    }
    let millis: u64 = value
        .parse()
        .map_err(|_| NewsWorkerConfigError::InvalidDuration(name))?;
    Ok(Duration::from_millis(millis).clamp(minimum, maximum))
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, NewsWorkerConfigError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(default);
    }
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(NewsWorkerConfigError::InvalidBoolean(name)),
    }
}

/// Result of one complete collection cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NewsCycleOutcome {
    pub sources_considered: usize,
    pub sources_fetched: usize,
    pub sources_failed: usize,
    pub sources_skipped: usize,
    pub items_seen: usize,
    pub items_dropped: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// Errors that prevent a collection cycle from completing successfully.
#[derive(Debug, Error)]
pub enum NewsWorkerError {
    #[error(transparent)]
    Storage(#[from] RepositoryError),
    #[error(transparent)]
    Item(#[from] NewsItemError),
    #[error(transparent)]
    Cluster(#[from] tm_trading_domain::NewsClusterError),
    #[error("the news worker is not the source of truth for news writes")]
    WriteAuthorityDisabled,
    #[error("the news source registry is empty")]
    NoSources,
    #[error("no enabled RSS source is due for collection")]
    NoEligibleSources,
    #[error("all {sources} polled news source(s) failed")]
    AllSourcesFailed { sources: usize },
}

/// Invalid News Worker configuration.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NewsWorkerConfigError {
    #[error("{0} is not a valid positive count")]
    InvalidCount(&'static str),
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
    /// A cycle that polled at least one source and stored its items.
    #[must_use]
    pub fn success(outcome: NewsCycleOutcome, duration_ms: u64) -> Self {
        Self {
            ok: true,
            skipped: None,
            duration_ms,
            output_count: Some((outcome.inserted + outcome.updated) as u64),
            error: None,
            summary: Some(format!(
                "considered {} source(s): polled {}, failed {}, skipped {}; stored {} new and {} updated item(s), {} unchanged, {} dropped",
                outcome.sources_considered,
                outcome.sources_fetched,
                outcome.sources_failed,
                outcome.sources_skipped,
                outcome.inserted,
                outcome.updated,
                outcome.unchanged,
                outcome.items_dropped,
            )),
            dependencies_reachable: Some(true),
            sink_writable: Some(true),
        }
    }

    /// A cycle that polled nothing because no source is due; it is not a success.
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
            sink_writable: None,
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
pub struct NewsWorker {
    instance_id: String,
    executable_path: String,
    config: NewsWorkerConfig,
    evidence: WorkerEvidence,
}

impl NewsWorker {
    #[must_use]
    pub fn new(instance_id: &str, config: NewsWorkerConfig) -> Self {
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
    pub fn config(&self) -> &NewsWorkerConfig {
        &self.config
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

    /// Reports readiness only after a successful, non-skipped collection cycle.
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

/// Polls every due, allowed RSS source once and stores the items it returns.
///
/// A source that fails records its sanitized health code and does not fail the
/// other sources. The cycle only succeeds when at least one source was polled.
pub async fn run_news_cycle(
    source_repository: &dyn NewsSourceRepository,
    item_repository: &dyn NewsItemRepository,
    cluster_repository: &dyn NewsClusterRepository,
    fetcher: &dyn NewsFeedFetcher,
    config: &NewsWorkerConfig,
    now: DateTime<Utc>,
) -> Result<NewsCycleOutcome, NewsWorkerError> {
    if !config.source_of_truth {
        return Err(NewsWorkerError::WriteAuthorityDisabled);
    }
    let sources = source_repository.list().await?;
    if sources.is_empty() {
        return Err(NewsWorkerError::NoSources);
    }

    let mut outcome = NewsCycleOutcome::default();
    let mut attempted = 0_usize;
    let mut succeeded = 0_usize;

    for source in sources.iter().take(config.max_sources_per_cycle) {
        outcome.sources_considered += 1;
        if !is_due(source, now)
            || source.transport() != NewsTransport::Rss
            || source.license_gate() != NewsLicenseGate::Allowed
        {
            outcome.sources_skipped += 1;
            continue;
        }
        let Some(feed_url) = source.feed_url() else {
            outcome.sources_skipped += 1;
            continue;
        };
        attempted += 1;

        match fetcher.fetch(feed_url).await {
            Ok(raw_items) => {
                let mut items = Vec::new();
                for raw_item in raw_items.iter().take(config.max_items_per_source) {
                    outcome.items_seen += 1;
                    match build_item(raw_item, source, now) {
                        Ok(item) => items.push(item),
                        Err(_) => outcome.items_dropped += 1,
                    }
                }
                let upsert = item_repository.upsert_seen(&items).await?;
                outcome.inserted += upsert.inserted;
                outcome.updated += upsert.updated;
                outcome.unchanged += upsert.unchanged;

                // Group the freshly stored stories into event clusters so the
                // read side can present one event instead of many near-duplicate
                // headlines. Only recently observed clusters are loaded so the
                // assignment stays bounded.
                let mut recent_clusters = cluster_repository
                    .list(&tm_trading_storage::NewsClusterQuery {
                        limit: 200,
                        ..tm_trading_storage::NewsClusterQuery::default()
                    })
                    .await?;
                for item in &items {
                    let tokens = significant_tokens(item.title());
                    if tokens.is_empty() {
                        continue;
                    }
                    let market_group = Some(item.market_group().to_owned());
                    if let Some(position) = recent_clusters
                        .iter()
                        .position(|cluster| cluster.matches_tokens(&tokens))
                    {
                        let mut cluster = recent_clusters[position].clone();
                        cluster.add_item(item.item_id(), now);
                        cluster_repository.upsert(&cluster).await?;
                        recent_clusters[position] = cluster;
                    } else {
                        let cluster = tm_trading_domain::NewsCluster::new(
                            tm_trading_domain::NewNewsCluster {
                                tokens,
                                item_id: item.item_id().to_owned(),
                                market_group,
                                observed_at: now,
                            },
                        )
                        .map_err(NewsWorkerError::Cluster)?;
                        cluster_repository.upsert(&cluster).await?;
                        recent_clusters.push(cluster);
                    }
                }
                source_repository
                    .record_health(source.source_id(), now, items.len() as u32, None)
                    .await?;
                outcome.sources_fetched += 1;
                succeeded += 1;
            }
            Err(error) => {
                source_repository
                    .record_health(source.source_id(), now, 0, Some(error.code()))
                    .await?;
                outcome.sources_failed += 1;
            }
        }
    }

    if attempted == 0 {
        return Err(NewsWorkerError::NoEligibleSources);
    }
    if succeeded == 0 {
        return Err(NewsWorkerError::AllSourcesFailed { sources: attempted });
    }
    Ok(outcome)
}

fn is_due(source: &NewsSource, now: DateTime<Utc>) -> bool {
    if source.retry_requested_at().is_some() {
        return true;
    }
    match source.health().last_fetch_at {
        None => true,
        Some(last_fetch_at) => {
            now >= last_fetch_at + chrono::Duration::seconds(i64::from(source.poll_interval_sec()))
        }
    }
}

fn build_item(
    raw: &RawFeedItem,
    source: &NewsSource,
    now: DateTime<Utc>,
) -> Result<NewsItem, NewsItemError> {
    NewsItem::new(NewNewsItem {
        source_id: source.source_id().to_owned(),
        url: raw.link.clone(),
        origin_url: raw.link.clone(),
        title: raw.title.clone(),
        excerpt: raw.excerpt.clone(),
        language: source.languages().first().cloned().unwrap_or_default(),
        published_at: raw.published_at,
        first_seen_at: now,
        ingested_at: now,
        market_group: None,
        source_market_groups: source.market_groups().to_vec(),
        event_type: None,
        tags: raw.tags.clone(),
        level: NewsLevel::Med,
        score: 0.0,
        direction: NewsDirection::Flat,
        rights: NewsItemRights {
            allowed_display: source.rights().allowed_display,
            allowed_summarize: source.rights().allowed_summarize,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use tm_trading_domain::{NewNewsSource, NewsLicenseStatus, NewsRights, NewsTrustTier};
    use tm_trading_news_providers::NewsFetchError;
    use tm_trading_storage::{
        InMemoryNewsClusterRepository, InMemoryNewsItemRepository, InMemoryNewsSourceRepository,
        NewsItemQuery,
    };

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    fn config() -> NewsWorkerConfig {
        NewsWorkerConfig {
            max_items_per_source: 10,
            max_sources_per_cycle: 10,
            cycle_interval: DEFAULT_CYCLE_INTERVAL,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            cycle_timeout: DEFAULT_CYCLE_TIMEOUT,
            source_of_truth: true,
        }
    }

    fn source_input(source_id: &str, license_status: NewsLicenseStatus) -> NewNewsSource {
        NewNewsSource {
            source_id: source_id.to_owned(),
            name: source_id.to_owned(),
            provider: None,
            transport: NewsTransport::Rss,
            feed_url: Some(format!("https://example.com/{source_id}.rss")),
            trust_tier: NewsTrustTier::T2Secondary,
            market_groups: vec!["crypto".to_owned()],
            languages: vec!["en".to_owned()],
            license_status,
            rights: NewsRights {
                allowed_store_metadata: true,
                allowed_display: true,
                allowed_summarize: false,
                allowed_training: false,
            },
            enabled: true,
            poll_interval_sec: 600,
            retention_days: 7,
            created_at: at(),
        }
    }

    struct StubFetcher {
        result: Result<Vec<RawFeedItem>, NewsFetchError>,
    }

    #[async_trait]
    impl NewsFeedFetcher for StubFetcher {
        async fn fetch(&self, _url: &str) -> Result<Vec<RawFeedItem>, NewsFetchError> {
            match &self.result {
                Ok(items) => Ok(items.clone()),
                Err(error) => Err(NewsFetchError::Status {
                    status: match error {
                        NewsFetchError::Status { status } => *status,
                        _ => 500,
                    },
                }),
            }
        }
    }

    fn raw_item(link: &str) -> RawFeedItem {
        RawFeedItem {
            title: "Bitcoin holds support".to_owned(),
            link: link.to_owned(),
            published_at: None,
            excerpt: "A short summary.".to_owned(),
            tags: vec!["bitcoin".to_owned()],
        }
    }

    #[tokio::test]
    async fn polls_a_due_allowed_source_and_stores_its_items() {
        let sources = InMemoryNewsSourceRepository::new();
        sources
            .upsert_seed(source_input("coindesk", NewsLicenseStatus::Ok))
            .await
            .unwrap();
        let items = InMemoryNewsItemRepository::new();
        let clusters = InMemoryNewsClusterRepository::new();
        let fetcher = StubFetcher {
            result: Ok(vec![raw_item("https://example.com/story")]),
        };

        let outcome = run_news_cycle(&sources, &items, &clusters, &fetcher, &config(), at())
            .await
            .expect("cycle");

        assert_eq!(outcome.sources_fetched, 1);
        assert_eq!(outcome.items_seen, 1);
        assert_eq!(outcome.inserted, 1);
        let stored = items.list(&NewsItemQuery::default()).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source_id(), "coindesk");
        let source = sources.get("coindesk").await.unwrap().unwrap();
        assert_eq!(source.health().last_item_at, Some(at()));
    }

    #[tokio::test]
    async fn skips_a_source_that_is_not_due_yet() {
        let sources = InMemoryNewsSourceRepository::new();
        sources
            .upsert_seed(source_input("coindesk", NewsLicenseStatus::Ok))
            .await
            .unwrap();
        sources
            .record_health("coindesk", at(), 1, None)
            .await
            .unwrap();
        let items = InMemoryNewsItemRepository::new();
        let clusters = InMemoryNewsClusterRepository::new();
        let fetcher = StubFetcher {
            result: Ok(vec![raw_item("https://example.com/story")]),
        };

        let error = run_news_cycle(
            &sources,
            &items,
            &clusters,
            &fetcher,
            &config(),
            at() + chrono::Duration::seconds(30),
        )
        .await
        .expect_err("no source is due");

        assert!(matches!(error, NewsWorkerError::NoEligibleSources));
        assert!(
            items
                .list(&NewsItemQuery::default())
                .await
                .unwrap()
                .is_empty()
        );
        let later = at() + chrono::Duration::seconds(600);
        let outcome = run_news_cycle(&sources, &items, &clusters, &fetcher, &config(), later)
            .await
            .expect("cycle after the interval");
        assert_eq!(outcome.sources_fetched, 1);
    }

    #[tokio::test]
    async fn records_a_sanitized_health_code_when_a_fetch_fails() {
        let sources = InMemoryNewsSourceRepository::new();
        sources
            .upsert_seed(source_input("coindesk", NewsLicenseStatus::Ok))
            .await
            .unwrap();
        let items = InMemoryNewsItemRepository::new();
        let clusters = InMemoryNewsClusterRepository::new();
        let fetcher = StubFetcher {
            result: Err(NewsFetchError::Status { status: 429 }),
        };

        let error = run_news_cycle(&sources, &items, &clusters, &fetcher, &config(), at())
            .await
            .expect_err("all sources failed");

        assert!(matches!(
            error,
            NewsWorkerError::AllSourcesFailed { sources: 1 }
        ));
        let source = sources.get("coindesk").await.unwrap().unwrap();
        assert_eq!(source.health().last_error.as_deref(), Some("rate_limited"));
        assert!(
            items
                .list(&NewsItemQuery::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_license_gate_blocks_an_unverified_source() {
        let sources = InMemoryNewsSourceRepository::new();
        sources
            .upsert_seed(source_input("sketchy", NewsLicenseStatus::Unknown))
            .await
            .unwrap();
        let items = InMemoryNewsItemRepository::new();
        let clusters = InMemoryNewsClusterRepository::new();
        let fetcher = StubFetcher {
            result: Ok(vec![raw_item("https://example.com/story")]),
        };

        let error = run_news_cycle(&sources, &items, &clusters, &fetcher, &config(), at())
            .await
            .expect_err("gated source is never polled");

        assert!(matches!(error, NewsWorkerError::NoEligibleSources));
        assert!(
            items
                .list(&NewsItemQuery::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_cycle_without_write_authority_does_nothing() {
        let sources = InMemoryNewsSourceRepository::new();
        sources
            .upsert_seed(source_input("coindesk", NewsLicenseStatus::Ok))
            .await
            .unwrap();
        let items = InMemoryNewsItemRepository::new();
        let clusters = InMemoryNewsClusterRepository::new();
        let fetcher = StubFetcher {
            result: Ok(vec![raw_item("https://example.com/story")]),
        };
        let mut disabled = config();
        disabled.source_of_truth = false;

        let error = run_news_cycle(&sources, &items, &clusters, &fetcher, &disabled, at())
            .await
            .expect_err("write authority is required");

        assert!(matches!(error, NewsWorkerError::WriteAuthorityDisabled));
        assert!(
            items
                .list(&NewsItemQuery::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn readiness_requires_a_successful_cycle() {
        let mut worker = NewsWorker::new("news-worker@local-agent:1", config());
        assert!(worker.ready_after_success(at()).is_none());

        let message = worker.cycle(
            at(),
            CycleResult::success(
                NewsCycleOutcome {
                    sources_fetched: 1,
                    inserted: 2,
                    ..NewsCycleOutcome::default()
                },
                5,
            ),
        );
        assert!(matches!(message, WorkerMessage::Cycle { ok: true, .. }));
        assert!(worker.ready_after_success(at()).is_some());
    }
}
