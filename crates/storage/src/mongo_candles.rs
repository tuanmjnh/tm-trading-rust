use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::{IndexOptions, WriteConcern};
use mongodb::{ClientSession, Collection, Database, IndexModel};

use tm_trading_domain::{CANDLE_SCHEMA_VERSION, Candle, NewCandle};

use crate::bson_support::{number_as_f64, number_as_i64, timestamp};
use crate::{CandleRepository, CandleUpsertOutcome, RepositoryError};

/// Durable `candles` store matching the legacy recorder's document shape.
///
/// Rows are closed bars keyed by `{symbol}:{timeframe}:{openTime}`, so re-recording the
/// same close is an upsert rather than a duplicate. Forming bars are never accepted here.
#[derive(Clone)]
pub struct MongoCandleRepository {
    collection: Collection<Document>,
}

impl MongoCandleRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("candles");
        let index = |name: &str, keys: Document| {
            IndexModel::builder()
                .keys(keys)
                .options(IndexOptions::builder().name(name.to_owned()).build())
                .build()
        };
        collection
            .create_indexes(vec![
                index(
                    "symbol_timeframe_openTime",
                    doc! { "symbol": 1, "timeframe": 1, "openTime": -1 },
                ),
                index("openTime", doc! { "openTime": -1 }),
                index("source_openTime", doc! { "source": 1, "openTime": -1 }),
            ])
            .await?;
        Ok(Self { collection })
    }

    async fn upsert_all(
        &self,
        candles: &[Candle],
        mut session: Option<&mut ClientSession>,
    ) -> Result<CandleUpsertOutcome, RepositoryError> {
        let mut outcome = CandleUpsertOutcome::default();
        for candle in candles {
            let mut set = to_document(candle);
            set.remove("_id");
            // `createdAt` mirrors the legacy recorder, which derives it from `ingestTime`.
            let created_at = Utc
                .timestamp_millis_opt(candle.ingest_time())
                .single()
                .ok_or(RepositoryError::CorruptRecord("ingestTime"))?;
            let update = doc! {
                "$set": set,
                "$setOnInsert": {
                    "_id": candle.id(),
                    "createdAt": timestamp(created_at),
                },
            };
            let result = match session.as_deref_mut() {
                Some(session) => {
                    self.collection
                        .update_one(doc! { "_id": candle.id() }, update)
                        .session(session)
                        .upsert(true)
                        .await?
                }
                None => {
                    self.collection
                        .update_one(doc! { "_id": candle.id() }, update)
                        .upsert(true)
                        .await?
                }
            };
            if result.upserted_id.is_some() {
                outcome.inserted += 1;
            } else {
                outcome.existing += 1;
            }
        }
        Ok(outcome)
    }
}

#[async_trait]
impl CandleRepository for MongoCandleRepository {
    async fn upsert_closed(
        &self,
        candles: &[Candle],
    ) -> Result<CandleUpsertOutcome, RepositoryError> {
        if candles.is_empty() {
            return Ok(CandleUpsertOutcome::default());
        }
        if candles.len() == 1 {
            return self.upsert_all(candles, None).await;
        }

        // A multi-symbol cycle must be all-or-nothing. Transactions require MongoDB to run
        // as a replica set, which is the production topology for durable candle batches.
        let mut session = self.collection.client().start_session().await?;
        session
            .start_transaction()
            .write_concern(WriteConcern::majority())
            .await?;
        match self.upsert_all(candles, Some(&mut session)).await {
            Ok(outcome) => {
                commit(&mut session).await?;
                Ok(outcome)
            }
            Err(error) => {
                let _ = session.abort_transaction().await;
                Err(error)
            }
        }
    }

    async fn recent(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: usize,
    ) -> Result<Vec<Candle>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut cursor = self
            .collection
            .find(doc! { "symbol": symbol, "timeframe": timeframe })
            .sort(doc! { "openTime": -1 })
            .limit(i64::try_from(limit).unwrap_or(i64::MAX))
            .await?;
        let mut candles = Vec::new();
        while cursor.advance().await? {
            candles.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(candles)
    }

    async fn get(&self, candle_id: &str) -> Result<Option<Candle>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": candle_id })
            .await?
            .map(from_document)
            .transpose()
    }
}

