use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{
    AiBriefingDto, AiBriefingReferenceKindDto, AiBriefingTaskDto, AiJobDto, AiJobStatusDto,
    ResearchEventDto,
};

use crate::i18n::UiStrings;
use crate::slideover::Slideover;

/// Localized failure state for one part of the AI Briefing panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiBriefingPanelError {
    List,
    Detail,
    Jobs,
    Request,
}

/// Independent state for the AI Briefing panel and its detail overlay.
#[derive(Clone, Default, PartialEq)]
pub struct AiBriefingPanelState {
    pub briefings: Option<Vec<AiBriefingDto>>,
    pub list_error: Option<AiBriefingPanelError>,
    pub selected_id: Option<String>,
    pub detail: Option<AiBriefingDto>,
    pub detail_error: Option<AiBriefingPanelError>,
    pub latest_job: Option<AiJobDto>,
    pub job_error: Option<AiBriefingPanelError>,
    pub request_error: Option<AiBriefingPanelError>,
}

impl AiBriefingPanelState {
    pub fn list_loaded(&mut self, briefings: Vec<AiBriefingDto>) {
        self.briefings = Some(briefings);
        self.list_error = None;
    }

    pub fn list_failed(&mut self) {
        self.list_error = Some(AiBriefingPanelError::List);
    }

    pub fn jobs_loaded(&mut self, jobs: Vec<AiJobDto>) {
        if let Some(job) = jobs.first() {
            self.latest_job = Some(job.clone());
        }
        self.job_error = None;
    }

    pub fn jobs_failed(&mut self) {
        self.job_error = Some(AiBriefingPanelError::Jobs);
    }

    pub fn request_created(&mut self, job: AiJobDto) {
        self.latest_job = Some(job);
        self.request_error = None;
    }

    pub fn request_failed(&mut self) {
        self.request_error = Some(AiBriefingPanelError::Request);
    }

    pub fn open(&mut self, briefing_id: String) {
        self.selected_id = Some(briefing_id);
        self.detail = None;
        self.detail_error = None;
    }

    pub fn close(&mut self) {
        self.selected_id = None;
        self.detail = None;
        self.detail_error = None;
    }

    pub fn detail_loaded(&mut self, briefing: AiBriefingDto) {
        if self.selected_id.as_deref() == Some(briefing.id.as_str()) {
            self.detail = Some(briefing);
            self.detail_error = None;
        }
    }

    pub fn detail_failed(&mut self, briefing_id: &str) {
        if self.selected_id.as_deref() == Some(briefing_id) {
            self.detail = None;
            self.detail_error = Some(AiBriefingPanelError::Detail);
        }
    }
}

