//! Public RSS/Atom news adapters.
//!
//! The adapters are read-only and bounded: they fetch one public feed URL with
//! a timeout and a byte ceiling, accept only a document that actually looks
//! like RSS, Atom, or RDF, and extract the rights-safe fields the product
//! stores: title, link, publication time, a short excerpt, and category tags.
//! Full article text is never fetched or retained.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;
use tm_trading_domain::is_safe_news_feed_url;
use url::Url;

/// Largest feed document accepted from one source.
pub const MAX_FEED_BYTES: usize = 2_000_000;
/// Largest number of items accepted from one feed document.
pub const MAX_FEED_ITEMS: usize = 100;
/// Largest excerpt retained from one item.
pub const MAX_FEED_EXCERPT_CHARS: usize = 240;

/// One story as published by a feed, before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFeedItem {
    pub title: String,
    pub link: String,
    /// Provider publication time; absent when the feed has no usable date.
    pub published_at: Option<DateTime<Utc>>,
    /// Plain-text excerpt with markup removed and length bounded.
    pub excerpt: String,
    pub tags: Vec<String>,
}

/// Whether a document looks like an RSS, Atom, or RDF feed.
#[must_use]
pub fn looks_like_feed(text: &str) -> bool {
    let head = text.get(..text.len().min(2_000)).unwrap_or(text);
    let head = head.to_ascii_lowercase();
    ["<rss", "<feed", "<rdf"]
        .iter()
        .any(|tag| head.contains(tag))
}

/// Parses one RSS 2.0, Atom, or RDF feed document.
///
/// Items without an absolute HTTP(S) link or without a title are dropped
/// instead of producing a half-usable record.
pub fn parse_feed(text: &str) -> Result<Vec<RawFeedItem>, NewsFetchError> {
    if !looks_like_feed(text) {
        return Err(NewsFetchError::InvalidFeed);
    }
    let mut items = Vec::new();
    for block in elements(text, "item")
        .into_iter()
        .chain(elements(text, "entry"))
    {
        let Some(item) = parse_item(&block) else {
            continue;
        };
        items.push(item);
        if items.len() == MAX_FEED_ITEMS {
            break;
        }
    }
    if items.is_empty() {
        return Err(NewsFetchError::InvalidFeed);
    }
    Ok(items)
}

fn parse_item(block: &Element) -> Option<RawFeedItem> {
    let title = plain_text(&inner_of(block, &["title"])?);
    if title.is_empty() {
        return None;
    }
    let link = item_link(block)?;
    let published_at = inner_of(block, &["pubdate", "published", "dc:date", "updated"])
        .and_then(|value| parse_published_at(&plain_text(&value)));
    let excerpt = inner_of(block, &["description", "summary"])
        .map(|value| plain_text(&value))
        .map(|value| truncate(&value, MAX_FEED_EXCERPT_CHARS))
        .unwrap_or_default();
    let mut tags = Vec::new();
    for element in elements(&block.inner, "category") {
        if let Some(term) = attribute(&element.attrs, "term") {
            push_tag(&mut tags, &term);
            continue;
        }
        push_tag(&mut tags, &plain_text(&element.inner));
    }
    Some(RawFeedItem {
        title,
        link,
        published_at,
        excerpt,
        tags,
    })
}

fn item_link(block: &Element) -> Option<String> {
    let mut fallback = None;
    for element in elements(&block.inner, "link") {
        let text = plain_text(&element.inner);
        let Some(href) = attribute(&element.attrs, "href").or_else(|| {
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }) else {
            continue;
        };
        let href = href.trim();
        let rel = attribute(&element.attrs, "rel").unwrap_or_default();
        if rel.is_empty() || rel.eq_ignore_ascii_case("alternate") {
            if let Some(url) = absolute_http_url(href) {
                return Some(url);
            }
        } else if fallback.is_none() {
            fallback = absolute_http_url(href);
        }
    }
    fallback
}

fn absolute_http_url(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some(value.to_owned())
}

fn push_tag(tags: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !tags.iter().any(|existing| existing == value) {
        tags.push(value.to_owned());
    }
}

/// Parses an RFC 3339 or RFC 2822 publication time.
#[must_use]
pub fn parse_published_at(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_rfc2822(value))
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

struct Element {
    attrs: String,
    inner: String,
}

fn elements(content: &str, tag: &str) -> Vec<Element> {
    let lower = content.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(&open) {
        let start = cursor + relative;
        let after = start + open.len();
        if !lower[after..].chars().next().is_some_and(|character| {
            character == '>' || character == '/' || character.is_ascii_whitespace()
        }) {
            cursor = after;
            continue;
        }
        let Some(open_end_relative) = lower[after..].find('>') else {
            break;
        };
        let open_end = after + open_end_relative;
        let attrs = content[after..open_end].to_owned();
        if attrs.trim_end().ends_with('/') {
            found.push(Element {
                attrs,
                inner: String::new(),
            });
            cursor = open_end + 1;
            continue;
        }
        let Some(close_relative) = lower[open_end..].find(&close) else {
            break;
        };
        let close_start = open_end + close_relative;
        found.push(Element {
            attrs,
            inner: content[open_end + 1..close_start].to_owned(),
        });
        cursor = close_start + close.len();
    }
    found
}

