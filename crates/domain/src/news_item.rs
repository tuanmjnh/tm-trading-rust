use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};
use thiserror::Error;
use url::Url;

/// Schema stamp written to every `news_items` document by this producer.
pub const NEWS_ITEM_SCHEMA_VERSION: &str = "1";
/// Prefix of the deterministic news item identifier.
pub const NEWS_ITEM_ID_PREFIX: &str = "news_";
/// Rights-safe excerpt ceiling, matching the legacy license matrix.
pub const NEWS_EXCERPT_MAX_CHARS: usize = 240;
/// Market group used when neither the item nor its source declares one.
pub const DEFAULT_NEWS_ITEM_MARKET_GROUP: &str = "crypto";
/// Language used when an item or its source declares none.
pub const DEFAULT_NEWS_ITEM_LANGUAGE: &str = "en";

/// Editorial significance of one story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsLevel {
    High,
    Med,
    Low,
}

impl NewsLevel {
    /// The stored label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Med => "med",
            Self::Low => "low",
        }
    }

    /// Reads a stored label.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Self::High),
            "med" => Some(Self::Med),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

/// Direction the story is read as having for the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsDirection {
    Pos,
    Neg,
    Flat,
}

impl NewsDirection {
    /// The stored label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pos => "pos",
            Self::Neg => "neg",
            Self::Flat => "flat",
        }
    }

    /// Reads a stored label.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pos" => Some(Self::Pos),
            "neg" => Some(Self::Neg),
            "flat" => Some(Self::Flat),
            _ => None,
        }
    }
}

/// Rights snapshot copied from the source at ingestion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsItemRights {
    pub allowed_display: bool,
    pub allowed_summarize: bool,
}

impl Default for NewsItemRights {
    fn default() -> Self {
        Self {
            allowed_display: true,
            allowed_summarize: false,
        }
    }
}

/// Fields a collector supplies for one normalized story.
#[derive(Debug, Clone)]
pub struct NewNewsItem {
    pub source_id: String,
    /// Article URL as seen in the feed; the canonical form is derived from it.
    pub url: String,
    /// Citation link kept exactly as the source published it.
    pub origin_url: String,
    pub title: String,
    pub excerpt: String,
    pub language: String,
    /// Provider publication time. It may be absent and is never replaced by
    /// the ingestion time.
    pub published_at: Option<DateTime<Utc>>,
    /// When the item was first observed.
    pub first_seen_at: DateTime<Utc>,
    /// When the item was written to storage. It stays frozen on re-ingest.
    pub ingested_at: DateTime<Utc>,
    /// Explicit market group. When absent it is derived from the tags and the
    /// groups the source declares.
    pub market_group: Option<String>,
    /// Market groups the source declares, used for the derived group.
    pub source_market_groups: Vec<String>,
    pub event_type: Option<String>,
    pub tags: Vec<String>,
    pub level: NewsLevel,
    pub score: f64,
    pub direction: NewsDirection,
    pub rights: NewsItemRights,
}

/// Complete envelope loaded from durable storage.
#[derive(Debug, Clone)]
pub struct StoredNewsItem {
    pub item_id: String,
    pub source_id: String,
    pub url: String,
    pub origin_url: String,
    pub title: String,
    pub excerpt: String,
    pub language: String,
    pub published_at: Option<DateTime<Utc>>,
    pub first_seen_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub market_group: String,
    pub event_type: Option<String>,
    pub tags: Vec<String>,
    pub level: NewsLevel,
    pub score: f64,
    pub direction: NewsDirection,
    pub rights: NewsItemRights,
    pub hash: Option<String>,
    pub content_hash: Option<String>,
    pub schema_version: String,
    pub updated_at: Option<DateTime<Utc>>,
}

/// One rights-safe normalized news story.
///
/// Only the title and a bounded excerpt are stored; full article text never
/// enters this record. `url` is the canonical identity while `origin_url` is
/// the citation link, and `published_at`, `first_seen_at`, and `ingested_at`
/// stay distinct.
#[derive(Debug, Clone, PartialEq)]
pub struct NewsItem {
    item_id: String,
    source_id: String,
    url: String,
    origin_url: String,
    title: String,
    excerpt: String,
    language: String,
    published_at: Option<DateTime<Utc>>,
    first_seen_at: DateTime<Utc>,
    ingested_at: DateTime<Utc>,
    market_group: String,
    event_type: Option<String>,
    tags: Vec<String>,
    level: NewsLevel,
    score: f64,
    direction: NewsDirection,
    rights: NewsItemRights,
    hash: String,
    content_hash: String,
    schema_version: String,
    updated_at: Option<DateTime<Utc>>,
}

