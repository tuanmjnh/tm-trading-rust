use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc, oid::ObjectId};
use mongodb::options::{Collation, CollationStrength, IndexOptions};
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::Watchlist;

use crate::bson_support::{from_timestamp, is_duplicate_key, timestamp};
use crate::watchlists_memory::sort_lists;
use crate::{RepositoryError, WatchlistRepository};

/// Durable `watchlists` store matching the legacy document shape.
///
/// `_id` stays a MongoDB `ObjectId` so rows written here remain readable by the legacy
/// application, and the owner-scoped unique name index keeps the legacy case-insensitive
/// uniqueness rule.
pub struct MongoWatchlistRepository {
    collection: Collection<Document>,
}

impl MongoWatchlistRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("watchlists");
        let name_index = IndexModel::builder()
            .keys(doc! { "ownerId": 1, "name": 1 })
            .options(
                IndexOptions::builder()
                    .name("ownerId_name".to_owned())
                    .unique(true)
                    .collation(
                        Collation::builder()
                            .locale("en")
                            .strength(CollationStrength::Primary)
                            .build(),
                    )
                    .build(),
            )
            .build();
        let default_index = IndexModel::builder()
            .keys(doc! { "ownerId": 1, "isDefault": 1, "updatedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("ownerId_isDefault_updatedAt".to_owned())
                    .build(),
            )
            .build();
        collection
            .create_indexes(vec![name_index, default_index])
            .await?;
        Ok(Self { collection })
    }

    async fn has_any(&self, owner_id: &str) -> Result<bool, RepositoryError> {
        Ok(self
            .collection
            .find_one(doc! { "ownerId": owner_id })
            .await?
            .is_some())
    }
}

#[async_trait]
impl WatchlistRepository for MongoWatchlistRepository {
    async fn list(&self, owner_id: &str) -> Result<Vec<Watchlist>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(doc! { "ownerId": owner_id })
            .sort(doc! { "isDefault": -1, "updatedAt": -1 })
            .await?;
        let mut lists = Vec::new();
        while cursor.advance().await? {
            lists.push(from_document(cursor.deserialize_current()?)?);
        }
        sort_lists(&mut lists);
        Ok(lists)
    }

    async fn create(
        &self,
        owner_id: &str,
        name: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError> {
        let name = tm_trading_domain::normalize_name(name)?;
        let is_default = !self.has_any(owner_id).await?;
        let watchlist = Watchlist::new(
            ObjectId::new().to_hex(),
            owner_id.to_owned(),
            name,
            symbols.to_vec(),
            is_default,
            now,
            now,
        )?;
        let insert = self.collection.insert_one(to_document(&watchlist)?).await;
        if let Err(error) = insert {
            if is_duplicate_key(&error) {
                return Err(RepositoryError::DuplicateWatchlistName);
            }
            return Err(error.into());
        }
        Ok(watchlist)
    }

    async fn replace_symbols(
        &self,
        owner_id: &str,
        watchlist_id: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError> {
        let id =
            ObjectId::parse_str(watchlist_id).map_err(|_| RepositoryError::WatchlistNotFound)?;
        let existing = self
            .collection
            .find_one(doc! { "_id": id, "ownerId": owner_id })
            .await?
            .ok_or(RepositoryError::WatchlistNotFound)?;
        let updated = from_document(existing)?.with_symbols(symbols.to_vec(), now)?;
        let result = self
            .collection
            .update_one(
                doc! { "_id": id, "ownerId": owner_id },
                doc! {
                    "$set": {
                        "symbols": updated.symbols(),
                        "updatedAt": timestamp(updated.updated_at()),
                    }
                },
            )
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::WatchlistNotFound);
        }
        Ok(updated)
    }

    async fn delete(&self, owner_id: &str, watchlist_id: &str) -> Result<(), RepositoryError> {
        let id =
            ObjectId::parse_str(watchlist_id).map_err(|_| RepositoryError::WatchlistNotFound)?;
        let result = self
            .collection
            .delete_one(doc! { "_id": id, "ownerId": owner_id })
            .await?;
        if result.deleted_count == 0 {
            return Err(RepositoryError::WatchlistNotFound);
        }
        Ok(())
    }
}

fn to_document(watchlist: &Watchlist) -> Result<Document, RepositoryError> {
    let id =
        ObjectId::parse_str(watchlist.id()).map_err(|_| RepositoryError::CorruptRecord("_id"))?;
    Ok(doc! {
        "_id": id,
        "ownerId": watchlist.owner_id(),
        "name": watchlist.name(),
        "symbols": watchlist.symbols(),
        "isDefault": watchlist.is_default(),
        "createdAt": timestamp(watchlist.created_at()),
        "updatedAt": timestamp(watchlist.updated_at()),
    })
}

fn from_document(document: Document) -> Result<Watchlist, RepositoryError> {
    let id = match document.get("_id") {
        Some(Bson::ObjectId(id)) => id.to_hex(),
        _ => return Err(RepositoryError::CorruptRecord("_id")),
    };
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let symbols = document
        .get_array("symbols")
        .map_err(|_| RepositoryError::CorruptRecord("symbols"))?
        .iter()
        .map(|value| match value {
            Bson::String(symbol) => Ok(symbol.clone()),
            _ => Err(RepositoryError::CorruptRecord("symbols")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let timestamp_of = |field: &'static str| -> Result<DateTime<Utc>, RepositoryError> {
        document
            .get_datetime(field)
            .map_err(|_| RepositoryError::CorruptRecord(field))
            .and_then(|value| from_timestamp(*value))
    };
    Watchlist::new(
        id,
        text("ownerId")?,
        text("name")?,
        symbols,
        document.get_bool("isDefault").unwrap_or(false),
        timestamp_of("createdAt")?,
        timestamp_of("updatedAt")?,
    )
    .map_err(RepositoryError::from)
}
