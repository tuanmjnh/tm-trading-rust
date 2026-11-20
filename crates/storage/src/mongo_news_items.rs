use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{
    DEFAULT_NEWS_ITEM_LANGUAGE, DEFAULT_NEWS_ITEM_MARKET_GROUP, NEWS_ITEM_SCHEMA_VERSION,
    NewsDirection, NewsItem, NewsItemRights, NewsLevel, StoredNewsItem,
};

use crate::bson_support::{from_timestamp, is_duplicate_key, number_as_f64, timestamp};
use crate::{NewsItemQuery, NewsItemRepository, NewsItemUpsertOutcome, RepositoryError};

/// Durable `news_items` store compatible with the legacy collection.
///
/// `_id` is the deterministic `news_<sha1(canonicalUrl)>` string, so a replayed
/// feed hits the same row. The two query indexes the legacy model declares are
/// created at startup; the retention TTL index is left to the legacy collection
/// so this service never deletes rows on its own.
pub struct MongoNewsItemRepository {
    collection: Collection<Document>,
}

impl MongoNewsItemRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("news_items");
        let source_index = IndexModel::builder()
            .keys(doc! { "sourceId": 1, "ingestedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("sourceId_ingestedAt".to_owned())
                    .build(),
            )
            .build();
        let group_index = IndexModel::builder()
            .keys(doc! { "marketGroup": 1, "level": 1, "ingestedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("marketGroup_level_ingestedAt".to_owned())
                    .build(),
            )
            .build();
        collection
            .create_indexes(vec![source_index, group_index])
            .await?;
        Ok(Self { collection })
    }

    async fn merge_existing(
        &self,
        existing: NewsItem,
        incoming: &NewsItem,
        outcome: &mut NewsItemUpsertOutcome,
    ) -> Result<(), RepositoryError> {
        let merged = NewsItem::merge_reingest(existing.clone(), incoming.clone());
        if merged == existing {
            outcome.unchanged += 1;
            return Ok(());
        }
        self.collection
            .update_one(
                doc! { "_id": merged.item_id() },
                doc! { "$set": fields(&merged) },
            )
            .await?;
        outcome.updated += 1;
        Ok(())
    }
}

