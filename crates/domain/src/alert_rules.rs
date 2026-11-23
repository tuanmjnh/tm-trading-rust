//! Rule-based alert evaluation and notification builders.
//!
//! Evaluates market intelligence and research events against rule thresholds and
//! constructs deduplicated [`NotificationOutboxItem`] records ready for dispatch
//! via Telegram and desktop channels.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::market_intel::{FundingRateRecord, LiquidationSide, LiquidationZone};
use crate::notification::{
    NotificationChannel, NotificationError, NotificationOutboxItem, NotificationPriority,
};
use crate::research::ResearchEvent;

/// Types of trigger conditions supported by the alert rule engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertRuleKind {
    FundingExtreme { threshold: f64 },
    LiquidationSpike { min_usd: f64 },
    ResearchEventScore { min_score: f64 },
    PatternMatch { pattern: String },
}

/// A user or system alert rule definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub condition: AlertRuleKind,
    pub channel: NotificationChannel,
    pub priority: NotificationPriority,
}

impl AlertRule {
    #[must_use]
    pub fn default_funding_rule() -> Self {
        Self {
            id: "rule_funding_extreme".to_owned(),
            name: "Extreme Funding Rate (>=0.05%)".to_owned(),
            enabled: true,
            condition: AlertRuleKind::FundingExtreme { threshold: 0.0005 },
            channel: NotificationChannel::Telegram,
            priority: NotificationPriority::Important,
        }
    }

    #[must_use]
    pub fn default_research_rule() -> Self {
        Self {
            id: "rule_research_high_conviction".to_owned(),
            name: "High Conviction Pattern (|score| >= 0.7)".to_owned(),
            enabled: true,
            condition: AlertRuleKind::ResearchEventScore { min_score: 0.7 },
            channel: NotificationChannel::Telegram,
            priority: NotificationPriority::Normal,
        }
    }
}

/// Builds a deduplicated [`NotificationOutboxItem`] for an extreme funding rate event.
pub fn build_funding_alert(
    record: &FundingRateRecord,
    channel: NotificationChannel,
    priority: NotificationPriority,
    now: DateTime<Utc>,
) -> Result<NotificationOutboxItem, NotificationError> {
    let notify_id = format!("funding:{}:{}", record.symbol, record.next_funding_time);
    let mut item = NotificationOutboxItem::new(
        notify_id,
        channel,
        "funding_extreme".to_owned(),
        priority,
        now,
        now,
    )?;

    let payer = if record.rate > 0.0 {
        "long trả short"
    } else {
        "short trả long"
    };
    let oi_str = record
        .oi_notional_usd
        .map(|usd| format!(" · OI ${:.1}M", usd / 1_000_000.0))
        .unwrap_or_default();

    item.symbol = Some(record.symbol.clone());
    item.title = Some(format!(
        "💸 TM Trading — FUNDING CỰC ĐOAN ({})",
        record.symbol
    ));
    item.body = Some(format!(
        "• {}: {:.4}% ({}) · Mark ${:.2}{}\nKỳ kế tiếp tính theo nextFundingTime.",
        record.symbol, record.pct, payer, record.price, oi_str
    ));

    Ok(item)
}

/// Builds a deduplicated [`NotificationOutboxItem`] for an observed research signal.
pub fn build_research_alert(
    event: &ResearchEvent,
    channel: NotificationChannel,
    priority: NotificationPriority,
    now: DateTime<Utc>,
) -> Result<NotificationOutboxItem, NotificationError> {
    let notify_id = format!("research:{}:{}", event.symbol(), event.id());
    let mut item = NotificationOutboxItem::new(
        notify_id,
        channel,
        "research_signal".to_owned(),
        priority,
        now,
        now,
    )?;

    let dir_str = match event.direction() {
        1 => "BULLISH 🟢",
        -1 => "BEARISH 🔴",
        _ => "NEUTRAL ⚪",
    };

    item.symbol = Some(event.symbol().to_owned());
    item.title = Some(format!(
        "🎯 TM Trading — Tín hiệu kỹ thuật [{} - {}]",
        event.symbol(),
        event.method().to_uppercase()
    ));
    item.body = Some(format!(
        "• Cặp: {}\n• Khung thời gian: {}\n• Phương pháp: {}\n• Mẫu hình: {} ({})\n• Điểm tin cậy: {:.2}\n• Thời gian ghi nhận: {}",
        event.symbol(),
        event.timeframe(),
        event.method().to_uppercase(),
        event.event_type(),
        dir_str,
        event.score(),
        event.observed_at().format("%Y-%m-%d %H:%M:%S UTC")
    ));

    Ok(item)
}

