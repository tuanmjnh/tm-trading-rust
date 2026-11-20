use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{NEWS_CLUSTER_SCHEMA_VERSION, NewsCluster};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{NewsClusterQuery, NewsClusterRepository, RepositoryError};

pub struct MongoNewsClusterRepository {
    collection: Collection<Document>,
}

impl MongoNewsClusterRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("news_clusters");
        let index = IndexModel::builder()
            .keys(doc! { "marketGroup": 1, "lastSeenAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("marketGroup_lastSeenAt".to_owned())
                    .build(),
            )
            .build();
        collection.create_indexes(vec![index]).await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl NewsClusterRepository for MongoNewsClusterRepository {
    async fn upsert(&self, cluster: &NewsCluster) -> Result<(), RepositoryError> {
        let filter = doc! { "_id": cluster.cluster_id() };
        let mut document = fields(cluster);
        document.insert("_id", cluster.cluster_id());
        self.collection
            .replace_one(filter, document)
            .upsert(true)
            .await?;
        Ok(())
    }

    async fn list(&self, query: &NewsClusterQuery) -> Result<Vec<NewsCluster>, RepositoryError> {
        let mut filter = Document::new();
        if let Some(market_group) = &query.market_group {
            filter.insert("marketGroup", market_group.clone());
        }
        if let Some(since) = query.since {
            filter.insert("lastSeenAt", doc! { "$gte": timestamp(since) });
        }
        let limit = i64::try_from(query.limit).unwrap_or(i64::MAX);
        let mut cursor = self
            .collection
            .find(filter)
            .sort(doc! { "lastSeenAt": -1, "_id": -1 })
            .limit(limit)
            .await?;
        let mut clusters = Vec::new();
        while cursor.advance().await? {
            clusters.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(clusters)
    }

    async fn get(&self, cluster_id: &str) -> Result<Option<NewsCluster>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": cluster_id })
            .await?
            .map(from_document)
            .transpose()
    }
}

fn fields(cluster: &NewsCluster) -> Document {
    doc! {
        "itemIds": cluster.item_ids().to_vec(),
        "tokens": cluster.tokens().to_vec(),
        "marketGroup": cluster.market_group(),
        "firstSeenAt": timestamp(cluster.first_seen_at()),
        "lastSeenAt": timestamp(cluster.last_seen_at()),
        "schemaVersion": cluster.schema_version(),
    }
}

fn from_document(document: Document) -> Result<NewsCluster, RepositoryError> {
    use tm_trading_domain::StoredNewsCluster;
    let cluster_id = required_text(&document, "_id")?;
    let item_ids = string_array(&document, "itemIds");
    let tokens = string_array(&document, "tokens");
    let market_group = optional_text_field(&document, "marketGroup")?
        .unwrap_or_else(|| DEFAULT_FALLBACK.to_owned());
    let first_seen_at = required_timestamp(&document, "firstSeenAt")?;
    let last_seen_at = required_timestamp(&document, "lastSeenAt")?;
    let schema_version = match optional_text_field(&document, "schemaVersion")? {
        None => NEWS_CLUSTER_SCHEMA_VERSION.to_owned(),
        Some(value) if value == NEWS_CLUSTER_SCHEMA_VERSION => value,
        Some(_) => return Err(RepositoryError::CorruptRecord("schemaVersion")),
    };
    NewsCluster::from_stored(StoredNewsCluster {
        cluster_id,
        item_ids,
        tokens,
        market_group,
        first_seen_at,
        last_seen_at,
        schema_version,
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

fn required_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    match document.get(field) {
        Some(Bson::DateTime(value)) => from_timestamp(*value),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

const DEFAULT_FALLBACK: &str = "crypto";
