use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};
use thiserror::Error;

/// Schema stamp written to every `news_clusters` document by this producer.
pub const NEWS_CLUSTER_SCHEMA_VERSION: &str = "1";
/// Prefix of the deterministic news cluster identifier.
pub const NEWS_CLUSTER_ID_PREFIX: &str = "newscl_";
/// Jaccard similarity threshold above which two stories join one cluster.
pub const NEWS_CLUSTER_JACCARD_THRESHOLD: f64 = 0.5;
/// Upper bound on the stories kept in one cluster.
pub const MAX_NEWS_CLUSTER_ITEMS: usize = 50;
/// Market group used when a cluster declares none.
pub const DEFAULT_NEWS_CLUSTER_MARKET_GROUP: &str = "crypto";

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "of", "in", "on", "for", "to", "with", "as", "at", "by",
    "from", "is", "are", "was", "were", "be", "been", "has", "have", "had", "this", "that",
    "these", "those", "it", "its", "new", "news", "says", "said", "will", "can", "may", "not",
    "into", "over", "after", "before", "about", "than", "more", "most", "also", "just",
];

/// Fields a collector supplies when it opens a cluster around one story.
#[derive(Debug, Clone)]
pub struct NewNewsCluster {
    /// Significant tokens extracted from the story title.
    pub tokens: Vec<String>,
    /// The first story that opened this cluster.
    pub item_id: String,
    /// Market group inherited from the story; `None` falls back to crypto.
    pub market_group: Option<String>,
    /// When the story was first observed.
    pub observed_at: DateTime<Utc>,
}

/// Complete envelope loaded from durable storage.
#[derive(Debug, Clone)]
pub struct StoredNewsCluster {
    pub cluster_id: String,
    pub item_ids: Vec<String>,
    pub tokens: Vec<String>,
    pub market_group: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub schema_version: String,
}

/// One cluster of stories that describe the same event.
#[derive(Debug, Clone, PartialEq)]
pub struct NewsCluster {
    cluster_id: String,
    item_ids: Vec<String>,
    tokens: Vec<String>,
    market_group: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    schema_version: String,
}

impl NewsCluster {
    /// Builds a new cluster around one story.
    pub fn new(input: NewNewsCluster) -> Result<Self, NewsClusterError> {
        let item_id = required("itemId", input.item_id)?;
        let tokens = normalize_tokens(input.tokens);
        let cluster_id = news_cluster_id_for(&tokens);
        let market_group = match input.market_group {
            Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
            _ => DEFAULT_NEWS_CLUSTER_MARKET_GROUP.to_owned(),
        };
        Ok(Self {
            cluster_id,
            item_ids: vec![item_id],
            tokens,
            market_group,
            first_seen_at: input.observed_at,
            last_seen_at: input.observed_at,
            schema_version: NEWS_CLUSTER_SCHEMA_VERSION.to_owned(),
        })
    }

    /// Rebuilds an envelope from durable storage.
    pub fn from_stored(input: StoredNewsCluster) -> Result<Self, NewsClusterError> {
        let cluster_id = required("clusterId", input.cluster_id)?;
        let tokens = normalize_tokens(input.tokens);
        let market_group = if input.market_group.trim().is_empty() {
            DEFAULT_NEWS_CLUSTER_MARKET_GROUP.to_owned()
        } else {
            input.market_group.trim().to_owned()
        };
        let schema_version = required("schemaVersion", input.schema_version)?;
        if schema_version != NEWS_CLUSTER_SCHEMA_VERSION {
            return Err(NewsClusterError::InvalidSchemaVersion);
        }
        let mut item_ids = Vec::with_capacity(input.item_ids.len());
        for value in input.item_ids {
            if !value.trim().is_empty()
                && !item_ids.iter().any(|existing: &String| existing == &value)
            {
                item_ids.push(value);
            }
        }
        if item_ids.len() > MAX_NEWS_CLUSTER_ITEMS {
            item_ids.truncate(MAX_NEWS_CLUSTER_ITEMS);
        }
        Ok(Self {
            cluster_id,
            item_ids,
            tokens,
            market_group,
            first_seen_at: input.first_seen_at,
            last_seen_at: input.last_seen_at,
            schema_version,
        })
    }

    /// Whether another story's tokens are close enough to join this cluster.
    #[must_use]
    pub fn matches_tokens(&self, tokens: &[String]) -> bool {
        news_cluster_jaccard(&self.tokens, tokens) >= NEWS_CLUSTER_JACCARD_THRESHOLD
    }

    /// Adds one story to the cluster, keeping the bound and updating the window.
    pub fn add_item(&mut self, item_id: &str, observed_at: DateTime<Utc>) {
        let item_id = item_id.trim();
        if item_id.is_empty() {
            return;
        }
        if !self.item_ids.iter().any(|existing| existing == item_id)
            && self.item_ids.len() < MAX_NEWS_CLUSTER_ITEMS
        {
            self.item_ids.push(item_id.to_owned());
        }
        if observed_at < self.first_seen_at {
            self.first_seen_at = observed_at;
        }
        if observed_at > self.last_seen_at {
            self.last_seen_at = observed_at;
        }
    }

