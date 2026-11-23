use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::TryStreamExt;
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::{FindOptions, IndexOptions, ReturnDocument};
use mongodb::{Collection, Database, IndexModel};

use tm_trading_contracts::NotificationOutboxSummaryDto;
use tm_trading_domain::{
    DEFAULT_OUTBOX_MAX_ATTEMPTS, NotificationAttemptRecord, NotificationChannel,
    NotificationLogItem, NotificationOutboxItem, NotificationPriority, NotificationState,
    OUTBOX_SCHEMA_VERSION, ROUTER_SCHEMA_VERSION,
};

use crate::bson_support::{from_timestamp, is_duplicate_key, number_as_i64, timestamp};
use crate::notifications::{
    EnqueueNotificationOutcome, NotificationLogQuery, NotificationLogRepository,
    NotificationOutboxQuery, NotificationOutboxRepository, RecordNotificationLogOutcome,
};
use crate::RepositoryError;

fn parse_millis_or_date(doc: &Document, field: &'static str) -> Result<DateTime<Utc>, RepositoryError> {
    match doc.get(field) {
        Some(Bson::DateTime(dt)) => from_timestamp(*dt),
        Some(Bson::Int64(ms)) => Utc
            .timestamp_millis_opt(*ms)
            .single()
            .ok_or(RepositoryError::CorruptRecord(field)),
        Some(Bson::Int32(ms)) => Utc
            .timestamp_millis_opt(i64::from(*ms))
            .single()
            .ok_or(RepositoryError::CorruptRecord(field)),
        Some(Bson::Double(ms)) if ms.is_finite() => Utc
            .timestamp_millis_opt(*ms as i64)
            .single()
            .ok_or(RepositoryError::CorruptRecord(field)),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn parse_opt_millis_or_date(
    doc: &Document,
    field: &'static str,
) -> Result<Option<DateTime<Utc>>, RepositoryError> {
    match doc.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::DateTime(dt)) => from_timestamp(*dt).map(Some),
        Some(Bson::Int64(ms)) => Utc
            .timestamp_millis_opt(*ms)
            .single()
            .map(Some)
            .ok_or(RepositoryError::CorruptRecord(field)),
        Some(Bson::Int32(ms)) => Utc
            .timestamp_millis_opt(i64::from(*ms))
            .single()
            .map(Some)
            .ok_or(RepositoryError::CorruptRecord(field)),
        Some(Bson::Double(ms)) if ms.is_finite() => Utc
            .timestamp_millis_opt(*ms as i64)
            .single()
            .map(Some)
            .ok_or(RepositoryError::CorruptRecord(field)),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

/// Durable `notification_outbox` MongoDB store.
pub struct MongoNotificationOutboxRepository {
    collection: Collection<Document>,
}

impl MongoNotificationOutboxRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        ensure_outbox_indexes(database).await?;
        Ok(Self {
            collection: database.collection::<Document>("notification_outbox"),
        })
    }
}

async fn ensure_outbox_indexes(database: &Database) -> Result<(), RepositoryError> {
    let collection = database.collection::<Document>("notification_outbox");
    let state_next = IndexModel::builder()
        .keys(doc! { "state": 1, "nextAttemptAt": 1 })
        .options(
            IndexOptions::builder()
                .name("state_nextAttemptAt".to_owned())
                .build(),
        )
        .build();
    let channel_state = IndexModel::builder()
        .keys(doc! { "channel": 1, "state": 1 })
        .options(
            IndexOptions::builder()
                .name("channel_state".to_owned())
                .build(),
        )
        .build();
    let notify_id = IndexModel::builder()
        .keys(doc! { "notifyId": 1 })
        .options(IndexOptions::builder().name("notifyId".to_owned()).build())
        .build();
    let owner_id = IndexModel::builder()
        .keys(doc! { "ownerId": 1 })
        .options(IndexOptions::builder().name("ownerId".to_owned()).build())
        .build();

    collection
        .create_indexes(vec![state_next, channel_state, notify_id, owner_id])
        .await?;
    Ok(())
}

