use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{ResearchEventDto, ResearchFreshnessDto, ResearchQualityDto};

use crate::i18n::UiStrings;
use crate::slideover::Slideover;

/// Independent state for the Signal Center and its detail overlay.
#[derive(Clone, Default, PartialEq)]
pub struct ResearchPanelState {
    pub events: Option<Vec<ResearchEventDto>>,
    pub list_error: Option<String>,
    pub selected_id: Option<String>,
    pub detail: Option<ResearchEventDto>,
    pub detail_error: Option<String>,
}

impl ResearchPanelState {
    pub fn open(&mut self, event_id: String) {
        self.selected_id = Some(event_id);
        self.detail = None;
        self.detail_error = None;
    }

    pub fn close(&mut self) {
        self.selected_id = None;
        self.detail = None;
        self.detail_error = None;
    }

    pub fn list_loaded(&mut self, events: Vec<ResearchEventDto>) {
        self.events = Some(events);
        self.list_error = None;
    }

    pub fn list_failed(&mut self, error: String) {
        self.list_error = Some(error);
    }

    pub fn detail_loaded(&mut self, event: ResearchEventDto) {
        if self.selected_id.as_deref() == Some(event.id.as_str()) {
            self.detail = Some(event);
            self.detail_error = None;
        }
    }

    pub fn detail_failed(&mut self, event_id: &str, error: String) {
        if self.selected_id.as_deref() == Some(event_id) {
            self.detail = None;
            self.detail_error = Some(error);
        }
    }
}