impl NewsItem {
    /// Builds and validates one normalized story.
    pub fn new(input: NewNewsItem) -> Result<Self, NewsItemError> {
        let source_id = required("sourceId", input.source_id)?;
        let canonical = canonicalize_news_url(&input.url).ok_or(NewsItemError::InvalidUrl)?;
        let item_id = news_item_id_for(&canonical).ok_or(NewsItemError::InvalidUrl)?;
        let origin_url = normalize_origin_url(&input.origin_url)?;
        let title = required("title", input.title)?;
        let excerpt = truncate_excerpt(&input.excerpt);
        let language = defaulted(input.language, DEFAULT_NEWS_ITEM_LANGUAGE);
        let tags = normalize_tags(input.tags);
        let event_type = input
            .event_type
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let market_group = match input.market_group {
            Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
            _ => market_group_for(&tags, &input.source_market_groups),
        };
        if !input.score.is_finite() {
            return Err(NewsItemError::InvalidScore);
        }
        let hash = sha1_hex(&format!("{title}|{canonical}"));
        let content_hash = news_content_hash_for(
            &title,
            &excerpt,
            input.level,
            input.direction,
            &tags,
            event_type.as_deref(),
        );
        Ok(Self {
            item_id,
            source_id,
            url: canonical,
            origin_url,
            title,
            excerpt,
            language,
            published_at: input.published_at,
            first_seen_at: input.first_seen_at,
            ingested_at: input.ingested_at,
            market_group,
            event_type,
            tags,
            level: input.level,
            score: input.score,
            direction: input.direction,
            rights: input.rights,
            hash,
            content_hash,
            schema_version: NEWS_ITEM_SCHEMA_VERSION.to_owned(),
            updated_at: None,
        })
    }

    /// Rebuilds an envelope from durable storage.
    ///
    /// A stored row keeps the identifier, hashes, and schema stamp it was
    /// written with, so a reader never rewrites history. Publication time is
    /// not required and is never replaced with the ingestion time.
    pub fn from_stored(input: StoredNewsItem) -> Result<Self, NewsItemError> {
        let item_id = required("itemId", input.item_id)?;
        let source_id = required("sourceId", input.source_id)?;
        let canonical = canonicalize_news_url(&input.url).ok_or(NewsItemError::InvalidUrl)?;
        let origin_url = normalize_origin_url(&input.origin_url)?;
        let title = required("title", input.title)?;
        let excerpt = truncate_excerpt(&input.excerpt);
        let language = defaulted(input.language, DEFAULT_NEWS_ITEM_LANGUAGE);
        let tags = normalize_tags(input.tags);
        let event_type = input
            .event_type
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let market_group = defaulted(input.market_group, DEFAULT_NEWS_ITEM_MARKET_GROUP);
        if !input.score.is_finite() {
            return Err(NewsItemError::InvalidScore);
        }
        let schema_version = required("schemaVersion", input.schema_version)?;
        Ok(Self {
            item_id,
            source_id,
            url: canonical,
            origin_url,
            title,
            excerpt,
            language,
            published_at: input.published_at,
            first_seen_at: input.first_seen_at,
            ingested_at: input.ingested_at,
            market_group,
            event_type,
            tags,
            level: input.level,
            score: input.score,
            direction: input.direction,
            rights: input.rights,
            hash: input.hash.unwrap_or_default(),
            content_hash: input.content_hash.unwrap_or_default(),
            schema_version,
            updated_at: input.updated_at,
        })
    }