fn outbox_to_doc(item: &NotificationOutboxItem) -> Document {
    let mut doc = doc! {
        "_id": &item.id,
        "outboxVersion": &item.outbox_version,
        "notifyId": &item.notify_id,
        "channel": item.channel.as_str(),
        "kind": &item.kind,
        "priority": item.priority.as_str(),
        "at": item.at.timestamp_millis(),
        "state": item.state.as_str(),
        "attempts": item.attempts as i32,
        "dead": item.dead,
        "createdAt": timestamp(item.created_at),
        "updatedAt": timestamp(item.updated_at),
    };

    if let Some(dedupe) = &item.dedupe_key {
        doc.insert("dedupeKey", dedupe);
    }
    if let Some(owner) = &item.owner_id {
        doc.insert("ownerId", owner);
    }
    if let Some(cat) = &item.category {
        doc.insert("category", cat);
    }
    if let Some(preset) = &item.preset {
        doc.insert("preset", preset);
    }
    if let Some(name) = &item.name {
        doc.insert("name", name);
    }
    if let Some(sym) = &item.symbol {
        doc.insert("symbol", sym);
    }
    if let Some(title) = &item.title {
        doc.insert("title", title);
    }
    if let Some(body) = &item.body {
        doc.insert("body", body);
    }
    if let Some(next) = item.next_attempt_at {
        doc.insert("nextAttemptAt", next.timestamp_millis());
    }
    if let Some(err) = &item.last_error {
        doc.insert("lastError", err);
    }
    if let Some(msg) = &item.message_id {
        doc.insert("messageId", msg);
    }
    if let Some(outcome) = item.outcome_at {
        doc.insert("outcomeAt", outcome.timestamp_millis());
    }

    doc
}

fn doc_to_outbox(doc: &Document) -> Result<NotificationOutboxItem, RepositoryError> {
    let id = doc
        .get_str("_id")
        .map_err(|_| RepositoryError::CorruptRecord("_id"))?
        .to_owned();
    let outbox_version = doc
        .get_str("outboxVersion")
        .unwrap_or(OUTBOX_SCHEMA_VERSION)
        .to_owned();
    let notify_id = doc
        .get_str("notifyId")
        .map_err(|_| RepositoryError::CorruptRecord("notifyId"))?
        .to_owned();
    let dedupe_key = doc.get_str("dedupeKey").ok().map(str::to_owned);
    let channel_raw = doc
        .get_str("channel")
        .map_err(|_| RepositoryError::CorruptRecord("channel"))?;
    let channel = NotificationChannel::from_str(channel_raw)
        .map_err(|_| RepositoryError::CorruptRecord("channel"))?;

    let owner_id = doc.get_str("ownerId").ok().map(str::to_owned);
    let category = doc.get_str("category").ok().map(str::to_owned);
    let kind = doc
        .get_str("kind")
        .map_err(|_| RepositoryError::CorruptRecord("kind"))?
        .to_owned();
    let priority_raw = doc
        .get_str("priority")
        .map_err(|_| RepositoryError::CorruptRecord("priority"))?;
    let priority = NotificationPriority::from_str(priority_raw)
        .map_err(|_| RepositoryError::CorruptRecord("priority"))?;

    let preset = doc.get_str("preset").ok().map(str::to_owned);
    let name = doc.get_str("name").ok().map(str::to_owned);
    let symbol = doc.get_str("symbol").ok().map(str::to_owned);
    let title = doc.get_str("title").ok().map(str::to_owned);
    let body = doc.get_str("body").ok().map(str::to_owned);

    let at = parse_millis_or_date(doc, "at")?;
    let state_raw = doc
        .get_str("state")
        .map_err(|_| RepositoryError::CorruptRecord("state"))?;
    let state = NotificationState::from_str(state_raw)
        .map_err(|_| RepositoryError::CorruptRecord("state"))?;

    let attempts = match doc.get("attempts") {
        Some(v) => number_as_i64("attempts", v)? as u32,
        None => 0,
    };
    let next_attempt_at = parse_opt_millis_or_date(doc, "nextAttemptAt")?;
    let last_error = doc.get_str("lastError").ok().map(str::to_owned);
    let message_id = doc.get_str("messageId").ok().map(str::to_owned);
    let dead = doc.get_bool("dead").unwrap_or(false);
    let outcome_at = parse_opt_millis_or_date(doc, "outcomeAt")?;

    let now = Utc::now();
    let created_at = match doc.get_datetime("createdAt") {
        Ok(dt) => from_timestamp(*dt)?,
        Err(_) => at,
    };
    let updated_at = match doc.get_datetime("updatedAt") {
        Ok(dt) => from_timestamp(*dt)?,
        Err(_) => now,
    };

    Ok(NotificationOutboxItem {
        id,
        outbox_version,
        notify_id,
        dedupe_key,
        channel,
        owner_id,
        category,
        kind,
        priority,
        preset,
        name,
        symbol,
        title,
        body,
        at,
        state,
        attempts,
        next_attempt_at,
        last_error,
        message_id,
        dead,
        outcome_at,
        created_at,
        updated_at,
    })
}

