use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::Candle;

use crate::{CandleRepository, CandleUpsertOutcome, RepositoryError};

#[derive(Default)]
pub struct InMemoryCandleRepository {
    candles: Mutex<HashMap<String, Candle>>,
}

impl InMemoryCandleRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CandleRepository for InMemoryCandleRepository {
    async fn upsert_closed(
        &self,
        candles: &[Candle],
    ) -> Result<CandleUpsertOutcome, RepositoryError> {
        let mut stored = self
            .candles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut outcome = CandleUpsertOutcome::default();
        for candle in candles {
            if stored
                .insert(candle.id().to_owned(), candle.clone())
                .is_some()
            {
                outcome.existing += 1;
            } else {
                outcome.inserted += 1;
            }
        }
        Ok(outcome)
    }

    async fn recent(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: usize,
    ) -> Result<Vec<Candle>, RepositoryError> {
        let stored = self
            .candles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut candles: Vec<Candle> = stored
            .values()
            .filter(|candle| candle.symbol() == symbol && candle.timeframe() == timeframe)
            .cloned()
            .collect();
        candles.sort_by_key(|candle| std::cmp::Reverse(candle.open_time()));
        candles.truncate(limit);
        Ok(candles)
    }

    async fn get(&self, candle_id: &str) -> Result<Option<Candle>, RepositoryError> {
        let stored = self
            .candles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(candle_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_trading_domain::NewCandle;

    fn candle(open_time: i64) -> Candle {
        Candle::new(NewCandle {
            symbol: "BTCUSDT".to_owned(),
            timeframe: "1m".to_owned(),
            market: None,
            source: "binance".to_owned(),
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 1.0,
            open_time,
            close_time: open_time + 59_999,
            event_time: None,
            ingest_time: None,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn re_recording_the_same_close_keeps_one_row() {
        let repository = InMemoryCandleRepository::new();
        let bar = candle(1_700_000_000_000);

        assert_eq!(
            repository
                .upsert_closed(std::slice::from_ref(&bar))
                .await
                .unwrap(),
            CandleUpsertOutcome {
                inserted: 1,
                existing: 0
            }
        );
        assert_eq!(
            repository
                .upsert_closed(std::slice::from_ref(&bar))
                .await
                .unwrap(),
            CandleUpsertOutcome {
                inserted: 0,
                existing: 1
            }
        );
        assert_eq!(
            repository.recent("BTCUSDT", "1m", 10).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn recent_is_newest_first_and_bounded() {
        let repository = InMemoryCandleRepository::new();
        let older = candle(1_700_000_000_000);
        let newer = candle(1_700_000_060_000);
        repository
            .upsert_closed(&[older, newer.clone()])
            .await
            .unwrap();

        assert_eq!(
            repository.recent("BTCUSDT", "1m", 1).await.unwrap(),
            vec![newer]
        );
    }
}