    /// Applies a re-observation of the same story.
    ///
    /// `first_seen_at` and `ingested_at` stay frozen on the stored value. Only
    /// a material content change updates the envelope, and `updated_at` never
    /// moves backwards. An unchanged observation returns the stored row.
    #[must_use]
    pub fn merge_reingest(stored: Self, incoming: Self) -> Self {
        if !stored.content_hash.is_empty() && stored.content_hash == incoming.content_hash {
            return stored;
        }
        let updated_at = Some(match stored.updated_at {
            Some(existing) if existing > incoming.ingested_at => existing,
            _ => incoming.ingested_at,
        });
        Self {
            first_seen_at: stored.first_seen_at,
            ingested_at: stored.ingested_at,
            updated_at,
            ..incoming
        }
    }

    /// Whether this story may be displayed under its rights snapshot.
    #[must_use]
    pub const fn display_allowed(&self) -> bool {
        self.rights.allowed_display
    }

    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    #[must_use]
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn origin_url(&self) -> &str {
        &self.origin_url
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn excerpt(&self) -> &str {
        &self.excerpt
    }

    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    #[must_use]
    pub const fn published_at(&self) -> Option<DateTime<Utc>> {
        self.published_at
    }

    #[must_use]
    pub const fn first_seen_at(&self) -> DateTime<Utc> {
        self.first_seen_at
    }

    #[must_use]
    pub const fn ingested_at(&self) -> DateTime<Utc> {
        self.ingested_at
    }

    #[must_use]
    pub fn market_group(&self) -> &str {
        &self.market_group
    }

    #[must_use]
    pub fn event_type(&self) -> Option<&str> {
        self.event_type.as_deref()
    }

    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    #[must_use]
    pub const fn level(&self) -> NewsLevel {
        self.level
    }

    #[must_use]
    pub const fn score(&self) -> f64 {
        self.score
    }

    #[must_use]
    pub const fn direction(&self) -> NewsDirection {
        self.direction
    }

    #[must_use]
    pub const fn rights(&self) -> NewsItemRights {
        self.rights
    }

    #[must_use]
    pub fn hash(&self) -> &str {
        &self.hash
    }

    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub const fn updated_at(&self) -> Option<DateTime<Utc>> {
        self.updated_at
    }
}

/// Canonical form of an article URL used for identity.
///
/// Tracking parameters, the fragment, the default scheme, and a trailing path
/// slash are normalized away so the same story shared through different links
/// maps to one row. Unparseable or non-HTTP input returns `None`.
#[must_use]
pub fn canonicalize_news_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    if url.scheme() == "http" {
        url.set_scheme("https").ok()?;
    }
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking_key(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let strip_slash = url.path().len() > 1 && url.path().ends_with('/');
    url.set_query(None);
    let mut canonical = url.to_string();
    if strip_slash {
        canonical.pop();
    }
    if !pairs.is_empty() {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in &pairs {
            serializer.append_pair(key, value);
        }
        canonical.push('?');
        canonical.push_str(&serializer.finish());
    }
    Some(canonical)
}

/// Deterministic identifier of one story.
///
/// The identifier is `news_` followed by the SHA-1 of the canonical URL, the
/// same derivation the legacy ingestion uses, so both producers address the
/// same row.
#[must_use]
pub fn news_item_id_for(url: &str) -> Option<String> {
    canonicalize_news_url(url)
        .map(|canonical| format!("{NEWS_ITEM_ID_PREFIX}{}", sha1_hex(&canonical)))
}

/// Chooses the market group of one story.
///
/// A tag mapped to a group the source declares wins; otherwise the source's
/// first declared group is used, and `crypto` is the final fallback.
#[must_use]
pub fn market_group_for(tags: &[String], source_groups: &[String]) -> String {
    let groups: Vec<&str> = source_groups
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    for tag in tags {
        if let Some(group) = tag_market_group(tag)
            && groups
                .iter()
                .any(|declared| declared.eq_ignore_ascii_case(group))
        {
            return group.to_owned();
        }
    }
    groups.first().map_or_else(
        || DEFAULT_NEWS_ITEM_MARKET_GROUP.to_owned(),
        |first| (*first).to_owned(),
    )
}

/// Material-change fingerprint of one story.
///
/// It covers only fields a correction can change; volatile values such as the
/// score or timestamps are excluded, so two observations of the same story
/// produce the same fingerprint.
#[must_use]
pub fn news_content_hash_for(
    title: &str,
    excerpt: &str,
    level: NewsLevel,
    direction: NewsDirection,
    tags: &[String],
    event_type: Option<&str>,
) -> String {
    let mut sorted_tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    sorted_tags.sort_unstable();
    sha1_hex(
        &[
            title.trim(),
            excerpt.trim(),
            level.as_str(),
            direction.as_str(),
            &sorted_tags.join(","),
            event_type.unwrap_or_default(),
        ]
        .join("|"),
    )
}

fn tag_market_group(tag: &str) -> Option<&'static str> {
    match tag.trim().to_ascii_lowercase().as_str() {
        "commodity" => Some("fx_gold_commod"),
        "macro_release" | "central_bank" => Some("macro_cb"),
        "equity_market" => Some("equities_vn"),
        "filing" => Some("equities_global"),
        _ => None,
    }
}