#[async_trait]
impl NotificationOutboxRepository for MongoNotificationOutboxRepository {
    async fn enqueue(
        &self,
        item: NotificationOutboxItem,
    ) -> Result<EnqueueNotificationOutcome, RepositoryError> {
        let doc = outbox_to_doc(&item);
        let update = doc! { "$setOnInsert": doc };

        match self
            .collection
            .update_one(doc! { "_id": &item.id }, update)
            .upsert(true)
            .await
        {
            Ok(result) => {
                if result.upserted_id.is_some() {
                    Ok(EnqueueNotificationOutcome::Enqueued(item))
                } else {
                    let existing_doc = self
                        .collection
                        .find_one(doc! { "_id": &item.id })
                        .await?
                        .ok_or(RepositoryError::NotificationOutboxNotFound)?;
                    Ok(EnqueueNotificationOutcome::Existing(doc_to_outbox(
                        &existing_doc,
                    )?))
                }
            }
            Err(e) if is_duplicate_key(&e) => {
                let existing_doc = self
                    .collection
                    .find_one(doc! { "_id": &item.id })
                    .await?
                    .ok_or(RepositoryError::NotificationOutboxNotFound)?;
                Ok(EnqueueNotificationOutcome::Existing(doc_to_outbox(
                    &existing_doc,
                )?))
            }
            Err(e) => Err(RepositoryError::Mongo(e)),
        }
    }

    async fn get(&self, id: &str) -> Result<Option<NotificationOutboxItem>, RepositoryError> {
        match self.collection.find_one(doc! { "_id": id }).await? {
            Some(doc) => Ok(Some(doc_to_outbox(&doc)?)),
            None => Ok(None),
        }
    }

    async fn list(
        &self,
        query: &NotificationOutboxQuery,
    ) -> Result<Vec<NotificationOutboxItem>, RepositoryError> {
        let mut filter = Document::new();
        if let Some(owner) = &query.owner_id {
            filter.insert("ownerId", owner);
        }
        if let Some(channel) = query.channel {
            filter.insert("channel", channel.as_str());
        }
        if let Some(state) = query.state {
            filter.insert("state", state.as_str());
        }
        if let Some(dead) = query.dead {
            filter.insert("dead", dead);
        }

        let options = FindOptions::builder()
            .sort(doc! { "nextAttemptAt": 1, "createdAt": 1 })
            .limit(query.limit as i64)
            .build();

        let mut cursor = self.collection.find(filter).with_options(options).await?;
        let mut items = Vec::new();
        while let Some(doc) = cursor.try_next().await? {
            items.push(doc_to_outbox(&doc)?);
        }
        Ok(items)
    }

