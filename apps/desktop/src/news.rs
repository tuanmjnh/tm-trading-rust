use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{
    NewsClusterDto, NewsClusterListDto, NewsDirectionDto, NewsFreshnessDto, NewsHealthStateDto,
    NewsItemDto, NewsLevelDto, NewsListDto, NewsSourceDto, NewsSourceListDto,
};

use crate::i18n::UiStrings;
use crate::slideover::Slideover;

/// Independent state for the News & Events panel and its detail overlay.
#[derive(Clone, Default, PartialEq)]
pub struct NewsPanelState {
    pub items: Option<Vec<NewsItemDto>>,
    pub list_error: Option<String>,
    pub stale_after_seconds: i64,
    pub sources: Option<Vec<NewsSourceDto>>,
    pub sources_error: Option<String>,
    pub clusters: Option<Vec<NewsClusterDto>>,
    pub clusters_error: Option<String>,
    pub selected_id: Option<String>,
    pub detail: Option<NewsItemDto>,
    pub detail_error: Option<String>,
}

impl NewsPanelState {
    pub fn list_loaded(&mut self, list: NewsListDto) {
        self.items = Some(list.items);
        self.stale_after_seconds = list.stale_after_seconds;
        self.list_error = None;
    }

    pub fn list_failed(&mut self, error: String) {
        self.list_error = Some(error);
    }

    pub fn sources_loaded(&mut self, list: NewsSourceListDto) {
        self.sources = Some(list.sources);
        self.sources_error = None;
    }

    pub fn sources_failed(&mut self, error: String) {
        self.sources_error = Some(error);
    }

    pub fn clusters_loaded(&mut self, list: NewsClusterListDto) {
        self.clusters = Some(list.clusters);
        self.clusters_error = None;
    }

    pub fn clusters_failed(&mut self, error: String) {
        self.clusters_error = Some(error);
    }

    pub fn open(&mut self, news_id: String) {
        self.selected_id = Some(news_id);
        self.detail = None;
        self.detail_error = None;
    }

    pub fn close(&mut self) {
        self.selected_id = None;
        self.detail = None;
        self.detail_error = None;
    }

    pub fn detail_loaded(&mut self, item: NewsItemDto) {
        if self.selected_id.as_deref() == Some(item.id.as_str()) {
            self.detail = Some(item);
            self.detail_error = None;
        }
    }

    pub fn detail_failed(&mut self, news_id: &str, error: String) {
        if self.selected_id.as_deref() == Some(news_id) {
            self.detail = None;
            self.detail_error = Some(error);
        }
    }
}