fn inner_of(block: &Element, tags: &[&str]) -> Option<String> {
    for tag in tags {
        if let Some(element) = elements(&block.inner, tag).into_iter().next() {
            let inner = element.inner.trim();
            if !inner.is_empty() {
                return Some(inner.to_owned());
            }
        }
    }
    None
}

fn attribute(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let pattern = format!("{name}=");
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(&pattern) {
        let index = cursor + relative;
        let boundary = index == 0
            || attrs[..index]
                .chars()
                .last()
                .is_some_and(|character| character.is_ascii_whitespace() || character == '/');
        if !boundary {
            cursor = index + pattern.len();
            continue;
        }
        let rest = attrs[index + pattern.len()..].trim_start();
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            let rest = &rest[1..];
            let end = rest.find(quote)?;
            return Some(decode_entities(&rest[..end]));
        }
        let end = rest
            .find(|character: char| character.is_ascii_whitespace() || character == '/')
            .unwrap_or(rest.len());
        return Some(decode_entities(&rest[..end]));
    }
    None
}

/// Removes markup and CDATA wrappers from a feed value and collapses whitespace.
#[must_use]
pub fn plain_text(value: &str) -> String {
    let unwrapped = unwrap_cdata(value);
    let mut text = String::with_capacity(unwrapped.len());
    let mut in_tag = false;
    for character in unwrapped.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let decoded = decode_entities(&text);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unwrap_cdata(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("<![CDATA[") {
        text.push_str(&rest[..start]);
        let after = &rest[start + "<![CDATA[".len()..];
        let Some(end) = after.find("]]>") else {
            text.push_str(after);
            return text;
        };
        text.push_str(&after[..end]);
        rest = &after[end + "]]>".len()..];
    }
    text.push_str(rest);
    text
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn decode_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('&') {
        decoded.push_str(&rest[..index]);
        let candidate = &rest[index..];
        let Some(end) = candidate.find(';') else {
            decoded.push_str(candidate);
            return decoded;
        };
        let entity = &candidate[1..end];
        let replacement = entity_replacement(entity);
        match replacement {
            Some(character) => {
                decoded.push(character);
                rest = &candidate[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &candidate[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn entity_replacement(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "ndash" | "mdash" => Some('-'),
        "hellip" => Some('.'),
        _ => {
            let number = entity.strip_prefix('#')?;
            let value = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(value)
        }
    }
}

/// One failed feed fetch or parse, classified for source health.
#[derive(Debug, Error)]
pub enum NewsFetchError {
    #[error("feed URL is not a safe public HTTP(S) address")]
    InvalidUrl,
    #[error("failed to build the feed HTTP client: {0}")]
    Client(String),
    #[error("feed request failed: {0}")]
    Request(String),
    #[error("feed request timed out")]
    Timeout,
    #[error("feed request failed with HTTP {status}")]
    Status { status: u16 },
    #[error("feed document exceeds the {MAX_FEED_BYTES} byte limit")]
    TooLarge,
    #[error("response is not a usable RSS, Atom, or RDF feed")]
    InvalidFeed,
}

impl NewsFetchError {
    /// Stable, sanitized class recorded with source health.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl | Self::InvalidFeed | Self::TooLarge => "invalid_feed",
            Self::Client(_) => "unavailable",
            Self::Request(_) => "connection_error",
            Self::Timeout => "timeout",
            Self::Status { status } => match status {
                401 | 403 | 451 => "permission_denied",
                429 => "rate_limited",
                _ => "http_error",
            },
        }
    }
}

/// Reads one public feed without following redirects or exceeding its bounds.
#[async_trait]
pub trait NewsFeedFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<Vec<RawFeedItem>, NewsFetchError>;
}

/// Bounded HTTP feed fetcher.
pub struct HttpFeedFetcher {
    client: reqwest::Client,
}

impl HttpFeedFetcher {
    /// Builds the fetcher with one overall timeout.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be built.
    pub fn new(timeout: Duration) -> Result<Self, NewsFetchError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                "tm-trading-news-providers/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|error| NewsFetchError::Client(error.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl NewsFeedFetcher for HttpFeedFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<RawFeedItem>, NewsFetchError> {
        if !is_safe_news_feed_url(url) {
            return Err(NewsFetchError::InvalidUrl);
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(classify_request_error)?;
        if !response.status().is_success() {
            return Err(NewsFetchError::Status {
                status: response.status().as_u16(),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEED_BYTES as u64)
        {
            return Err(NewsFetchError::TooLarge);
        }
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(classify_request_error)? {
            if body.len() + chunk.len() > MAX_FEED_BYTES {
                return Err(NewsFetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&body);
        parse_feed(&text)
    }
}

fn classify_request_error(error: reqwest::Error) -> NewsFetchError {
    if error.is_timeout() {
        return NewsFetchError::Timeout;
    }
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("certificate") || lower.contains("tls") {
        return NewsFetchError::Request("tls_error".to_owned());
    }
    NewsFetchError::Request(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>Example</title>
<item>
  <title><![CDATA[Bitcoin &amp; Ether rally]]></title>
  <link>https://example.com/a?utm_source=rss</link>
  <description><![CDATA[<p>Traders &amp; funds add exposure.</p>]]></description>
  <pubDate>Fri, 20 Nov 2026 08:00:00 GMT</pubDate>
  <category>bitcoin</category>
  <category>macro</category>
</item>
<item>
  <title>No link here</title>
  <description>Dropped.</description>
</item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Central bank</title>
  <entry>
    <title>Rate decision</title>
    <link rel="alternate" href="https://example.org/decision"/>
    <link rel="self" href="https://example.org/feed"/>
    <published>2026-11-20T08:30:00Z</published>
    <summary>Policy rate unchanged.</summary>
    <category term="central_bank"/>
  </entry>
</feed>"#;

    const RDF: &str = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<item rdf:about="https://example.net/filing">
  <title>8-K filing</title>
  <link>https://example.net/filing</link>
  <dc:date>2026-11-19T21:00:00Z</dc:date>
</item>
</rdf:RDF>"#;

    #[test]
    fn parses_rss_items_with_entities_and_markup() {
        let items = parse_feed(RSS).expect("parse RSS");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bitcoin & Ether rally");
        assert_eq!(items[0].link, "https://example.com/a?utm_source=rss");
        assert_eq!(items[0].excerpt, "Traders & funds add exposure.");
        assert_eq!(items[0].tags, ["bitcoin", "macro"]);
        assert_eq!(
            items[0].published_at,
            parse_published_at("2026-11-20T08:00:00Z")
        );
    }

    #[test]
    fn parses_atom_entries_and_prefers_the_alternate_link() {
        let items = parse_feed(ATOM).expect("parse Atom");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Rate decision");
        assert_eq!(items[0].link, "https://example.org/decision");
        assert_eq!(items[0].excerpt, "Policy rate unchanged.");
        assert_eq!(items[0].tags, ["central_bank"]);
        assert_eq!(
            items[0].published_at,
            parse_published_at("2026-11-20T08:30:00Z")
        );
    }

    #[test]
    fn parses_rdf_items_with_a_dc_date() {
        let items = parse_feed(RDF).expect("parse RDF");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link, "https://example.net/filing");
        assert!(items[0].published_at.is_some());
    }

    #[test]
    fn rejects_a_document_that_is_not_a_feed() {
        assert!(!looks_like_feed("{\"items\":[]}"));
        assert!(matches!(
            parse_feed("<html><body>Not a feed</body></html>"),
            Err(NewsFetchError::InvalidFeed)
        ));
    }

    #[test]
    fn rejects_relative_links_and_keeps_the_excerpt_bounded() {
        let feed = format!(
            "<rss><channel><item><title>Relative</title><link>/story</link></item>
             <item><title>Long</title><link>https://example.com/long</link>
             <description>{}</description></item></channel></rss>",
            "word ".repeat(200)
        );
        let items = parse_feed(&feed).expect("parse feed");
        assert_eq!(items.len(), 1);
        assert!(items[0].excerpt.chars().count() <= MAX_FEED_EXCERPT_CHARS);
    }

    #[test]
    fn parses_the_common_publication_formats() {
        assert!(parse_published_at("2026-11-20T08:00:00+07:00").is_some());
        assert!(parse_published_at("Fri, 20 Nov 2026 08:00:00 GMT").is_some());
        assert_eq!(parse_published_at("not a date"), None);
        assert_eq!(parse_published_at(""), None);
    }

    #[test]
    fn health_codes_stay_within_the_legacy_diagnostic_set() {
        for error in [
            NewsFetchError::InvalidUrl,
            NewsFetchError::Client("x".to_owned()),
            NewsFetchError::Request("x".to_owned()),
            NewsFetchError::Timeout,
            NewsFetchError::Status { status: 429 },
            NewsFetchError::Status { status: 403 },
            NewsFetchError::Status { status: 500 },
            NewsFetchError::TooLarge,
            NewsFetchError::InvalidFeed,
        ] {
            assert_eq!(
                tm_trading_domain::sanitize_news_fetch_error(error.code()),
                error.code()
            );
        }
    }
}
