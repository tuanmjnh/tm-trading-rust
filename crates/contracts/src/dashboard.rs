use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// High-density aggregated metrics for the Desktop Unified Dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DashboardSummaryDto {
    pub generated_at: DateTime<Utc>,
    pub services: ServicesSummaryDto,
    pub market: MarketSummaryDto,
    pub research: ResearchSummaryDto,
    pub news: NewsSummaryDto,
    pub ai: AiSummaryDto,
    pub notifications: NotificationsSummaryDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ServicesSummaryDto {
    pub total: usize,
    pub running: usize,
    pub ready: usize,
    pub degraded: usize,
    pub failed: usize,
    pub stopped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MarketSummaryDto {
    pub tracked_symbols: usize,
    pub total_candles: usize,
    pub last_candle_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ResearchSummaryDto {
    pub total_events: usize,
    pub events_24h: usize,
    pub last_event_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NewsSummaryDto {
    pub total_articles: usize,
    pub articles_24h: usize,
    pub total_sources: usize,
    pub active_sources: usize,
    pub last_article_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AiSummaryDto {
    pub queued_jobs: usize,
    pub running_jobs: usize,
    pub completed_jobs: usize,
    pub failed_jobs: usize,
    pub total_briefings: usize,
    pub last_briefing_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationsSummaryDto {
    pub queued: usize,
    pub sent: usize,
    pub failed: usize,
    pub last_sent_at: Option<DateTime<Utc>>,
    pub telegram_configured: bool,
}