    async fn summary(
        &self,
        owner_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxSummaryDto, RepositoryError> {
        let mut filter = Document::new();
        if let Some(owner) = owner_id {
            filter.insert("ownerId", owner);
        }

        let mut cursor = self.collection.find(filter).await?;
        let mut queued = 0;
        let mut sent = 0;
        let mut accepted = 0;
        let mut failed = 0;
        let mut dead = 0;

        while let Some(doc) = cursor.try_next().await? {
            let item = doc_to_outbox(&doc)?;
            if item.dead {
                dead += 1;
            }
            match item.state {
                NotificationState::Queued => queued += 1,
                NotificationState::Sent => sent += 1,
                NotificationState::Accepted => accepted += 1,
                NotificationState::Failed => failed += 1,
            }
        }

        Ok(NotificationOutboxSummaryDto {
            queued,
            sent,
            accepted,
            failed,
            dead,
            generated_at: now,
        })
    }

    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<NotificationOutboxItem>, RepositoryError> {
        let now_ms = now.timestamp_millis();
        let lease_until_ms = (now + lease_duration).timestamp_millis();

        let filter = doc! {
            "state": "queued",
            "dead": false,
            "nextAttemptAt": { "$lte": now_ms },
            "$or": [
                { "leaseUntil": { "$exists": false } },
                { "leaseUntil": null },
                { "leaseUntil": { "$lte": now_ms } }
            ]
        };

        let update = doc! {
            "$set": {
                "leaseHolder": worker_id,
                "leaseUntil": lease_until_ms,
                "updatedAt": timestamp(now)
            }
        };

        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .sort(doc! { "nextAttemptAt": 1, "createdAt": 1 })
            .return_document(ReturnDocument::After)
            .build();

        match self
            .collection
            .find_one_and_update(filter, update)
            .with_options(options)
            .await?
        {
            Some(doc) => Ok(Some(doc_to_outbox(&doc)?)),
            None => Ok(None),
        }
    }

    async fn mark_accepted(
        &self,
        id: &str,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        let filter = doc! { "_id": id };
        let update = doc! {
            "$set": {
                "state": "accepted",
                "messageId": message_id,
                "outcomeAt": now.timestamp_millis(),
                "dead": false,
                "updatedAt": timestamp(now)
            },
            "$unset": {
                "leaseHolder": "",
                "leaseUntil": ""
            }
        };

        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();

        match self
            .collection
            .find_one_and_update(filter, update)
            .with_options(options)
            .await?
        {
            Some(doc) => Ok(doc_to_outbox(&doc)?),
            None => Err(RepositoryError::NotificationOutboxNotFound),
        }
    }

    async fn mark_sent(
        &self,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        let filter = doc! { "_id": id };
        let update = doc! {
            "$set": {
                "state": "sent",
                "outcomeAt": now.timestamp_millis(),
                "updatedAt": timestamp(now)
            },
            "$unset": {
                "leaseHolder": "",
                "leaseUntil": ""
            }
        };

        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();

        match self
            .collection
            .find_one_and_update(filter, update)
            .with_options(options)
            .await?
        {
            Some(doc) => Ok(doc_to_outbox(&doc)?),
            None => Err(RepositoryError::NotificationOutboxNotFound),
        }
    }

    async fn mark_failed(
        &self,
        id: &str,
        error: &str,
        retryable: bool,
        next_attempt_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        // Read current attempts first
        let current = self
            .get(id)
            .await?
            .ok_or(RepositoryError::NotificationOutboxNotFound)?;
        let attempts = current.attempts + 1;

        let (state, dead, next_attempt, outcome_at) =
            if !retryable || attempts >= DEFAULT_OUTBOX_MAX_ATTEMPTS {
                ("failed", true, None, Some(now.timestamp_millis()))
            } else {
                ("queued", false, next_attempt_at.map(|t| t.timestamp_millis()), None)
            };

        let filter = doc! { "_id": id };
        let mut set_doc = doc! {
            "state": state,
            "attempts": attempts as i32,
            "dead": dead,
            "lastError": error,
            "updatedAt": timestamp(now)
        };
        if let Some(next) = next_attempt {
            set_doc.insert("nextAttemptAt", next);
        }
        if let Some(outcome) = outcome_at {
            set_doc.insert("outcomeAt", outcome);
        }

        let mut update = doc! {
            "$set": set_doc,
            "$unset": {
                "leaseHolder": "",
                "leaseUntil": ""
            }
        };

        if next_attempt.is_none() {
            update
                .get_document_mut("$unset")
                .unwrap()
                .insert("nextAttemptAt", "");
        }

        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();

        match self
            .collection
            .find_one_and_update(filter, update)
            .with_options(options)
            .await?
        {
            Some(doc) => Ok(doc_to_outbox(&doc)?),
            None => Err(RepositoryError::NotificationOutboxNotFound),
        }
    }
}

/// Durable `notification_logs` MongoDB store.
pub struct MongoNotificationLogRepository {
    collection: Collection<Document>,
}

impl MongoNotificationLogRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        ensure_log_indexes(database).await?;
        Ok(Self {
            collection: database.collection::<Document>("notification_logs"),
        })
    }
}