#[async_trait]
impl NewsItemRepository for MongoNewsItemRepository {
    async fn upsert_seen(
        &self,
        items: &[NewsItem],
    ) -> Result<NewsItemUpsertOutcome, RepositoryError> {
        let mut outcome = NewsItemUpsertOutcome::default();
        for item in items {
            let filter = doc! { "_id": item.item_id() };
            let Some(document) = self.collection.find_one(filter).await? else {
                let mut document = fields(item);
                document.insert("_id", item.item_id());
                match self.collection.insert_one(document).await {
                    Ok(_) => outcome.inserted += 1,
                    Err(error) if is_duplicate_key(&error) => {
                        if let Some(document) = self
                            .collection
                            .find_one(doc! { "_id": item.item_id() })
                            .await?
                        {
                            self.merge_existing(from_document(document)?, item, &mut outcome)
                                .await?;
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
                continue;
            };
            self.merge_existing(from_document(document)?, item, &mut outcome)
                .await?;
        }
        Ok(outcome)
    }

    async fn list(&self, query: &NewsItemQuery) -> Result<Vec<NewsItem>, RepositoryError> {
        let mut filter = Document::new();
        if let Some(source_id) = &query.source_id {
            filter.insert("sourceId", source_id.clone());
        }
        if let Some(market_group) = &query.market_group {
            filter.insert("marketGroup", market_group.clone());
        }
        if let Some(level) = query.level {
            filter.insert("level", level.as_str());
        }
        if let Some(since) = query.since {
            filter.insert("ingestedAt", doc! { "$gte": timestamp(since) });
        }
        let limit = i64::try_from(query.limit).unwrap_or(i64::MAX);
        let mut cursor = self
            .collection
            .find(filter)
            .sort(doc! { "ingestedAt": -1, "_id": -1 })
            .limit(limit)
            .await?;
        let mut items = Vec::new();
        while cursor.advance().await? {
            items.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(items)
    }

    async fn get(&self, item_id: &str) -> Result<Option<NewsItem>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": item_id })
            .await?
            .map(from_document)
            .transpose()
    }
}

fn fields(item: &NewsItem) -> Document {
    doc! {
        "sourceId": item.source_id(),
        "url": item.url(),
        "originUrl": item.origin_url(),
        "title": item.title(),
        "excerpt": item.excerpt(),
        "language": item.language(),
        "publishedAt": optional_timestamp(item.published_at()),
        "firstSeenAt": timestamp(item.first_seen_at()),
        "ingestedAt": timestamp(item.ingested_at()),
        "marketGroup": item.market_group(),
        "eventType": optional_text(item.event_type()),
        "tags": item.tags().to_vec(),
        "level": item.level().as_str(),
        "score": item.score(),
        "dir": item.direction().as_str(),
        "rights": doc! {
            "allowedDisplay": item.rights().allowed_display,
            "allowedSummarize": item.rights().allowed_summarize,
        },
        "hash": item.hash(),
        "contentHash": item.content_hash(),
        "updatedAt": optional_timestamp(item.updated_at()),
        "schemaVersion": item.schema_version(),
    }
}

fn optional_timestamp(value: Option<DateTime<Utc>>) -> Bson {
    value.map_or(Bson::Null, |value| Bson::DateTime(timestamp(value)))
}

fn optional_text(value: Option<&str>) -> Bson {
    value.map_or(Bson::Null, |value| Bson::String(value.to_owned()))
}

fn from_document(document: Document) -> Result<NewsItem, RepositoryError> {
    let item_id = required_text(&document, "_id")?;
    let source_id = required_text(&document, "sourceId")?;
    let url = required_text(&document, "url")?;
    let origin_url = optional_text_field(&document, "originUrl")?.unwrap_or_else(|| url.clone());
    let title = required_text(&document, "title")?;
    let excerpt = optional_text_field(&document, "excerpt")?.unwrap_or_default();
    let language = optional_text_field(&document, "language")?
        .unwrap_or_else(|| DEFAULT_NEWS_ITEM_LANGUAGE.to_owned());
    let published_at = optional_timestamp_field(&document, "publishedAt")?;
    let first_seen_at = required_timestamp(&document, "firstSeenAt")?;
    let ingested_at = required_timestamp(&document, "ingestedAt")?;
    let market_group = optional_text_field(&document, "marketGroup")?
        .unwrap_or_else(|| DEFAULT_NEWS_ITEM_MARKET_GROUP.to_owned());
    let event_type = optional_text_field(&document, "eventType")?;
    let tags = string_array(&document, "tags");
    let level = optional_text_field(&document, "level")?
        .and_then(|value| NewsLevel::parse(&value))
        .unwrap_or(NewsLevel::Med);
    let score = match document.get("score") {
        None | Some(Bson::Null) => 0.0,
        Some(value) => number_as_f64("score", value)?,
    };
    let direction = optional_text_field(&document, "dir")?
        .and_then(|value| NewsDirection::parse(&value))
        .unwrap_or(NewsDirection::Flat);
    let rights = rights_from_document(document.get("rights"));
    let hash = optional_text_field(&document, "hash")?;
    let content_hash = optional_text_field(&document, "contentHash")?;
    let schema_version = match optional_text_field(&document, "schemaVersion")? {
        None => NEWS_ITEM_SCHEMA_VERSION.to_owned(),
        Some(value) if value == NEWS_ITEM_SCHEMA_VERSION => value,
        Some(_) => return Err(RepositoryError::CorruptRecord("schemaVersion")),
    };
    let updated_at = optional_timestamp_field(&document, "updatedAt")?;

    NewsItem::from_stored(StoredNewsItem {
        item_id,
        source_id,
        url,
        origin_url,
        title,
        excerpt,
        language,
        published_at,
        first_seen_at,
        ingested_at,
        market_group,
        event_type,
        tags,
        level,
        score,
        direction,
        rights,
        hash,
        content_hash,
        schema_version,
        updated_at,
    })
    .map_err(Into::into)
}

fn required_text(document: &Document, field: &'static str) -> Result<String, RepositoryError> {
    match document.get(field) {
        Some(Bson::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_text_field(
    document: &Document,
    field: &'static str,
) -> Result<Option<String>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        Some(_) => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn string_array(document: &Document, field: &'static str) -> Vec<String> {
    match document.get(field) {
        Some(Bson::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Bson::String(value) => Some(value.to_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn rights_from_document(value: Option<&Bson>) -> NewsItemRights {
    let Some(Bson::Document(document)) = value else {
        return NewsItemRights::default();
    };
    NewsItemRights {
        allowed_display: document
            .get_bool("allowedDisplay")
            .unwrap_or(NewsItemRights::default().allowed_display),
        allowed_summarize: document
            .get_bool("allowedSummarize")
            .unwrap_or(NewsItemRights::default().allowed_summarize),
    }
}

fn required_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    match document.get(field) {
        Some(Bson::DateTime(value)) => from_timestamp(*value),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_timestamp_field(
    document: &Document,
    field: &'static str,
) -> Result<Option<DateTime<Utc>>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::DateTime(value)) => Ok(Some(from_timestamp(*value)?)),
        Some(_) => Err(RepositoryError::CorruptRecord(field)),
    }
}