fn is_tracking_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_")
        || key == "fbclid"
        || key == "gclid"
        || key.starts_with("mc_")
        || key.starts_with("igsh")
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

fn truncate_excerpt(value: &str) -> String {
    value.chars().take(NEWS_EXCERPT_MAX_CHARS).collect()
}

fn defaulted(value: String, default: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        default.to_owned()
    } else {
        value.to_owned()
    }
}

fn normalize_tags(values: Vec<String>) -> Vec<String> {
    let mut tags = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() || tags.iter().any(|existing: &String| existing == value) {
            continue;
        }
        tags.push(value.to_owned());
    }
    tags
}

fn normalize_origin_url(value: &str) -> Result<String, NewsItemError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsItemError::EmptyField("originUrl"));
    }
    let url = Url::parse(value).map_err(|_| NewsItemError::InvalidOriginUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NewsItemError::InvalidOriginUrl);
    }
    Ok(value.to_owned())
}

fn required(field: &'static str, value: String) -> Result<String, NewsItemError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsItemError::EmptyField(field));
    }
    Ok(value.to_owned())
}

/// Invalid news item data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NewsItemError {
    #[error("news item field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("news item URL is not a usable HTTP or HTTPS URL")]
    InvalidUrl,
    #[error("news item citation URL is not a usable HTTP or HTTPS URL")]
    InvalidOriginUrl,
    #[error("news item score must be finite")]
    InvalidScore,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    fn input() -> NewNewsItem {
        NewNewsItem {
            source_id: "coindesk".to_owned(),
            url: "https://www.coindesk.com/story?utm_source=x&b=2&a=1#top".to_owned(),
            origin_url: "https://www.coindesk.com/story?utm_source=x&b=2&a=1#top".to_owned(),
            title: "Bitcoin holds above support".to_owned(),
            excerpt: "A short summary.".to_owned(),
            language: "en".to_owned(),
            published_at: None,
            first_seen_at: at(),
            ingested_at: at(),
            market_group: None,
            source_market_groups: vec!["crypto".to_owned()],
            event_type: None,
            tags: vec!["bitcoin".to_owned(), "bitcoin".to_owned()],
            level: NewsLevel::Med,
            score: 0.5,
            direction: NewsDirection::Flat,
            rights: NewsItemRights::default(),
        }
    }

    #[test]
    fn canonicalizes_identity_but_keeps_the_original_link() {
        assert_eq!(
            canonicalize_news_url("http://Example.com/story/?utm_source=x&b=2&a=1#top").as_deref(),
            Some("https://example.com/story?a=1&b=2")
        );
        let item = NewsItem::new(input()).unwrap();
        assert_eq!(item.url(), "https://www.coindesk.com/story?a=1&b=2");
        assert_eq!(
            item.origin_url(),
            "https://www.coindesk.com/story?utm_source=x&b=2&a=1#top"
        );
        assert_eq!(
            item.item_id(),
            format!(
                "news_{}",
                sha1_hex("https://www.coindesk.com/story?a=1&b=2")
            )
        );
    }

    #[test]
    fn the_identifier_is_deterministic_for_the_same_story() {
        let first = NewsItem::new(input()).unwrap();
        let second = NewsItem::new(input()).unwrap();
        assert_eq!(first.item_id(), second.item_id());
        assert!(first.item_id().starts_with(NEWS_ITEM_ID_PREFIX));
        assert_ne!(
            news_item_id_for("https://example.com/a"),
            news_item_id_for("https://example.com/b")
        );
        assert_eq!(news_item_id_for("not a url"), None);
    }

    #[test]
    fn the_excerpt_is_bounded_and_never_splits_a_character() {
        let mut long = input();
        long.excerpt = "é".repeat(NEWS_EXCERPT_MAX_CHARS + 50);
        let item = NewsItem::new(long).unwrap();
        assert_eq!(item.excerpt().chars().count(), NEWS_EXCERPT_MAX_CHARS);
    }

    #[test]
    fn derives_the_market_group_from_declared_groups_only() {
        let tags = vec!["commodity".to_owned()];
        assert_eq!(
            market_group_for(
                &tags,
                &["equities_vn".to_owned(), "fx_gold_commod".to_owned()]
            ),
            "fx_gold_commod"
        );
        assert_eq!(
            market_group_for(&tags, &["equities_vn".to_owned()]),
            "equities_vn"
        );
        assert_eq!(market_group_for(&tags, &[]), DEFAULT_NEWS_ITEM_MARKET_GROUP);
        assert_eq!(market_group_for(&[], &["macro_cb".to_owned()]), "macro_cb");
    }

    #[test]
    fn a_rewritten_headline_changes_the_content_fingerprint() {
        let first = NewsItem::new(input()).unwrap();
        let mut rewritten = input();
        rewritten.title = "Bitcoin breaks support".to_owned();
        let second = NewsItem::new(rewritten).unwrap();
        assert_ne!(first.content_hash(), second.content_hash());
        assert_ne!(first.hash(), second.hash());
    }

    #[test]
    fn re_ingest_freezes_the_observation_times_and_updates_on_change() {
        let stored = NewsItem::new(input()).unwrap();
        let later = at() + chrono::Duration::hours(1);

        let mut unchanged = input();
        unchanged.first_seen_at = later;
        unchanged.ingested_at = later;
        let merged = NewsItem::merge_reingest(stored.clone(), NewsItem::new(unchanged).unwrap());
        assert_eq!(merged, stored);
        assert_eq!(merged.updated_at(), None);

        let mut corrected = input();
        corrected.title = "Bitcoin breaks support".to_owned();
        corrected.ingested_at = later;
        let merged = NewsItem::merge_reingest(stored, NewsItem::new(corrected).unwrap());
        assert_eq!(merged.first_seen_at(), at());
        assert_eq!(merged.ingested_at(), at());
        assert_eq!(merged.updated_at(), Some(later));
        assert_eq!(merged.title(), "Bitcoin breaks support");
    }

    #[test]
    fn a_missing_publication_time_is_not_replaced_by_ingestion_time() {
        let item = NewsItem::new(input()).unwrap();
        assert_eq!(item.published_at(), None);
        let stored = NewsItem::from_stored(StoredNewsItem {
            item_id: item.item_id().to_owned(),
            source_id: item.source_id().to_owned(),
            url: item.url().to_owned(),
            origin_url: item.origin_url().to_owned(),
            title: item.title().to_owned(),
            excerpt: item.excerpt().to_owned(),
            language: item.language().to_owned(),
            published_at: None,
            first_seen_at: item.first_seen_at(),
            ingested_at: item.ingested_at(),
            market_group: item.market_group().to_owned(),
            event_type: None,
            tags: item.tags().to_vec(),
            level: item.level(),
            score: item.score(),
            direction: item.direction(),
            rights: item.rights(),
            hash: None,
            content_hash: None,
            schema_version: NEWS_ITEM_SCHEMA_VERSION.to_owned(),
            updated_at: None,
        })
        .unwrap();
        assert_eq!(stored.published_at(), None);
    }

    #[test]
    fn rejects_unusable_input() {
        let mut blank_title = input();
        blank_title.title = "  ".to_owned();
        assert_eq!(
            NewsItem::new(blank_title),
            Err(NewsItemError::EmptyField("title"))
        );

        let mut bad_url = input();
        bad_url.url = "javascript:alert(1)".to_owned();
        assert_eq!(NewsItem::new(bad_url), Err(NewsItemError::InvalidUrl));

        let mut non_finite = input();
        non_finite.score = f64::NAN;
        assert_eq!(NewsItem::new(non_finite), Err(NewsItemError::InvalidScore));
    }
}