async fn ensure_log_indexes(database: &Database) -> Result<(), RepositoryError> {
    let collection = database.collection::<Document>("notification_logs");
    let notify_sent = IndexModel::builder()
        .keys(doc! { "notifyId": 1, "sentAt": -1 })
        .options(
            IndexOptions::builder()
                .name("notifyId_sentAt".to_owned())
                .build(),
        )
        .build();
    let kind_sent = IndexModel::builder()
        .keys(doc! { "kind": 1, "sentAt": -1 })
        .options(IndexOptions::builder().name("kind_sentAt".to_owned()).build())
        .build();
    let priority_sent = IndexModel::builder()
        .keys(doc! { "priority": 1, "sentAt": -1 })
        .options(
            IndexOptions::builder()
                .name("priority_sentAt".to_owned())
                .build(),
        )
        .build();
    let symbol_sent = IndexModel::builder()
        .keys(doc! { "symbol": 1, "sentAt": -1 })
        .options(
            IndexOptions::builder()
                .name("symbol_sentAt".to_owned())
                .build(),
        )
        .build();
    let owner_sent = IndexModel::builder()
        .keys(doc! { "ownerId": 1, "sentAt": -1 })
        .options(
            IndexOptions::builder()
                .name("ownerId_sentAt".to_owned())
                .build(),
        )
        .build();

    collection
        .create_indexes(vec![
            notify_sent,
            kind_sent,
            priority_sent,
            symbol_sent,
            owner_sent,
        ])
        .await?;
    Ok(())
}

fn log_to_doc(item: &NotificationLogItem) -> Document {
    let mut doc = doc! {
        "_id": &item.id,
        "notifyId": &item.notify_id,
        "kind": &item.kind,
        "priority": item.priority.as_str(),
        "at": item.at.timestamp_millis(),
        "sentAt": item.sent_at.timestamp_millis(),
        "deduped": item.deduped,
        "routerVersion": &item.router_version,
    };

    if let Some(owner) = &item.owner_id {
        doc.insert("ownerId", owner);
    }
    if let Some(dedupe) = &item.dedupe_key {
        doc.insert("dedupeKey", dedupe);
    }
    if let Some(preset) = &item.preset {
        doc.insert("preset", preset);
    }
    if let Some(name) = &item.name {
        doc.insert("name", name);
    }
    if let Some(sym) = &item.symbol {
        doc.insert("symbol", sym);
    }
    if let Some(title) = &item.title {
        doc.insert("title", title);
    }
    if let Some(body) = &item.body {
        doc.insert("body", body);
    }

    let attempts: Vec<Document> = item
        .attempts
        .iter()
        .map(|att| {
            let mut att_doc = doc! {
                "channel": att.channel.as_str(),
                "ok": att.ok,
            };
            if let Some(err) = &att.error {
                att_doc.insert("error", err);
            }
            att_doc
        })
        .collect();
    doc.insert("attempts", attempts);

    let delivered: Vec<String> = item
        .delivered
        .iter()
        .map(|ch| ch.as_str().to_owned())
        .collect();
    doc.insert("delivered", delivered);

    doc
}