/// Commits with a bounded retry for `UnknownTransactionCommitResult`.
///
/// The commit is idempotent, so retrying cannot duplicate the batch. Reporting the batch as
/// failed while it may have committed would be worse: the caller could retry a writer cycle
/// that already succeeded.
async fn commit(session: &mut ClientSession) -> Result<(), RepositoryError> {
    const MAX_COMMIT_ATTEMPTS: usize = 3;
    let mut attempts = 0;
    loop {
        attempts += 1;
        match session.commit_transaction().await {
            Ok(()) => return Ok(()),
            Err(error)
                if attempts < MAX_COMMIT_ATTEMPTS
                    && error.contains_label(mongodb::error::UNKNOWN_TRANSACTION_COMMIT_RESULT) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn to_document(candle: &Candle) -> Document {
    let mut document = doc! {
        "_id": candle.id(),
        "symbol": candle.symbol(),
        "timeframe": candle.timeframe(),
        "source": candle.source(),
        "state": "closed",
        "open": candle.open(),
        "high": candle.high(),
        "low": candle.low(),
        "close": candle.close(),
        "volume": candle.volume(),
        "openTime": candle.open_time(),
        "closeTime": candle.close_time(),
        "eventTime": candle.event_time(),
        "ingestTime": candle.ingest_time(),
        "schemaVersion": candle.schema_version(),
    };
    match candle.market() {
        Some(market) => {
            document.insert("market", market);
        }
        None => {
            document.insert("market", Bson::Null);
        }
    }
    document
}

fn from_document(document: Document) -> Result<Candle, RepositoryError> {
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let number = |field: &'static str| -> Result<f64, RepositoryError> {
        document
            .get(field)
            .ok_or(RepositoryError::CorruptRecord(field))
            .and_then(|value| number_as_f64(field, value))
    };
    let integer = |field: &'static str| -> Result<i64, RepositoryError> {
        document
            .get(field)
            .ok_or(RepositoryError::CorruptRecord(field))
            .and_then(|value| number_as_i64(field, value))
    };
    let optional_number = |field: &'static str| -> Result<Option<f64>, RepositoryError> {
        match document.get(field) {
            None | Some(Bson::Null) => Ok(None),
            Some(value) => number_as_f64(field, value).map(Some),
        }
    };
    let optional_integer = |field: &'static str| -> Result<Option<i64>, RepositoryError> {
        match document.get(field) {
            None | Some(Bson::Null) => Ok(None),
            Some(value) => number_as_i64(field, value).map(Some),
        }
    };

    if text("state")? != "closed" {
        return Err(RepositoryError::CorruptRecord("state"));
    }
    let market = match document.get("market") {
        None | Some(Bson::Null) => None,
        Some(Bson::String(value)) => Some(value.clone()),
        Some(_) => return Err(RepositoryError::CorruptRecord("market")),
    };
    // Rows written before the schema stamp existed are still readable: the stamp only
    // guards against a future incompatible shape.
    let stored_version = match document.get("schemaVersion") {
        None | Some(Bson::Null) => CANDLE_SCHEMA_VERSION.to_owned(),
        Some(Bson::String(value)) => value.clone(),
        Some(_) => return Err(RepositoryError::CorruptRecord("schemaVersion")),
    };
    if stored_version != CANDLE_SCHEMA_VERSION {
        return Err(RepositoryError::CorruptRecord("schemaVersion"));
    }

    let candle = Candle::new(NewCandle {
        symbol: text("symbol")?,
        timeframe: text("timeframe")?,
        market,
        source: text("source")?,
        open: number("open")?,
        high: number("high")?,
        low: number("low")?,
        close: number("close")?,
        // The legacy schema defaults a missing volume to 0, so old rows stay readable.
        volume: optional_number("volume")?.unwrap_or(0.0),
        open_time: integer("openTime")?,
        close_time: integer("closeTime")?,
        event_time: optional_integer("eventTime")?,
        ingest_time: optional_integer("ingestTime")?,
    })?;
    if candle.id() != text("_id")? {
        return Err(RepositoryError::CorruptRecord("_id"));
    }
    Ok(candle)
}
