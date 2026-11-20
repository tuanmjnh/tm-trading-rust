use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::ResearchEvent;

use crate::{RepositoryError, ResearchEventRepository, ResearchEventUpsertOutcome};

/// In-memory research event store, used by the local memory deployment mode.
#[derive(Default)]
pub struct InMemoryResearchEventRepository {
    events: Mutex<HashMap<String, ResearchEvent>>,
}

impl InMemoryResearchEventRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResearchEventRepository for InMemoryResearchEventRepository {
    async fn upsert(
        &self,
        events: &[ResearchEvent],
    ) -> Result<ResearchEventUpsertOutcome, RepositoryError> {
        let mut stored = self
            .events
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut outcome = ResearchEventUpsertOutcome::default();
        for event in events {
            if stored
                .insert(event.id().to_owned(), event.clone())
                .is_some()
            {
                outcome.existing += 1;
            } else {
                outcome.inserted += 1;
            }
        }
        Ok(outcome)
    }

    async fn list(
        &self,
        query: &crate::ResearchEventQuery,
    ) -> Result<Vec<ResearchEvent>, RepositoryError> {
        let stored = self
            .events
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut events: Vec<ResearchEvent> = stored
            .values()
            .filter(|event| query.matches(event))
            .cloned()
            .collect();
        // Newest observation first; the identifier breaks ties so the order is total.
        events.sort_by(|left, right| {
            right
                .detected_at()
                .cmp(&left.detected_at())
                .then_with(|| right.id().cmp(left.id()))
        });
        events.truncate(query.limit);
        Ok(events)
    }

    async fn get(&self, event_id: &str) -> Result<Option<ResearchEvent>, RepositoryError> {
        let stored = self
            .events
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(event_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResearchEventQuery;
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::BTreeMap;
    use tm_trading_domain::{NewResearchEvent, ResearchQuality};

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, minute as u32, 0)
            .unwrap()
    }

    fn event(symbol: &str, event_type: &str, minute: i64) -> ResearchEvent {
        ResearchEvent::new(NewResearchEvent {
            owner_scope: "local-operator".to_owned(),
            symbol: symbol.to_owned(),
            timeframe: "15m".to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            event_type: event_type.to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: at(minute),
            detected_at: at(minute),
            ingested_at: at(minute),
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: "rsnap1_1".to_owned(),
            quality_state: ResearchQuality::Complete,
            quality_reasons: Vec::new(),
            evidence: BTreeMap::from([("close".to_owned(), 100.0)]),
            context: BTreeMap::new(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn re_recording_the_same_event_keeps_one_row() {
        let repository = InMemoryResearchEventRepository::new();
        let event = event("BTCUSDT", "ST LONG", 0);

        let first = repository
            .upsert(std::slice::from_ref(&event))
            .await
            .unwrap();
        let second = repository
            .upsert(std::slice::from_ref(&event))
            .await
            .unwrap();

        assert_eq!(first.inserted, 1);
        assert_eq!(second.existing, 1);
        assert_eq!(
            repository
                .list(&ResearchEventQuery::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn lists_newest_first_and_filters_by_owner_and_symbol() {
        let repository = InMemoryResearchEventRepository::new();
        let events = vec![
            event("BTCUSDT", "ST LONG", 0),
            event("BTCUSDT", "BC", 15),
            event("ETHUSDT", "SV", 30),
        ];
        repository.upsert(&events).await.unwrap();

        let all = repository
            .list(&ResearchEventQuery::default())
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].event_type(), "SV");

        let filtered = repository
            .list(&ResearchEventQuery {
                symbol: Some("BTCUSDT".to_owned()),
                ..ResearchEventQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].event_type(), "BC");

        let foreign = repository
            .list(&ResearchEventQuery {
                owner_scope: Some("someone-else".to_owned()),
                ..ResearchEventQuery::default()
            })
            .await
            .unwrap();
        assert!(foreign.is_empty());
    }

    #[tokio::test]
    async fn the_limit_is_applied() {
        let repository = InMemoryResearchEventRepository::new();
        let events = vec![
            event("BTCUSDT", "ST LONG", 0),
            event("BTCUSDT", "BC", 15),
            event("BTCUSDT", "SV", 30),
        ];
        repository.upsert(&events).await.unwrap();

        let limited = repository
            .list(&ResearchEventQuery {
                limit: 2,
                ..ResearchEventQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(limited.len(), 2);
    }
}