    #[must_use]
    pub fn cluster_id(&self) -> &str {
        &self.cluster_id
    }

    #[must_use]
    pub fn item_ids(&self) -> &[String] {
        &self.item_ids
    }

    #[must_use]
    pub fn tokens(&self) -> &[String] {
        &self.tokens
    }

    #[must_use]
    pub fn market_group(&self) -> &str {
        &self.market_group
    }

    #[must_use]
    pub const fn first_seen_at(&self) -> DateTime<Utc> {
        self.first_seen_at
    }

    #[must_use]
    pub const fn last_seen_at(&self) -> DateTime<Utc> {
        self.last_seen_at
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }
}

/// Extracts the significant tokens of one headline.
#[must_use]
pub fn significant_tokens(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = text
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .filter(|word| !STOP_WORDS.contains(word))
        .map(str::to_owned)
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

/// Deterministic identifier of one cluster.
#[must_use]
pub fn news_cluster_id_for(tokens: &[String]) -> String {
    let mut sorted: Vec<&str> = tokens.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    format!("{NEWS_CLUSTER_ID_PREFIX}{}", sha1_hex(&sorted.join("|")))
}

/// Jaccard similarity between two token sets.
#[must_use]
pub fn news_cluster_jaccard(left: &[String], right: &[String]) -> f64 {
    let left: HashSet<&str> = left.iter().map(String::as_str).collect();
    let right: HashSet<&str> = right.iter().map(String::as_str).collect();
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let union = left.union(&right).count();
    if union == 0 {
        return 0.0;
    }
    let intersection = left.intersection(&right).count();
    intersection as f64 / union as f64
}

fn normalize_tokens(values: Vec<String>) -> Vec<String> {
    let mut tokens: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn required(field: &'static str, value: String) -> Result<String, NewsClusterError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsClusterError::EmptyField(field));
    }
    Ok(value.to_owned())
}

fn sha1_hex(value: &str) -> String {
    use std::fmt::Write;
    let mut hasher = Sha1::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

/// Invalid news cluster data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NewsClusterError {
    #[error("news cluster field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("news cluster schema version is not supported")]
    InvalidSchemaVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    #[test]
    fn significant_tokens_drop_stop_words_and_short_fragments() {
        let tokens = significant_tokens("Bitcoin holds above key support level");
        assert!(tokens.contains(&"bitcoin".to_owned()));
        assert!(tokens.contains(&"holds".to_owned()));
        assert!(tokens.contains(&"above".to_owned()));
        assert!(tokens.contains(&"support".to_owned()));
        assert!(!tokens.contains(&"the".to_owned()));
    }

    #[test]
    fn cluster_id_is_deterministic_for_the_same_tokens() {
        let tokens = vec!["bitcoin".to_owned(), "support".to_owned()];
        assert_eq!(
            news_cluster_id_for(&tokens),
            news_cluster_id_for(&["support".to_owned(), "bitcoin".to_owned()])
        );
        assert!(news_cluster_id_for(&tokens).starts_with(NEWS_CLUSTER_ID_PREFIX));
    }

    #[test]
    fn jaccard_groups_close_headlines() {
        let left = significant_tokens("Bitcoin holds above support");
        let right = significant_tokens("Bitcoin holds above key support");
        assert!(news_cluster_jaccard(&left, &right) >= NEWS_CLUSTER_JACCARD_THRESHOLD);
    }

    #[test]
    fn a_cluster_adds_stories_and_updates_its_window() {
        let tokens = significant_tokens("Bitcoin holds above support");
        let mut cluster = NewsCluster::new(NewNewsCluster {
            tokens: tokens.clone(),
            item_id: "news_1".to_owned(),
            market_group: Some("crypto".to_owned()),
            observed_at: at(),
        })
        .unwrap();
        cluster.add_item("news_2", at() + chrono::Duration::minutes(5));
        assert_eq!(
            cluster.item_ids(),
            &["news_1".to_owned(), "news_2".to_owned()]
        );
        assert_eq!(cluster.last_seen_at(), at() + chrono::Duration::minutes(5));
        assert!(cluster.matches_tokens(&tokens));
    }

    #[test]
    fn round_trips_through_storage() {
        let tokens = significant_tokens("Bitcoin holds above support");
        let cluster = NewsCluster::new(NewNewsCluster {
            tokens: tokens.clone(),
            item_id: "news_1".to_owned(),
            market_group: None,
            observed_at: at(),
        })
        .unwrap();
        let stored = StoredNewsCluster {
            cluster_id: cluster.cluster_id().to_owned(),
            item_ids: cluster.item_ids().to_vec(),
            tokens: cluster.tokens().to_vec(),
            market_group: cluster.market_group().to_owned(),
            first_seen_at: cluster.first_seen_at(),
            last_seen_at: cluster.last_seen_at(),
            schema_version: cluster.schema_version().to_owned(),
        };
        let rebuilt = NewsCluster::from_stored(stored).unwrap();
        assert_eq!(rebuilt, cluster);
    }
}
