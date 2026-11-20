use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{NewResearchEvent, ResearchEvent, ResearchQuality};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{
    RepositoryError, ResearchEventQuery, ResearchEventRepository, ResearchEventUpsertOutcome,
};

/// Durable `research_events` store.
///
/// The identifier is the domain identifier, so the same observation is written
/// once: a replayed window updates the existing row instead of adding a copy.
pub struct MongoResearchEventRepository {
    collection: Collection<Document>,
}

impl MongoResearchEventRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("research_events");
        let owner_index = IndexModel::builder()
            .keys(doc! { "ownerScope": 1, "detectedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("ownerScope_detectedAt".to_owned())
                    .build(),
            )
            .build();
        let symbol_index = IndexModel::builder()
            .keys(doc! { "ownerScope": 1, "symbol": 1, "timeframe": 1, "detectedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("ownerScope_symbol_timeframe_detectedAt".to_owned())
                    .build(),
            )
            .build();
        collection
            .create_indexes(vec![owner_index, symbol_index])
            .await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl ResearchEventRepository for MongoResearchEventRepository {
    async fn upsert(
        &self,
        events: &[ResearchEvent],
    ) -> Result<ResearchEventUpsertOutcome, RepositoryError> {
        let mut outcome = ResearchEventUpsertOutcome::default();
        for event in events {
            let filter = doc! { "_id": event.id() };
            let update = doc! {
                "$set": fields(event)?,
            };
            let result = self.collection.update_one(filter, update).await?;
            if result.matched_count == 0 {
                self.collection.insert_one(to_document(event)?).await?;
                outcome.inserted += 1;
            } else {
                outcome.existing += 1;
            }
        }
        Ok(outcome)
    }

    async fn list(
        &self,
        query: &ResearchEventQuery,
    ) -> Result<Vec<ResearchEvent>, RepositoryError> {
        let mut filter = Document::new();
        if let Some(owner) = &query.owner_scope {
            filter.insert("ownerScope", owner.clone());
        }
        if let Some(symbol) = &query.symbol {
            filter.insert("symbol", symbol.clone());
        }
        if let Some(timeframe) = &query.timeframe {
            filter.insert("timeframe", timeframe.clone());
        }
        if let Some(event_type) = &query.event_type {
            filter.insert("eventType", event_type.clone());
        }
        if let Some(since) = query.since {
            filter.insert("detectedAt", doc! { "$gte": timestamp(since) });
        }
        let limit = i64::try_from(query.limit).unwrap_or(i64::MAX);
        let mut cursor = self
            .collection
            .find(filter)
            .sort(doc! { "detectedAt": -1, "_id": -1 })
            .limit(limit)
            .await?;
        let mut events = Vec::new();
        while cursor.advance().await? {
            events.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(events)
    }

    async fn get(&self, event_id: &str) -> Result<Option<ResearchEvent>, RepositoryError> {
        let Some(document) = self.collection.find_one(doc! { "_id": event_id }).await? else {
            return Ok(None);
        };
        Ok(Some(from_document(document)?))
    }
}

fn to_document(event: &ResearchEvent) -> Result<Document, RepositoryError> {
    let mut document = fields(event)?;
    document.insert("_id", event.id());
    Ok(document)
}

fn fields(event: &ResearchEvent) -> Result<Document, RepositoryError> {
    let evidence: Document = event
        .evidence()
        .iter()
        .map(|(key, value)| (key.clone(), Bson::Double(*value)))
        .collect();
    let context: Document = event
        .context()
        .iter()
        .map(|(key, value)| (key.clone(), Bson::String(value.clone())))
        .collect();
    Ok(doc! {
        "ownerScope": event.owner_scope(),
        "symbol": event.symbol(),
        "timeframe": event.timeframe(),
        "method": event.method(),
        "methodVersion": event.method_version(),
        "eventType": event.event_type(),
        "direction": i32::from(event.direction()),
        "score": event.score(),
        "observedAt": timestamp(event.observed_at()),
        "detectedAt": timestamp(event.detected_at()),
        "ingestedAt": timestamp(event.ingested_at()),
        "sourceId": event.source_id(),
        "providerMethod": event.provider_method(),
        "inputSnapshotId": event.input_snapshot_id(),
        "producerVersion": event.producer_version(),
        "schemaVersion": event.schema_version(),
        "qualityState": event.quality_state().as_str(),
        "qualityReasons": event.quality_reasons().to_vec(),
        "evidence": evidence,
        "context": context,
    })
}

fn from_document(document: Document) -> Result<ResearchEvent, RepositoryError> {
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let timestamp_of = |field: &'static str| -> Result<DateTime<Utc>, RepositoryError> {
        document
            .get_datetime(field)
            .map_err(|_| RepositoryError::CorruptRecord(field))
            .and_then(|value| from_timestamp(*value))
    };
    let number = |field: &'static str| -> Result<f64, RepositoryError> {
        document
            .get(field)
            .ok_or(RepositoryError::CorruptRecord(field))
            .and_then(|value| crate::bson_support::number_as_f64(field, value))
    };
    let evidence = document
        .get_document("evidence")
        .map_err(|_| RepositoryError::CorruptRecord("evidence"))?
        .iter()
        .map(|(key, value)| {
            crate::bson_support::number_as_f64("evidence", value).map(|value| (key.clone(), value))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let context = document
        .get_document("context")
        .map_err(|_| RepositoryError::CorruptRecord("context"))?
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(str::to_owned)
                .map(|value| (key.clone(), value))
                .ok_or(RepositoryError::CorruptRecord("context"))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let quality_reasons = document
        .get_array("qualityReasons")
        .map_err(|_| RepositoryError::CorruptRecord("qualityReasons"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(RepositoryError::CorruptRecord("qualityReasons"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let quality_state = ResearchQuality::parse(&text("qualityState")?)
        .ok_or(RepositoryError::CorruptRecord("qualityState"))?;
    let direction = document
        .get_i32("direction")
        .map_err(|_| RepositoryError::CorruptRecord("direction"))?;

    ResearchEvent::new(NewResearchEvent {
        owner_scope: text("ownerScope")?,
        symbol: text("symbol")?,
        timeframe: text("timeframe")?,
        method: text("method")?,
        method_version: text("methodVersion")?,
        event_type: text("eventType")?,
        direction: i8::try_from(direction)
            .map_err(|_| RepositoryError::CorruptRecord("direction"))?,
        score: number("score")?,
        observed_at: timestamp_of("observedAt")?,
        detected_at: timestamp_of("detectedAt")?,
        ingested_at: timestamp_of("ingestedAt")?,
        source_id: text("sourceId")?,
        provider_method: text("providerMethod")?,
        input_snapshot_id: text("inputSnapshotId")?,
        quality_state,
        quality_reasons,
        evidence,
        context,
    })
    .map_err(RepositoryError::from)
}