/// Renders owner-scoped deterministic research observations.
#[component]
pub fn SignalCenterPanel(
    state: ResearchPanelState,
    strings: UiStrings,
    on_open: EventHandler<String>,
) -> Element {
    let event_count = state.events.as_ref().map_or(0, Vec::len);
    let fresh_count = state.events.as_ref().map_or(0, |events| {
        events
            .iter()
            .filter(|event| event.freshness == ResearchFreshnessDto::Fresh)
            .count()
    });

    rsx! {
        section { class: "panel signal-center-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.signal_center}" }
                    p { class: "panel-subtitle", "{strings.signal_center_hint}" }
                }
                div { class: "signal-summary", aria_label: "{strings.signal_summary}",
                    span { strong { "{event_count}" } " {strings.research_events_count}" }
                    span { strong { "{fresh_count}" } " {strings.research_fresh_count}" }
                }
            }
            if let Some(error) = state.list_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.events.as_ref() {
                None if state.list_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.research_unavailable}" }
                        p { "{strings.research_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.research_loading}" }
                        p { "{strings.research_loading_hint}" }
                    }
                },
                Some(events) if events.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.research_empty}" }
                        p { "{strings.research_empty_hint}" }
                    }
                },
                Some(events) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services signal-table",
                            thead {
                                tr {
                                    th { "{strings.research_asset_column}" }
                                    th { "{strings.research_event_column}" }
                                    th { "{strings.research_timeframe_column}" }
                                    th { "{strings.research_context_column}" }
                                    th { "{strings.research_quality_column}" }
                                    th { "{strings.research_detected_column}" }
                                    th { "{strings.research_source_column}" }
                                    th { "{strings.research_freshness_column}" }
                                }
                            }
                            tbody {
                                for event in events.iter() {
                                    {
                                        let event_id = event.id.clone();
                                        let open = on_open;
                                        rsx! {
                                            tr { class: "signal-row",
                                                td {
                                                    button {
                                                        class: "signal-open",
                                                        type: "button",
                                                        onclick: move |_| open.call(event_id.clone()),
                                                        "{event.symbol}"
                                                    }
                                                }
                                                td {
                                                    div { class: "signal-event-type", "{event.event_type}" }
                                                    div { class: "signal-direction {direction_class(event.direction)}",
                                                        "{direction_label(event.direction, &strings)} · {format_score(event.score)}"
                                                    }
                                                }
                                                td { span { class: "timeframe-chip", "{event.timeframe}" } }
                                                td {
                                                    div { class: "signal-context", title: "{context_summary(&event.context)}",
                                                        "{context_summary(&event.context)}"
                                                    }
                                                }
                                                td {
                                                    span { class: "pill {quality_class(event.quality_state)}",
                                                        "{quality_label(event.quality_state, &strings)}"
                                                    }
                                                }
                                                td { class: "signal-time", "{format_timestamp(event.detected_at)}" }
                                                td {
                                                    div { class: "signal-source", "{event.source_id}" }
                                                    div { class: "signal-source-method", "{event.provider_method}" }
                                                }
                                                td {
                                                    span { class: "pill {freshness_class(event.freshness)}",
                                                        "{freshness_label(event.freshness, &strings)}"
                                                    }
                                                    div { class: "signal-age", "{format_age(event.age_seconds)}" }
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
}

/// Shows one research event with its evidence and provenance.
#[component]
pub fn ResearchEventSlideover(
    state: ResearchPanelState,
    strings: UiStrings,
    on_close: EventHandler<()>,
) -> Element {
    let title = state.detail.as_ref().map_or_else(
        || strings.research_detail.to_owned(),
        |event| format!("{} · {}", event.symbol, event.event_type),
    );

    rsx! {
        Slideover {
            title,
            close_label: strings.research_close,
            on_close,
            {
                rsx! {
                if let Some(error) = state.detail_error.as_ref() {
                    div { class: "banner error", role: "alert", "{error}" }
                }
                match state.detail.as_ref() {
                    None if state.detail_error.is_some() => rsx! {
                        div { class: "empty",
                            p { class: "empty-title", "{strings.research_detail_unavailable}" }
                            p { "{strings.research_detail_unavailable_hint}" }
                        }
                    },
                    None => rsx! {
                        div { class: "empty loading-state", role: "status",
                            p { class: "empty-title", "{strings.research_detail_loading}" }
                            p { "{strings.research_detail_loading_hint}" }
                        }
                    },
                    Some(event) => rsx! {
                        div { class: "research-detail",
                            div { class: "research-detail-hero",
                                div {
                                    span { class: "eyebrow", "{event.symbol} · {event.timeframe}" }
                                    h3 { "{event.event_type}" }
                                }
                                div { class: "detail-badges",
                                    span { class: "pill {direction_class(event.direction)}",
                                        "{direction_label(event.direction, &strings)}"
                                    }
                                    span { class: "pill {quality_class(event.quality_state)}",
                                        "{quality_label(event.quality_state, &strings)}"
                                    }
                                    span { class: "pill {freshness_class(event.freshness)}",
                                        "{freshness_label(event.freshness, &strings)}"
                                    }
                                }
                            }
                            div { class: "detail-score-grid",
                                div { span { class: "detail-label", "{strings.research_score}" } strong { "{format_score(event.score)}" } }
                                div { span { class: "detail-label", "{strings.research_age}" } strong { "{format_age(event.age_seconds)}" } }
                                div { span { class: "detail-label", "{strings.research_detected}" } strong { "{format_timestamp(event.detected_at)}" } }
                            }
                            section { class: "detail-section",
                                h4 { "{strings.research_provenance}" }
                                dl { class: "provenance-grid",
                                    ProvenanceItem { label: strings.research_event_id, value: event.id.clone() }
                                    ProvenanceItem { label: strings.research_owner, value: event.owner_scope.clone() }
                                    ProvenanceItem { label: strings.research_method, value: format!("{} v{}", event.method, event.method_version) }
                                    ProvenanceItem { label: strings.research_source, value: event.source_id.clone() }
                                    ProvenanceItem { label: strings.research_provider_method, value: event.provider_method.clone() }
                                    ProvenanceItem { label: strings.research_input_snapshot, value: event.input_snapshot_id.clone() }
                                    ProvenanceItem { label: strings.research_producer, value: event.producer_version.clone() }
                                    ProvenanceItem { label: strings.research_schema, value: event.schema_version.clone() }
                                    ProvenanceItem { label: strings.research_observed, value: format_timestamp(event.observed_at) }
                                    ProvenanceItem { label: strings.research_ingested, value: format_timestamp(event.ingested_at) }
                                }
                            }
                            section { class: "detail-section",
                                h4 { "{strings.research_evidence}" }
                                if event.evidence.is_empty() {
                                    p { class: "muted", "{strings.research_no_evidence}" }
                                } else {
                                    dl { class: "metadata-list",
                                        for (key, value) in event.evidence.iter() {
                                            div {
                                                dt { "{key}" }
                                                dd { "{value:.4}" }
                                            }
                                        }
                                    }
                                }
                            }
                            section { class: "detail-section",
                                h4 { "{strings.research_context}" }
                                if event.context.is_empty() {
                                    p { class: "muted", "{strings.research_no_context}" }
                                } else {
                                    dl { class: "metadata-list",
                                        for (key, value) in event.context.iter() {
                                            div {
                                                dt { "{key}" }
                                                dd { "{value}" }
                                            }
                                        }
                                    }
                                }
                            }
                            section { class: "detail-section",
                                h4 { "{strings.research_quality_reasons}" }
                                if event.quality_reasons.is_empty() {
                                    p { class: "muted", "{strings.research_no_quality_reasons}" }
                                } else {
                                    ul { class: "quality-reasons",
                                        for reason in event.quality_reasons.iter() {
                                            li { "{reason}" }
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
fn ProvenanceItem(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

#[allow(dead_code)]
fn context_summary(context: &BTreeMap<String, String>) -> String {
    context
        .iter()
        .take(2)
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_score(value: f64) -> String {
    format!("{value:.2}")
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

fn direction_label(direction: i8, strings: &UiStrings) -> &'static str {
    match direction {
        1 => strings.research_direction_bullish,
        -1 => strings.research_direction_bearish,
        _ => strings.research_direction_neutral,
    }
}

fn direction_class(direction: i8) -> &'static str {
    match direction {
        1 => "direction-bullish",
        -1 => "direction-bearish",
        _ => "direction-neutral",
    }
}

fn quality_label(quality: ResearchQualityDto, strings: &UiStrings) -> &'static str {
    match quality {
        ResearchQualityDto::Complete => strings.research_quality_complete,
        ResearchQualityDto::Partial => strings.research_quality_partial,
        ResearchQualityDto::Unknown => strings.research_quality_unknown,
    }
}

fn quality_class(quality: ResearchQualityDto) -> &'static str {
    match quality {
        ResearchQualityDto::Complete => "quality-complete",
        ResearchQualityDto::Partial => "quality-partial",
        ResearchQualityDto::Unknown => "quality-unknown",
    }
}

fn freshness_label(freshness: ResearchFreshnessDto, strings: &UiStrings) -> &'static str {
    match freshness {
        ResearchFreshnessDto::Fresh => strings.research_fresh,
        ResearchFreshnessDto::Stale => strings.research_stale,
    }
}

fn freshness_class(freshness: ResearchFreshnessDto) -> &'static str {
    match freshness {
        ResearchFreshnessDto::Fresh => "freshness-fresh",
        ResearchFreshnessDto::Stale => "freshness-stale",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tm_trading_contracts::ResearchEventDto;

    fn event(id: &str) -> ResearchEventDto {
        ResearchEventDto {
            id: id.to_owned(),
            owner_scope: "owner".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: "1m".to_owned(),
            event_type: "ST LONG".to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: Utc::now(),
            detected_at: Utc::now(),
            ingested_at: Utc::now(),
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: "rsnap1_1".to_owned(),
            producer_version: "research-worker-v1".to_owned(),
            schema_version: "1".to_owned(),
            quality_state: ResearchQualityDto::Complete,
            quality_reasons: Vec::new(),
            freshness: ResearchFreshnessDto::Fresh,
            age_seconds: 0,
            evidence: BTreeMap::new(),
            context: BTreeMap::new(),
        }
    }

    #[test]
    fn a_detail_response_for_a_different_event_is_ignored() {
        let mut state = ResearchPanelState::default();
        state.open("event-1".to_owned());
        state.detail_loaded(event("event-2"));
        assert!(state.detail.is_none());
    }

    #[test]
    fn context_summary_uses_at_most_two_deterministic_entries() {
        let context = BTreeMap::from([
            ("first".to_owned(), "1".to_owned()),
            ("second".to_owned(), "2".to_owned()),
            ("third".to_owned(), "3".to_owned()),
        ]);
        assert_eq!(context_summary(&context), "first=1 · second=2");
    }
}
