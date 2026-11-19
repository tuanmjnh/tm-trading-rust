use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use tm_trading_domain::Watchlist;

/// A named symbol list owned by the authenticated subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WatchlistDto {
    pub id: String,
    pub name: String,
    pub symbols: Vec<String>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Watchlist> for WatchlistDto {
    fn from(value: Watchlist) -> Self {
        Self {
            id: value.id().to_owned(),
            name: value.name().to_owned(),
            symbols: value.symbols().to_vec(),
            is_default: value.is_default(),
            created_at: value.created_at(),
            updated_at: value.updated_at(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateWatchlistRequest {
    pub name: String,
    #[serde(default)]
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReplaceWatchlistSymbolsRequest {
    pub symbols: Vec<String>,
}
