use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{AiProviderProfileDto, AiProviderProfileListDto};

use crate::i18n::UiStrings;

/// Independent state for the AI Provider Center panel.
#[derive(Clone, Default, PartialEq)]
pub struct AiProviderPanelState {
    pub profiles: Option<Vec<AiProviderProfileDto>>,
    pub list_error: Option<String>,
    pub testing_id: Option<String>,
    pub test_error: Option<String>,
}

impl AiProviderPanelState {
    pub fn list_loaded(&mut self, list: AiProviderProfileListDto) {
        self.profiles = Some(list.profiles);
        self.list_error = None;
    }

    pub fn list_failed(&mut self, error: String) {
        self.list_error = Some(error);
    }

    pub fn test_started(&mut self, id: String) {
        self.testing_id = Some(id);
        self.test_error = None;
    }

    pub fn test_succeeded(&mut self, profile: AiProviderProfileDto) {
        if let Some(profiles) = self.profiles.as_mut() {
            if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
                *existing = profile;
            } else {
                profiles.push(profile);
            }
        }
        self.testing_id = None;
        self.test_error = None;
    }

    pub fn test_failed(&mut self, _id: &str, error: String) {
        self.testing_id = None;
        self.test_error = Some(error);
    }
}

/// Renders the AI provider profile registry and bounded health checks.
#[component]
pub fn AiProviderPanel(
    state: AiProviderPanelState,
    strings: UiStrings,
    on_test: EventHandler<String>,
) -> Element {
    let profile_count = state.profiles.as_ref().map_or(0, Vec::len);
    let healthy_count = state.profiles.as_ref().map_or(0, |profiles| {
        profiles
            .iter()
            .filter(|profile| profile.health_state == "HEALTHY")
            .count()
    });

    rsx! {
        section { class: "panel ai-provider-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.ai_providers}" }
                    p { class: "panel-subtitle", "{strings.ai_providers_hint}" }
                }
                div { class: "signal-summary", aria_label: "{strings.ai_provider_summary}",
                    span { strong { "{profile_count}" } " {strings.ai_provider_profiles_count}" }
                    span { strong { "{healthy_count}" } " {strings.ai_provider_healthy_count}" }
                }
            }
            if let Some(error) = state.list_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            if let Some(error) = state.test_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.profiles.as_ref() {
                None if state.list_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_provider_unavailable}" }
                        p { "{strings.ai_provider_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.ai_provider_loading}" }
                        p { "{strings.ai_provider_loading_hint}" }
                    }
                },
                Some(profiles) if profiles.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_provider_empty}" }
                        p { "{strings.ai_provider_empty_hint}" }
                    }
                },
                Some(profiles) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services ai-provider-table",
                            thead {
                                tr {
                                    th { "{strings.ai_provider_name_column}" }
                                    th { "{strings.ai_provider_kind_column}" }
                                    th { "{strings.ai_provider_model_column}" }
                                    th { "{strings.ai_provider_health_column}" }
                                    th { "{strings.ai_provider_last_check_column}" }
                                    th { "{strings.ai_provider_error_column}" }
                                    th { "{strings.ai_provider_actions_column}" }
                                }
                            }
                            tbody {
                                for profile in profiles.iter() {
                                    {
                                        let profile_id = profile.id.clone();
                                        let testing = state.testing_id.as_deref() == Some(profile.id.as_str());
                                        let test_sender = on_test;
                                        rsx! {
                                            tr { class: "signal-row",
                                                td {
                                                    div { class: "service-name", "{profile.display_name}" }
                                                    div { class: "service-instance", "{profile.id}" }
                                                }
                                                td { span { class: "timeframe-chip", "{profile.kind}" } }
                                                td { class: "signal-source", "{profile.default_model}" }
                                                td {
                                                    span { class: "pill {health_class(&profile.health_state)}",
                                                        "{health_label(&profile.health_state, &strings)}"
                                                    }
                                                }
                                                td { class: "signal-time", "{format_optional_timestamp(profile.last_health_check_at)}" }
                                                td { class: "signal-source-method",
                                                    "{profile.last_error_code.clone().unwrap_or_else(|| strings.ai_provider_no_error.to_owned())}"
                                                }
                                                td { class: "actions",
                                                    button {
                                                        class: "button primary",
                                                        disabled: testing || !profile.enabled,
                                                        onclick: move |_| test_sender.call(profile_id.clone()),
                                                        if testing { "{strings.ai_provider_testing}" } else { "{strings.ai_provider_test}" }
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
}

fn health_label(state: &str, strings: &UiStrings) -> &'static str {
    match state {
        "HEALTHY" => strings.ai_provider_health_healthy,
        "UNHEALTHY" => strings.ai_provider_health_unhealthy,
        _ => strings.ai_provider_health_unknown,
    }
}

fn health_class(state: &str) -> &'static str {
    match state {
        "HEALTHY" => "state-ok",
        "UNHEALTHY" => "state-failed",
        _ => "state-unknown",
    }
}

fn format_optional_timestamp(value: Option<DateTime<Utc>>) -> String {
    value.map_or_else(
        || "-".to_owned(),
        |value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn profile(id: &str, health: &str) -> AiProviderProfileDto {
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        AiProviderProfileDto {
            id: id.to_owned(),
            kind: "openai".to_owned(),
            display_name: "Primary".to_owned(),
            enabled: true,
            base_url: "https://api.example.com/v1".to_owned(),
            default_model: "model-a".to_owned(),
            timeout_ms: 5_000,
            api_key_env: "AI_PRIMARY_KEY".to_owned(),
            health_state: health.to_owned(),
            last_health_check_at: Some(now),
            last_error_code: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_health_check_replaces_the_matching_profile() {
        let mut state = AiProviderPanelState::default();
        state.list_loaded(AiProviderProfileListDto {
            generated_at: Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap(),
            count: 1,
            profiles: vec![profile("primary", "UNKNOWN")],
        });
        state.test_started("primary".to_owned());
        state.test_succeeded(profile("primary", "HEALTHY"));
        assert!(state.testing_id.is_none());
        assert_eq!(state.profiles.as_ref().unwrap()[0].health_state, "HEALTHY");
        assert_eq!(state.profiles.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_test_keeps_the_existing_profiles() {
        let mut state = AiProviderPanelState::default();
        state.list_failed("down".to_owned());
        state.test_started("primary".to_owned());
        state.test_failed("primary", "boom".to_owned());
        assert!(state.testing_id.is_none());
        assert_eq!(state.test_error.as_deref(), Some("boom"));
        assert!(state.profiles.is_none());
    }
}