fn doc_to_log(doc: &Document) -> Result<NotificationLogItem, RepositoryError> {
    let id = doc
        .get_str("_id")
        .map_err(|_| RepositoryError::CorruptRecord("_id"))?
        .to_owned();
    let notify_id = doc
        .get_str("notifyId")
        .map_err(|_| RepositoryError::CorruptRecord("notifyId"))?
        .to_owned();
    let owner_id = doc.get_str("ownerId").ok().map(str::to_owned);
    let dedupe_key = doc.get_str("dedupeKey").ok().map(str::to_owned);
    let kind = doc
        .get_str("kind")
        .map_err(|_| RepositoryError::CorruptRecord("kind"))?
        .to_owned();
    let priority_raw = doc
        .get_str("priority")
        .map_err(|_| RepositoryError::CorruptRecord("priority"))?;
    let priority = NotificationPriority::from_str(priority_raw)
        .map_err(|_| RepositoryError::CorruptRecord("priority"))?;

    let preset = doc.get_str("preset").ok().map(str::to_owned);
    let name = doc.get_str("name").ok().map(str::to_owned);
    let symbol = doc.get_str("symbol").ok().map(str::to_owned);
    let title = doc.get_str("title").ok().map(str::to_owned);
    let body = doc.get_str("body").ok().map(str::to_owned);

    let at = parse_millis_or_date(doc, "at")?;
    let sent_at = parse_millis_or_date(doc, "sentAt")?;

    let attempts = match doc.get_array("attempts") {
        Ok(arr) => {
            let mut records = Vec::new();
            for item in arr {
                if let Some(att_doc) = item.as_document() {
                    let channel_str = att_doc.get_str("channel").unwrap_or("telegram");
                    let channel = NotificationChannel::from_str(channel_str)
                        .unwrap_or(NotificationChannel::Telegram);
                    let ok = att_doc.get_bool("ok").unwrap_or(false);
                    let error = att_doc.get_str("error").ok().map(str::to_owned);
                    records.push(NotificationAttemptRecord {
                        channel,
                        ok,
                        error,
                    });
                }
            }
            records
        }
        Err(_) => Vec::new(),
    };

    let delivered = match doc.get_array("delivered") {
        Ok(arr) => arr
            .iter()
            .filter_map(|b| b.as_str())
            .filter_map(|s| NotificationChannel::from_str(s).ok())
            .collect(),
        Err(_) => Vec::new(),
    };

    let deduped = doc.get_bool("deduped").unwrap_or(false);
    let router_version = doc
        .get_str("routerVersion")
        .unwrap_or(ROUTER_SCHEMA_VERSION)
        .to_owned();

    Ok(NotificationLogItem {
        id,
        notify_id,
        owner_id,
        dedupe_key,
        kind,
        priority,
        preset,
        name,
        symbol,
        title,
        body,
        at,
        sent_at,
        attempts,
        delivered,
        deduped,
        router_version,
    })
}

#[async_trait]
impl NotificationLogRepository for MongoNotificationLogRepository {
    async fn record(
        &self,
        item: NotificationLogItem,
    ) -> Result<RecordNotificationLogOutcome, RepositoryError> {
        let doc = log_to_doc(&item);
        let update = doc! { "$setOnInsert": doc };

        match self
            .collection
            .update_one(doc! { "_id": &item.id }, update)
            .upsert(true)
            .await
        {
            Ok(result) => {
                if result.upserted_id.is_some() {
                    Ok(RecordNotificationLogOutcome::Recorded(item))
                } else {
                    let existing_doc = self
                        .collection
                        .find_one(doc! { "_id": &item.id })
                        .await?
                        .ok_or(RepositoryError::NotificationLogNotFound)?;
                    Ok(RecordNotificationLogOutcome::Existing(doc_to_log(
                        &existing_doc,
                    )?))
                }
            }
            Err(e) if is_duplicate_key(&e) => {
                let existing_doc = self
                    .collection
                    .find_one(doc! { "_id": &item.id })
                    .await?
                    .ok_or(RepositoryError::NotificationLogNotFound)?;
                Ok(RecordNotificationLogOutcome::Existing(doc_to_log(
                    &existing_doc,
                )?))
            }
            Err(e) => Err(RepositoryError::Mongo(e)),
        }
    }

    async fn get(&self, id: &str) -> Result<Option<NotificationLogItem>, RepositoryError> {
        match self.collection.find_one(doc! { "_id": id }).await? {
            Some(doc) => Ok(Some(doc_to_log(&doc)?)),
            None => Ok(None),
        }
    }

    async fn list(
        &self,
        query: &NotificationLogQuery,
    ) -> Result<Vec<NotificationLogItem>, RepositoryError> {
        let mut filter = Document::new();
        if let Some(owner) = &query.owner_id {
            filter.insert("ownerId", owner);
        }
        if let Some(kind) = &query.kind {
            filter.insert("kind", kind);
        }
        if let Some(priority) = query.priority {
            filter.insert("priority", priority.as_str());
        }

        let options = FindOptions::builder()
            .sort(doc! { "sentAt": -1 })
            .limit(query.limit as i64)
            .build();

        let mut cursor = self.collection.find(filter).with_options(options).await?;
        let mut logs = Vec::new();
        while let Some(doc) = cursor.try_next().await? {
            logs.push(doc_to_log(&doc)?);
        }
        Ok(logs)
    }
}