/// Renders owner-scoped AI briefings with their evidence counts.
#[component]
pub fn AiBriefingPanel(
    state: AiBriefingPanelState,
    strings: UiStrings,
    research_events: Vec<ResearchEventDto>,
    on_open: EventHandler<String>,
    on_request: EventHandler<String>,
) -> Element {
    let briefing_count = state.briefings.as_ref().map_or(0, Vec::len);
    let mut selected_research_event_id = use_signal(String::new);
    let selected_id = selected_research_event_id.read().clone();
    let can_request = research_events.iter().any(|event| event.id == selected_id);

    rsx! {
        section { class: "panel ai-briefing-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.ai_briefing}" }
                    p { class: "panel-subtitle", "{strings.ai_briefing_hint}" }
                }
                div { class: "signal-summary", aria_label: "{strings.ai_briefing_summary}",
                    span { strong { "{briefing_count}" } " {strings.ai_briefing_briefings_count}" }
                }
            }
            div { class: "ai-briefing-request",
                div { class: "ai-briefing-request-field",
                    span { class: "detail-label", "{strings.ai_briefing_request_label}" }
                    select {
                        class: "text-input",
                        value: "{selected_id}",
                        aria_label: "{strings.ai_briefing_request_label}",
                        disabled: research_events.is_empty(),
                        onchange: move |event| selected_research_event_id.set(event.value()),
                        option { value: "", disabled: true, "{strings.ai_briefing_request_select}" }
                        for event in research_events.iter() {
                            option { value: "{event.id}",
                                "{event.symbol} · {event.event_type} · {event.id}"
                            }
                        }
                    }
                }
                button {
                    class: "button primary",
                    type: "button",
                    disabled: !can_request,
                    onclick: move |_| on_request.call(selected_id.clone()),
                    "{strings.ai_briefing_request_button}"
                }
            }
            if research_events.is_empty() {
                p { class: "panel-subtitle ai-briefing-request-hint", "{strings.ai_briefing_request_unavailable}" }
            }
            if state.request_error.is_some() {
                div { class: "banner error", role: "alert", "{strings.ai_briefing_request_failed}" }
            }
            if state.job_error.is_some() {
                div { class: "banner error", role: "alert", "{strings.ai_briefing_jobs_unavailable_hint}" }
            }
            if let Some(job) = state.latest_job.as_ref() {
                {
                    let open_briefing_id = job.briefing_id.clone();
                    rsx! {
                        div { class: "ai-briefing-job", role: "status",
                            span { class: "detail-label", "{strings.ai_briefing_job_status}" }
                            span { class: "pill {job_status_class(job.status)}",
                                "{job_status_label(job.status, &strings)}"
                            }
                            span { class: "operation-id", "{job.id}" }
                            if let Some(error_code) = job.error_code.as_ref() {
                                span { class: "error-text", "{error_code}" }
                            }
                            if let Some(briefing_id) = open_briefing_id {
                                button {
                                    class: "button",
                                    type: "button",
                                    onclick: move |_| on_open.call(briefing_id.clone()),
                                    "{strings.ai_briefing_open_briefing}"
                                }
                            }
                        }
                    }
                }
            }
            if state.list_error.is_some() {
                div { class: "banner error", role: "alert", "{strings.ai_briefing_unavailable_hint}" }
            }
            match state.briefings.as_ref() {
                None if state.list_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_briefing_unavailable}" }
                        p { "{strings.ai_briefing_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.ai_briefing_loading}" }
                        p { "{strings.ai_briefing_loading_hint}" }
                    }
                },
                Some(briefings) if briefings.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_briefing_empty}" }
                        p { "{strings.ai_briefing_empty_hint}" }
                    }
                },
                Some(briefings) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services signal-table",
                            thead {
                                tr {
                                    th { "{strings.ai_briefing_task_column}" }
                                    th { "{strings.ai_briefing_summary_column}" }
                                    th { "{strings.ai_briefing_model_column}" }
                                    th { "{strings.ai_briefing_evidence_column}" }
                                    th { "{strings.ai_briefing_generated_column}" }
                                }
                            }
                            tbody {
                                for briefing in briefings.iter() {
                                    {
                                        let briefing_id = briefing.id.clone();
                                        let open = on_open;
                                        rsx! {
                                            tr { class: "signal-row",
                                                td {
                                                    button {
                                                        class: "signal-open",
                                                        type: "button",
                                                        onclick: move |_| open.call(briefing_id.clone()),
                                                        "{task_label(briefing.task, &strings)}"
                                                    }
                                                }
                                                td { class: "signal-context", "{briefing_summary(briefing)}" }
                                                td {
                                                    div { class: "signal-source", "{briefing.model_id}" }
                                                    div { class: "signal-source-method", "{briefing.provider_id}" }
                                                }
                                                td { class: "signal-time",
                                                    "{briefing.input_snapshot_ids.len()} {strings.ai_briefing_snapshots} · {briefing.references.len()} {strings.ai_briefing_references}"
                                                }
                                                td { class: "signal-time", "{format_timestamp(briefing.generated_at)}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Shows one evidence-bound briefing with its claims and provenance.
#[component]
pub fn AiBriefingSlideover(
    state: AiBriefingPanelState,
    strings: UiStrings,
    on_close: EventHandler<()>,
) -> Element {
    let title = state.detail.as_ref().map_or_else(
        || strings.ai_briefing_detail.to_owned(),
        |briefing| {
            format!(
                "{} · {}",
                strings.ai_briefing_detail,
                task_label(briefing.task, &strings)
            )
        },
    );

    rsx! {
        Slideover {
            title,
            close_label: strings.ai_briefing_close,
            on_close,
            {
                rsx! {
                    if state.detail_error.is_some() {
                        div { class: "banner error", role: "alert", "{strings.ai_briefing_detail_unavailable_hint}" }
                    }
                    match state.detail.as_ref() {
                        None if state.detail_error.is_some() => rsx! {
                            div { class: "empty",
                                p { class: "empty-title", "{strings.ai_briefing_detail_unavailable}" }
                                p { "{strings.ai_briefing_detail_unavailable_hint}" }
                            }
                        },
                        None => rsx! {
                            div { class: "empty loading-state", role: "status",
                                p { class: "empty-title", "{strings.ai_briefing_detail_loading}" }
                                p { "{strings.ai_briefing_detail_loading_hint}" }
                            }
                        },
                        Some(briefing) => rsx! {
                            div { class: "research-detail",
                                div { class: "research-detail-hero",
                                    div {
                                        span { class: "eyebrow", "{task_label(briefing.task, &strings)}" }
                                        h3 { "{briefing_summary(briefing)}" }
                                    }
                                }
                                div { class: "detail-score-grid",
                                    div { span { class: "detail-label", "{strings.ai_briefing_model_column}" } strong { "{briefing.model_id}" } }
                                    div { span { class: "detail-label", "{strings.ai_briefing_provider}" } strong { "{briefing.provider_id}" } }
                                    div { span { class: "detail-label", "{strings.ai_briefing_generated_column}" } strong { "{format_timestamp(briefing.generated_at)}" } }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_facts}" }
                                    if briefing.facts.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for fact in briefing.facts.iter() {
                                                li { "{fact}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_interpretations}" }
                                    if briefing.interpretations.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for interpretation in briefing.interpretations.iter() {
                                                li { "{interpretation}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_uncertainty}" }
                                    if briefing.uncertainties.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for uncertainty in briefing.uncertainties.iter() {
                                                li { "{uncertainty}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_evidence}" }
                                    if briefing.references.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        dl { class: "metadata-list",
                                            for reference in briefing.references.iter() {
                                                div {
                                                    dt { "{reference_kind_label(reference.kind, &strings)}" }
                                                    dd { "{reference.value}" }
                                                }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_input_snapshots}" }
                                    if briefing.input_snapshot_ids.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for input_snapshot_id in briefing.input_snapshot_ids.iter() {
                                                li { "{input_snapshot_id}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_missing_inputs}" }
                                    if briefing.missing_inputs.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for missing_input in briefing.missing_inputs.iter() {
                                                li { "{missing_input}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_stale_inputs}" }
                                    if briefing.stale_inputs.is_empty() {
                                        p { class: "muted", "{strings.ai_briefing_none}" }
                                    } else {
                                        ul { class: "quality-reasons",
                                            for stale_input in briefing.stale_inputs.iter() {
                                                li { "{stale_input}" }
                                            }
                                        }
                                    }
                                }
                                section { class: "detail-section",
                                    h4 { "{strings.ai_briefing_provenance}" }
                                    dl { class: "provenance-grid",
                                        BriefingProvenanceItem { label: strings.ai_briefing_id, value: briefing.id.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_owner, value: briefing.owner_scope.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_provider, value: briefing.provider_id.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_model_column, value: briefing.model_registry_id.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_prompt, value: briefing.prompt_version.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_producer, value: briefing.producer_version.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_schema, value: briefing.schema_version.clone() }
                                        BriefingProvenanceItem { label: strings.ai_briefing_generated_column, value: format_timestamp(briefing.generated_at) }
                                    }
                                }
                            }
                        },
                    }
                }
            }
        }
    }
}

#[component]
fn BriefingProvenanceItem(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

fn briefing_summary(briefing: &AiBriefingDto) -> &str {
    briefing
        .facts
        .first()
        .or_else(|| briefing.interpretations.first())
        .or_else(|| briefing.uncertainties.first())
        .map_or("", String::as_str)
}

fn task_label(task: AiBriefingTaskDto, strings: &UiStrings) -> &'static str {
    match task {
        AiBriefingTaskDto::NewsClassification => strings.ai_briefing_task_news_classification,
        AiBriefingTaskDto::NewsSummary => strings.ai_briefing_task_news_summary,
        AiBriefingTaskDto::MarketBriefing => strings.ai_briefing_task_market_briefing,
        AiBriefingTaskDto::ResearchExplanation => strings.ai_briefing_task_research_explanation,
        AiBriefingTaskDto::DailyDigest => strings.ai_briefing_task_daily_digest,
    }
}

fn job_status_label(status: AiJobStatusDto, strings: &UiStrings) -> &'static str {
    match status {
        AiJobStatusDto::Queued => strings.ai_briefing_job_queued,
        AiJobStatusDto::Claimed => strings.ai_briefing_job_claimed,
        AiJobStatusDto::Succeeded => strings.ai_briefing_job_succeeded,
        AiJobStatusDto::Failed => strings.ai_briefing_job_failed,
        AiJobStatusDto::Expired => strings.ai_briefing_job_expired,
        AiJobStatusDto::OutcomeUnknown => strings.ai_briefing_job_outcome_unknown,
    }
}

fn job_status_class(status: AiJobStatusDto) -> &'static str {
    match status {
        AiJobStatusDto::Queued | AiJobStatusDto::Claimed => "status-progress",
        AiJobStatusDto::Succeeded => "status-succeeded",
        AiJobStatusDto::Failed => "status-failed",
        AiJobStatusDto::Expired | AiJobStatusDto::OutcomeUnknown => "status-cancelled",
    }
}

fn reference_kind_label(kind: AiBriefingReferenceKindDto, strings: &UiStrings) -> &'static str {
    match kind {
        AiBriefingReferenceKindDto::ResearchEvent => strings.ai_briefing_reference_research_event,
        AiBriefingReferenceKindDto::NewsItem => strings.ai_briefing_reference_news_item,
        AiBriefingReferenceKindDto::SourceUrl => strings.ai_briefing_reference_source_url,
    }
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap()
    }

    fn briefing(id: &str) -> AiBriefingDto {
        AiBriefingDto {
            id: id.to_owned(),
            owner_scope: "local-operator".to_owned(),
            task: AiBriefingTaskDto::MarketBriefing,
            provider_id: "primary".to_owned(),
            model_registry_id: "primary-large".to_owned(),
            model_id: "model-a".to_owned(),
            prompt_version: "market-briefing.v1".to_owned(),
            facts: vec!["Market fact".to_owned()],
            interpretations: vec!["Model interpretation".to_owned()],
            uncertainties: Vec::new(),
            input_snapshot_ids: vec!["rsnap1_0123456789abcdef".to_owned()],
            references: vec![tm_trading_contracts::AiBriefingReferenceDto {
                kind: AiBriefingReferenceKindDto::ResearchEvent,
                value: "rev1_0123456789abcdef".to_owned(),
            }],
            missing_inputs: Vec::new(),
            stale_inputs: Vec::new(),
            generated_at: at(),
            producer_version: "tm-trading-ai-worker/0.1.0".to_owned(),
            schema_version: "ai-briefing.v1".to_owned(),
        }
    }

    #[test]
    fn loading_briefings_clears_the_previous_list_error() {
        let mut state = AiBriefingPanelState::default();
        state.list_failed();
        state.list_loaded(vec![briefing("aibr1_1")]);

        assert!(state.list_error.is_none());
        assert_eq!(state.briefings.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_refresh_keeps_the_last_loaded_briefings() {
        let mut state = AiBriefingPanelState::default();
        state.list_loaded(vec![briefing("aibr1_1")]);
        state.list_failed();

        assert_eq!(state.list_error, Some(AiBriefingPanelError::List));
        assert_eq!(state.briefings.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn a_detail_response_for_the_previous_selection_is_ignored() {
        let mut state = AiBriefingPanelState::default();
        state.open("aibr1_2".to_owned());
        state.detail_loaded(briefing("aibr1_1"));

        assert!(state.detail.is_none());
    }

    #[test]
    fn closing_clears_the_selected_briefing() {
        let mut state = AiBriefingPanelState::default();
        state.open("aibr1_1".to_owned());
        state.detail_loaded(briefing("aibr1_1"));
        state.close();

        assert!(state.selected_id.is_none());
        assert!(state.detail.is_none());
    }

    #[test]
    fn briefing_task_labels_are_localized() {
        let english = UiStrings::for_locale(Locale::English);
        let vietnamese = UiStrings::for_locale(Locale::Vietnamese);

        assert_ne!(
            task_label(AiBriefingTaskDto::MarketBriefing, &english),
            task_label(AiBriefingTaskDto::MarketBriefing, &vietnamese)
        );
    }

    fn job(
        id: &str,
        status: tm_trading_contracts::AiJobStatusDto,
    ) -> tm_trading_contracts::AiJobDto {
        tm_trading_contracts::AiJobDto {
            id: id.to_owned(),
            owner_scope: "local-operator".to_owned(),
            correlation_id: "correlation-1".to_owned(),
            task: AiBriefingTaskDto::ResearchExplanation,
            status,
            attempt: 0,
            research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
            news_item_ids: Vec::new(),
            requested_at: at(),
            expires_at: at(),
            finished_at: None,
            briefing_id: None,
            error_code: None,
        }
    }

    #[test]
    fn a_created_request_tracks_the_latest_job_and_clears_the_request_error() {
        let mut state = AiBriefingPanelState::default();
        state.request_failed();
        state.request_created(job(
            "aijob1_1",
            tm_trading_contracts::AiJobStatusDto::Queued,
        ));

        assert_eq!(state.latest_job.as_ref().unwrap().id, "aijob1_1");
        assert!(state.request_error.is_none());
    }

    #[test]
    fn a_failed_job_refresh_keeps_the_last_loaded_job() {
        let mut state = AiBriefingPanelState::default();
        state.jobs_loaded(vec![job(
            "aijob1_1",
            tm_trading_contracts::AiJobStatusDto::Claimed,
        )]);
        state.jobs_failed();

        assert_eq!(state.latest_job.as_ref().unwrap().id, "aijob1_1");
        assert_eq!(state.job_error, Some(AiBriefingPanelError::Jobs));
    }

    #[test]
    fn job_status_labels_are_localized() {
        let english = UiStrings::for_locale(Locale::English);
        let vietnamese = UiStrings::for_locale(Locale::Vietnamese);

        assert_ne!(
            job_status_label(tm_trading_contracts::AiJobStatusDto::Queued, &english),
            job_status_label(tm_trading_contracts::AiJobStatusDto::Queued, &vietnamese)
        );
    }
}