/// Builds a deduplicated [`NotificationOutboxItem`] for an estimated liquidation zone alert.
pub fn build_liquidation_alert(
    symbol: &str,
    zone: &LiquidationZone,
    channel: NotificationChannel,
    priority: NotificationPriority,
    now: DateTime<Utc>,
) -> Result<NotificationOutboxItem, NotificationError> {
    let bucket_hour = now.timestamp() / 3600;
    let notify_id = format!(
        "liq:{}:{}:{}:{}",
        symbol,
        zone.side.as_str(),
        zone.lev as u32,
        bucket_hour
    );

    let mut item = NotificationOutboxItem::new(
        notify_id,
        channel,
        "liquidation_cluster".to_owned(),
        priority,
        now,
        now,
    )?;

    let side_str = match zone.side {
        LiquidationSide::Long => "LONG thanh lý",
        LiquidationSide::Short => "SHORT thanh lý",
    };

    item.symbol = Some(symbol.to_owned());
    item.title = Some(format!("💥 TM Trading — Vùng thanh lý lớn [{symbol}]"));
    item.body = Some(format!(
        "• Vùng: {:.0}x {}\n• Giá ước tính: ${:.2} ({:+.2}%)\n• Khối lượng dự kiến: ~${:.1}M",
        zone.lev,
        side_str,
        zone.price,
        zone.pct,
        zone.usd / 1_000_000.0
    ));

    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{NewResearchEvent, ResearchQuality};
    use std::collections::BTreeMap;

    #[test]
    fn builds_funding_alert_correctly() {
        let record = FundingRateRecord::new(
            "SOLUSDT",
            0.0012,
            1700000000000,
            150.0,
            Some(100_000.0),
            Some(15_000_000.0),
        );
        let now = Utc::now();
        let item = build_funding_alert(
            &record,
            NotificationChannel::Telegram,
            NotificationPriority::Important,
            now,
        )
        .expect("build funding alert");

        assert_eq!(item.notify_id, "funding:SOLUSDT:1700000000000");
        assert_eq!(item.id, "funding:SOLUSDT:1700000000000:telegram");
        assert_eq!(item.priority, NotificationPriority::Important);
        assert!(item.title.unwrap().contains("SOLUSDT"));
        assert!(item.body.unwrap().contains("15.0M"));
    }

    #[test]
    fn builds_research_alert_correctly() {
        let now = Utc::now();
        let event = ResearchEvent::new(NewResearchEvent {
            owner_scope: "test".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: "1h".to_owned(),
            method: "price-action".to_owned(),
            method_version: "1".to_owned(),
            event_type: "PIN BAR LONG".to_owned(),
            direction: 1,
            score: 0.8,
            observed_at: now,
            detected_at: now,
            ingested_at: now,
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: "snap_1".to_owned(),
            quality_state: ResearchQuality::Complete,
            quality_reasons: Vec::new(),
            evidence: BTreeMap::new(),
            context: BTreeMap::new(),
        })
        .expect("valid research event");

        let item = build_research_alert(
            &event,
            NotificationChannel::Telegram,
            NotificationPriority::Normal,
            now,
        )
        .expect("build research alert");

        assert!(item.notify_id.starts_with("research:BTCUSDT:"));
        assert_eq!(item.priority, NotificationPriority::Normal);
        assert!(item.title.unwrap().contains("BTCUSDT"));
        assert!(item.body.unwrap().contains("BULLISH 🟢"));
    }
}