/// Renders rights-safe normalized news and the health of their sources.
#[component]
pub fn NewsPanel(
    state: NewsPanelState,
    strings: UiStrings,
    on_open: EventHandler<String>,
) -> Element {
    let item_count = state.items.as_ref().map_or(0, Vec::len);
    let fresh_count = state.items.as_ref().map_or(0, |items| {
        items
            .iter()
            .filter(|item| item.freshness == NewsFreshnessDto::Fresh)
            .count()
    });
    let source_count = state.sources.as_ref().map_or(0, Vec::len);

    rsx! {
        section { class: "panel news-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.news_events}" }
                    p { class: "panel-subtitle", "{strings.news_events_hint}" }
                }
                div { class: "signal-summary", aria_label: "{strings.news_summary}",
                    span { strong { "{item_count}" } " {strings.news_items_count}" }
                    span { strong { "{fresh_count}" } " {strings.news_fresh_count}" }
                    span { strong { "{source_count}" } " {strings.news_sources_count}" }
                }
            }
            if let Some(error) = state.list_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.items.as_ref() {
                None if state.list_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_unavailable}" }
                        p { "{strings.news_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.news_loading}" }
                        p { "{strings.news_loading_hint}" }
                    }
                },
                Some(items) if items.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_empty}" }
                        p { "{strings.news_empty_hint}" }
                    }
                },
                Some(items) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services news-table",
                            thead {
                                tr {
                                    th { "{strings.news_title_column}" }
                                    th { "{strings.news_publisher_column}" }
                                    th { "{strings.news_published_column}" }
                                    th { "{strings.news_ingested_column}" }
                                    th { "{strings.news_market_column}" }
                                    th { "{strings.news_tags_column}" }
                                    th { "{strings.news_freshness_column}" }
                                }
                            }
                            tbody {
                                for item in items.iter() {
                                    {
                                        let news_id = item.id.clone();
                                        let open = on_open;
                                        rsx! {
                                            tr { class: "signal-row",
                                                td {
                                                    button {
                                                        class: "signal-open news-title",
                                                        r#type: "button",
                                                        onclick: move |_| open.call(news_id.clone()),
                                                        "{item.title}"
                                                    }
                                                    div { class: "signal-context", "{item.excerpt}" }
                                                }
                                                td {
                                                    div { class: "signal-source", "{item.source_id}" }
                                                    div { class: "signal-source-method", "{item.language}" }
                                                }
                                                td { class: "signal-time", "{format_optional_timestamp(item.published_at)}" }
                                                td { class: "signal-time", "{format_timestamp(item.ingested_at)}" }
                                                td { span { class: "timeframe-chip", "{item.market_group}" } }
                                                td {
                                                    div { class: "chips",
                                                        for tag in item.tags.iter() {
                                                            span { class: "chip", "{tag}" }
                                                        }
                                                    }
                                                }
                                                td {
                                                    span { class: "pill {freshness_class(item.freshness)}",
                                                        "{freshness_label(item.freshness, &strings)}"
                                                    }
                                                    div { class: "signal-age", "{format_age(item.age_seconds)}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "panel-head news-sources-head",
                div {
                    h3 { "{strings.news_sources}" }
                    p { class: "panel-subtitle", "{strings.news_sources_hint}" }
                }
            }
            if let Some(error) = state.sources_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.sources.as_ref() {
                None if state.sources_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_sources_unavailable}" }
                        p { "{strings.news_sources_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.news_sources_loading}" }
                    }
                },
                Some(sources) if sources.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_sources_empty}" }
                        p { "{strings.news_sources_empty_hint}" }
                    }
                },
                Some(sources) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services news-sources-table",
                            thead {
                                tr {
                                    th { "{strings.news_source_column}" }
                                    th { "{strings.news_transport_column}" }
                                    th { "{strings.news_health_column}" }
                                    th { "{strings.news_last_fetch_column}" }
                                    th { "{strings.news_last_item_column}" }
                                    th { "{strings.news_error_column}" }
                                }
                            }
                            tbody {
                                for source in sources.iter() {
                                    tr {
                                        td {
                                            div { class: "service-name", "{source.name}" }
                                            div { class: "service-instance", "{source.id} · {source.trust_tier}" }
                                        }
                                        td { span { class: "timeframe-chip", "{source.transport}" } }
                                        td {
                                            span { class: "pill {health_class(source.health_state)}",
                                                "{health_label(source.health_state, &strings)}"
                                            }
                                        }
                                        td { class: "signal-time", "{format_optional_timestamp(source.last_fetch_at)}" }
                                        td { class: "signal-time", "{format_optional_timestamp(source.last_item_at)}" }
                                        td { class: "signal-source-method",
                                            "{source.last_error.clone().unwrap_or_else(|| strings.news_no_error.to_owned())}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "panel-head news-sources-head",
                div {
                    h3 { "{strings.news_clusters}" }
                    p { class: "panel-subtitle", "{strings.news_clusters_hint}" }
                }
            }
            if let Some(error) = state.clusters_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.clusters.as_ref() {
                None if state.clusters_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_unavailable}" }
                        p { "{strings.news_clusters_empty_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.news_clusters_loading}" }
                    }
                },
                Some(clusters) if clusters.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.news_clusters_empty}" }
                        p { "{strings.news_clusters_empty_hint}" }
                    }
                },
                Some(clusters) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services news-clusters-table",
                            thead {
                                tr {
                                    th { "Cluster" }
                                    th { "Items" }
                                    th { "Topics" }
                                    th { "Last seen" }
                                }
                            }
                            tbody {
                                for cluster in clusters.iter() {
                                    tr {
                                        td { span { class: "timeframe-chip", "{cluster.id}" } }
                                        td { "{cluster.item_count}" }
                                        td {
                                            div { class: "chips",
                                                for token in cluster.tokens.iter() {
                                                    span { class: "chip", "{token}" }
                                                }
                                            }
                                        }
                                        td { class: "signal-time", "{format_timestamp(cluster.last_seen_at)}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Shows one story with its excerpt and provenance.
#[component]
pub fn NewsItemSlideover(
    state: NewsPanelState,
    strings: UiStrings,
    on_close: EventHandler<()>,
) -> Element {
    let title = state
        .detail
        .as_ref()
        .map_or_else(|| strings.news_detail.to_owned(), |item| item.title.clone());

    rsx! {
        Slideover {
            title,
            close_label: strings.news_close,
            on_close,
            {
                rsx! {
                    if let Some(error) = state.detail_error.as_ref() {
                        div { class: "banner error", role: "alert", "{error}" }
                    }
                    match state.detail.as_ref() {
                        None if state.detail_error.is_some() => rsx! {
                            div { class: "empty",
                                p { class: "empty-title", "{strings.news_detail_unavailable}" }
                                p { "{strings.news_detail_unavailable_hint}" }
                            }
                        },
                        None => rsx! {
                            div { class: "empty loading-state", role: "status",
                                p { class: "empty-title", "{strings.news_detail_loading}" }
                                p { "{strings.news_detail_loading_hint}" }
                            }
                        },
                        Some(item) => rsx! {
                            div { class: "research-detail",
                                div { class: "research-detail-hero",
                                    div {
                                        span { class: "eyebrow", "{item.source_id} · {item.market_group}" }
                                        h3 { "{item.title}" }
                                    }
                                    div { class: "detail-badges",
                                        span { class: "pill {level_class(item.level)}",
                                            "{level_label(item.level, &strings)}"
                                        }
                                        span { class: "pill {direction_class(item.direction)}",
                                            "{direction_label(item.direction, &strings)}"
                                        }
                                        span { class: "pill {freshness_class(item.freshness)}",
                                            "{freshness_label(item.freshness, &strings)}"
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.news_excerpt}" }
                                    if item.excerpt.is_empty() {
                                        p { class: "muted", "{strings.news_no_excerpt}" }
                                    } else {
                                        p { "{item.excerpt}" }
                                    }
                                    a {
                                        class: "button primary",
                                        href: "{item.url}",
                                        target: "_blank",
                                        rel: "noreferrer",
                                        "{strings.news_open_article}"
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.news_provenance}" }
                                    dl { class: "provenance-grid",
                                        NewsProvenanceItem { label: strings.news_item_id, value: item.id.clone() }
                                        NewsProvenanceItem { label: strings.news_source, value: item.source_id.clone() }
                                        NewsProvenanceItem { label: strings.news_language, value: item.language.clone() }
                                        NewsProvenanceItem { label: strings.news_market_group, value: item.market_group.clone() }
                                        NewsProvenanceItem {
                                            label: strings.news_event_type,
                                            value: item.event_type.clone().unwrap_or_else(|| strings.news_no_event_type.to_owned()),
                                        }
                                        NewsProvenanceItem { label: strings.news_published, value: format_optional_timestamp(item.published_at) }
                                        NewsProvenanceItem { label: strings.news_first_seen, value: format_timestamp(item.first_seen_at) }
                                        NewsProvenanceItem { label: strings.news_ingested, value: format_timestamp(item.ingested_at) }
                                        NewsProvenanceItem { label: strings.news_article_url, value: item.url.clone() }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.news_tags}" }
                                    if item.tags.is_empty() {
                                        p { class: "muted", "{strings.news_no_tags}" }
                                    } else {
                                        div { class: "chips",
                                            for tag in item.tags.iter() {
                                                span { class: "chip", "{tag}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NewsProvenanceItem(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_optional_timestamp(value: Option<DateTime<Utc>>) -> String {
    value.map_or_else(|| "-".to_owned(), format_timestamp)
}

fn format_age(seconds: i64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3_600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
}

fn freshness_label(freshness: NewsFreshnessDto, strings: &UiStrings) -> &'static str {
    match freshness {
        NewsFreshnessDto::Fresh => strings.news_fresh,
        NewsFreshnessDto::Stale => strings.news_stale,
    }
}

fn freshness_class(freshness: NewsFreshnessDto) -> &'static str {
    match freshness {
        NewsFreshnessDto::Fresh => "freshness-fresh",
        NewsFreshnessDto::Stale => "freshness-stale",
    }
}

fn level_label(level: NewsLevelDto, strings: &UiStrings) -> &'static str {
    match level {
        NewsLevelDto::High => strings.news_level_high,
        NewsLevelDto::Med => strings.news_level_med,
        NewsLevelDto::Low => strings.news_level_low,
    }
}

fn level_class(level: NewsLevelDto) -> &'static str {
    match level {
        NewsLevelDto::High => "level-high",
        NewsLevelDto::Med => "level-med",
        NewsLevelDto::Low => "level-low",
    }
}

fn direction_label(direction: NewsDirectionDto, strings: &UiStrings) -> &'static str {
    match direction {
        NewsDirectionDto::Pos => strings.news_direction_pos,
        NewsDirectionDto::Neg => strings.news_direction_neg,
        NewsDirectionDto::Flat => strings.news_direction_flat,
    }
}

fn direction_class(direction: NewsDirectionDto) -> &'static str {
    match direction {
        NewsDirectionDto::Pos => "direction-bullish",
        NewsDirectionDto::Neg => "direction-bearish",
        NewsDirectionDto::Flat => "direction-neutral",
    }
}

fn health_label(health: NewsHealthStateDto, strings: &UiStrings) -> &'static str {
    match health {
        NewsHealthStateDto::Ok => strings.news_health_ok,
        NewsHealthStateDto::Degraded => strings.news_health_degraded,
        NewsHealthStateDto::Down => strings.news_health_down,
        NewsHealthStateDto::Unknown => strings.news_health_unknown,
    }
}

fn health_class(health: NewsHealthStateDto) -> &'static str {
    match health {
        NewsHealthStateDto::Ok => "state-ok",
        NewsHealthStateDto::Degraded => "state-partial",
        NewsHealthStateDto::Down => "state-failed",
        NewsHealthStateDto::Unknown => "state-unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tm_trading_contracts::NewsDirectionDto;

    fn item(id: &str) -> NewsItemDto {
        NewsItemDto {
            id: id.to_owned(),
            source_id: "coindesk".to_owned(),
            url: "https://example.com/story".to_owned(),
            title: "Bitcoin holds support".to_owned(),
            excerpt: "Summary.".to_owned(),
            language: "en".to_owned(),
            published_at: None,
            first_seen_at: Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap(),
            ingested_at: Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap(),
            market_group: "crypto".to_owned(),
            event_type: None,
            tags: Vec::new(),
            level: NewsLevelDto::Med,
            direction: NewsDirectionDto::Flat,
            score: 0.0,
            freshness: NewsFreshnessDto::Fresh,
            age_seconds: 0,
        }
    }

    #[test]
    fn a_detail_response_for_a_different_story_is_ignored() {
        let mut state = NewsPanelState::default();
        state.open("news-1".to_owned());
        state.detail_loaded(item("news-2"));
        assert!(state.detail.is_none());
    }

    #[test]
    fn closing_clears_the_selection_and_the_detail() {
        let mut state = NewsPanelState::default();
        state.open("news-1".to_owned());
        state.detail_loaded(item("news-1"));
        assert!(state.detail.is_some());
        state.close();
        assert!(state.selected_id.is_none());
        assert!(state.detail.is_none());
    }

    #[test]
    fn a_failed_detail_keeps_the_error_for_the_selected_story_only() {
        let mut state = NewsPanelState::default();
        state.open("news-1".to_owned());
        state.detail_failed("news-2", "boom".to_owned());
        assert!(state.detail_error.is_none());
        state.detail_failed("news-1", "boom".to_owned());
        assert_eq!(state.detail_error.as_deref(), Some("boom"));
    }
}
