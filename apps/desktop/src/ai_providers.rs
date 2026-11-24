use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{
    AiModelDto, AiModelListDto, AiProviderProfileDto, AiProviderProfileListDto, AiTaskRouteDto,
    AiTaskRouteListDto, NewAiModelDto, NewAiProviderProfileDto, NewAiTaskRouteDto,
};

use crate::i18n::UiStrings;
use crate::modal::Modal;

/// Independent state for the AI Provider Center panel.
#[derive(Clone, Default, PartialEq)]
pub struct AiProviderPanelState {
    pub profiles: Option<Vec<AiProviderProfileDto>>,
    pub list_error: Option<String>,
    pub testing_id: Option<String>,
    pub test_error: Option<String>,
    pub models: Option<Vec<AiModelDto>>,
    pub models_error: Option<String>,
    pub testing_model_id: Option<String>,
    pub model_test_error: Option<String>,
    pub routes: Option<Vec<AiTaskRouteDto>>,
    pub routes_error: Option<String>,
}

impl AiProviderPanelState {
    pub fn routes_loaded(&mut self, list: AiTaskRouteListDto) {
        self.routes = Some(list.routes);
        self.routes_error = None;
    }

    pub fn routes_failed(&mut self, error: String) {
        self.routes_error = Some(error);
    }

    pub fn models_loaded(&mut self, list: AiModelListDto) {
        self.models = Some(list.models);
        self.models_error = None;
    }

    pub fn models_failed(&mut self, error: String) {
        self.models_error = Some(error);
    }

    pub fn model_test_started(&mut self, id: String) {
        self.testing_model_id = Some(id);
        self.model_test_error = None;
    }

    pub fn model_test_succeeded(&mut self, model: AiModelDto) {
        if let Some(models) = self.models.as_mut() {
            if let Some(existing) = models.iter_mut().find(|item| item.id == model.id) {
                *existing = model;
            } else {
                models.push(model);
            }
        }
        self.testing_model_id = None;
        self.model_test_error = None;
    }

    pub fn model_test_failed(&mut self, _id: &str, error: String) {
        self.testing_model_id = None;
        self.model_test_error = Some(error);
    }

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
    on_test_model: EventHandler<String>,
    #[props(default)] on_create_provider: Option<EventHandler<NewAiProviderProfileDto>>,
    #[props(default)] on_delete_provider: Option<EventHandler<String>>,
    #[props(default)] on_create_model: Option<EventHandler<(String, NewAiModelDto)>>,
    #[props(default)] on_delete_model: Option<EventHandler<String>>,
    #[props(default)] on_create_route: Option<EventHandler<NewAiTaskRouteDto>>,
    #[props(default)] on_delete_route: Option<EventHandler<String>>,
) -> Element {
    let mut show_add_provider = use_signal(|| false);
    let mut new_provider_id = use_signal(String::new);
    let mut new_provider_kind = use_signal(|| "gemini".to_owned());
    let mut new_provider_display = use_signal(String::new);
    let mut new_provider_base_url = use_signal(|| "https://generativelanguage.googleapis.com/v1beta".to_owned());
    let mut new_provider_default_model = use_signal(|| "gemini-2.5-flash".to_owned());
    let new_provider_timeout = use_signal(|| 30000u64);
    let mut new_provider_api_key_env = use_signal(|| "GEMINI_API_KEY".to_owned());

    let mut show_add_model = use_signal(|| false);
    let mut new_model_provider = use_signal(String::new);
    let mut new_model_id = use_signal(String::new);
    let mut new_model_name = use_signal(String::new);
    let mut new_model_display = use_signal(String::new);
    let new_model_context = use_signal(|| Option::<u32>::Some(1_048_576));
    let new_model_max_output = use_signal(|| Option::<u32>::Some(8_192));
    let mut new_model_streaming = use_signal(|| true);
    let mut new_model_json = use_signal(|| true);

    let mut show_add_route = use_signal(|| false);
    let mut new_route_task = use_signal(|| "research_explanation".to_owned());
    let mut new_route_primary = use_signal(String::new);
    let mut new_route_fallbacks = use_signal(String::new);

    let profile_count = state.profiles.as_ref().map_or(0, Vec::len);
    let healthy_count = state.profiles.as_ref().map_or(0, |profiles| {
        profiles
            .iter()
            .filter(|profile| profile.health_state == "HEALTHY")
            .count()
    });
    let providers_list = state.profiles.as_ref().cloned().unwrap_or_default();
    let registered_models = state.models.as_ref().cloned().unwrap_or_default();

    rsx! {
        section { class: "panel ai-provider-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.ai_providers}" }
                    p { class: "panel-subtitle", "{strings.ai_providers_hint}" }
                }
                div { class: "topbar-actions",
                    div { class: "signal-summary", aria_label: "{strings.ai_provider_summary}",
                        span { strong { "{profile_count}" } " {strings.ai_provider_profiles_count}" }
                        span { strong { "{healthy_count}" } " {strings.ai_provider_healthy_count}" }
                    }
                    if on_create_provider.is_some() {
                        button {
                            class: "button primary",
                            onclick: move |_| show_add_provider.set(true),
                            "+ {strings.ai_provider_add}"
                        }
                    }
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
                                        let del_id = profile.id.clone();
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
                                                    if let Some(on_delete) = on_delete_provider {
                                                        button {
                                                            class: "button danger",
                                                            onclick: move |_| on_delete.call(del_id.clone()),
                                                            "{strings.ai_provider_delete}"
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
            div { class: "panel-head signal-center-head",
                div {
                    h3 { "{strings.ai_models}" }
                    p { class: "panel-subtitle", "{strings.ai_models_hint}" }
                }
                if on_create_model.is_some() {
                    button {
                        class: "button primary",
                        onclick: move |_| show_add_model.set(true),
                        "+ {strings.ai_model_add}"
                    }
                }
            }
            if let Some(error) = state.models_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            if let Some(error) = state.model_test_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.models.as_ref() {
                None if state.models_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_provider_unavailable}" }
                        p { "{strings.ai_models_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.ai_provider_loading}" }
                        p { "{strings.ai_models_loading_hint}" }
                    }
                },
                Some(models) if models.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_models_empty}" }
                        p { "{strings.ai_models_empty_hint}" }
                    }
                },
                Some(models) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services ai-provider-table",
                            thead {
                                tr {
                                    th { "{strings.ai_model_name_column}" }
                                    th { "{strings.ai_model_provider_column}" }
                                    th { "{strings.ai_model_id_column}" }
                                    th { "{strings.ai_model_context_column}" }
                                    th { "{strings.ai_model_output_column}" }
                                    th { "{strings.ai_model_enabled_column}" }
                                    th { "{strings.ai_provider_health_column}" }
                                    th { "{strings.ai_provider_actions_column}" }
                                }
                            }
                            tbody {
                                for model in models.iter() {
                                    {
                                        let model_id = model.id.clone();
                                        let del_id = model.id.clone();
                                        let testing = state.testing_model_id.as_deref() == Some(model.id.as_str());
                                        let test_sender = on_test_model;
                                        let disabled = testing
                                            || !model.enabled
                                            || !state.profiles.as_ref().is_some_and(|profiles| {
                                                profiles.iter().any(|profile| {
                                                    profile.id == model.provider_id
                                                        && profile.enabled
                                                })
                                            });
                                        rsx! {
                                            tr { class: "signal-row",
                                                td {
                                                    div { class: "service-name", "{model.display_name}" }
                                                    div { class: "service-instance", "{model.id}" }
                                                }
                                                td { class: "signal-source", "{model.provider_id}" }
                                                td { span { class: "timeframe-chip", "{model.model_id}" } }
                                                td { class: "signal-time", "{format_tokens(model.context_window)}" }
                                                td { class: "signal-time", "{format_tokens(model.max_output_tokens)}" }
                                                td {
                                                    if model.enabled { "{strings.ai_model_enabled}" } else { "{strings.ai_model_disabled}" }
                                                }
                                                td {
                                                    span { class: "pill {health_class(&model.health_state)}",
                                                        "{health_label(&model.health_state, &strings)}"
                                                    }
                                                }
                                                td { class: "actions",
                                                    button {
                                                        class: "button primary",
                                                        disabled,
                                                        onclick: move |_| test_sender.call(model_id.clone()),
                                                        if testing { "{strings.ai_provider_testing}" } else { "{strings.ai_provider_test}" }
                                                    }
                                                    if let Some(on_delete) = on_delete_model {
                                                        button {
                                                            class: "button danger",
                                                            onclick: move |_| on_delete.call(del_id.clone()),
                                                            "{strings.ai_model_delete}"
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
            div { class: "panel-head signal-center-head",
                div {
                    h3 { "{strings.ai_task_routes}" }
                    p { class: "panel-subtitle", "{strings.ai_task_routes_hint}" }
                }
                if on_create_route.is_some() {
                    button {
                        class: "button primary",
                        onclick: move |_| show_add_route.set(true),
                        "+ {strings.ai_task_route_add}"
                    }
                }
            }
            if let Some(error) = state.routes_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            match state.routes.as_ref() {
                None if state.routes_error.is_some() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_provider_unavailable}" }
                        p { "{strings.ai_task_routes_unavailable_hint}" }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.ai_provider_loading}" }
                        p { "{strings.ai_task_routes_loading_hint}" }
                    }
                },
                Some(routes) if routes.is_empty() => rsx! {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.ai_task_routes_empty}" }
                        p { "{strings.ai_task_routes_empty_hint}" }
                    }
                },
                Some(routes) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services ai-provider-table",
                            thead {
                                tr {
                                    th { "{strings.ai_task_route_task_column}" }
                                    th { "{strings.ai_task_route_primary_column}" }
                                    th { "{strings.ai_task_route_fallback_column}" }
                                    th { "{strings.ai_task_route_attempts_column}" }
                                    th { "{strings.ai_task_route_state_column}" }
                                    th { "{strings.ai_provider_actions_column}" }
                                }
                            }
                            tbody {
                                for route in routes.iter() {
                                    {
                                        let del_task = route.task.clone();
                                        rsx! {
                                            tr { class: "signal-row",
                                                td { span { class: "timeframe-chip", "{route.task}" } }
                                                td { class: "signal-source", "{route.primary}" }
                                                td { class: "signal-source",
                                                    "{route.fallbacks.join(\", \")}"
                                                }
                                                td { class: "signal-time", "{route.providers_in_reach}" }
                                                td {
                                                    if route.enabled { "{strings.ai_model_enabled}" } else { "{strings.ai_model_disabled}" }
                                                }
                                                td { class: "actions",
                                                    if let Some(on_delete) = on_delete_route {
                                                        button {
                                                            class: "button danger",
                                                            onclick: move |_| on_delete.call(del_task.clone()),
                                                            "{strings.ai_task_route_delete}"
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
        if show_add_provider() {
            Modal {
                title: strings.ai_provider_add.to_string(),
                close_label: strings.modal_close.to_string(),
                on_close: move |_| show_add_provider.set(false),
                footer: rsx! {
                    button {
                        class: "button",
                        onclick: move |_| show_add_provider.set(false),
                        "{strings.modal_close}"
                    }
                    button {
                        class: "button primary",
                        onclick: move |_| {
                            let id = new_provider_id().trim().to_owned();
                            if id.is_empty() { return; }
                            if let Some(on_create) = on_create_provider {
                                on_create.call(NewAiProviderProfileDto {
                                    id,
                                    kind: new_provider_kind(),
                                    display_name: new_provider_display().trim().to_owned(),
                                    base_url: new_provider_base_url().trim().to_owned(),
                                    default_model: new_provider_default_model().trim().to_owned(),
                                    timeout_ms: new_provider_timeout(),
                                    api_key_env: new_provider_api_key_env().trim().to_owned(),
                                });
                            }
                            show_add_provider.set(false);
                        },
                        "{strings.ai_provider_add}"
                    }
                },
                div { class: "form-group",
                    label { "ID" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. gemini, openai, anthropic",
                        value: "{new_provider_id()}",
                        oninput: move |e| new_provider_id.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Kind" }
                    select {
                        class: "select-input",
                        value: "{new_provider_kind()}",
                        onchange: move |e| new_provider_kind.set(e.value()),
                        option { value: "gemini", "Gemini" }
                        option { value: "openai", "OpenAI" }
                        option { value: "anthropic", "Anthropic" }
                        option { value: "custom", "Custom" }
                    }
                }
                div { class: "form-group",
                    label { "Display Name" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. Google Gemini 2.5",
                        value: "{new_provider_display()}",
                        oninput: move |e| new_provider_display.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Base URL" }
                    input {
                        class: "text-input",
                        value: "{new_provider_base_url()}",
                        oninput: move |e| new_provider_base_url.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Default Model" }
                    input {
                        class: "text-input",
                        value: "{new_provider_default_model()}",
                        oninput: move |e| new_provider_default_model.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "API Key Env Var" }
                    input {
                        class: "text-input",
                        value: "{new_provider_api_key_env()}",
                        oninput: move |e| new_provider_api_key_env.set(e.value()),
                    }
                }
            }
        }
        if show_add_model() {
            Modal {
                title: strings.ai_model_add.to_string(),
                close_label: strings.modal_close.to_string(),
                on_close: move |_| show_add_model.set(false),
                footer: rsx! {
                    button {
                        class: "button",
                        onclick: move |_| show_add_model.set(false),
                        "{strings.modal_close}"
                    }
                    button {
                        class: "button primary",
                        onclick: move |_| {
                            let prov_id = new_model_provider().trim().to_owned();
                            let id = new_model_id().trim().to_owned();
                            let model_name = new_model_name().trim().to_owned();
                            if prov_id.is_empty() || id.is_empty() || model_name.is_empty() { return; }
                            if let Some(on_create) = on_create_model {
                                on_create.call((
                                    prov_id,
                                    NewAiModelDto {
                                        id,
                                        model_id: model_name,
                                        display_name: new_model_display().trim().to_owned(),
                                        context_window: new_model_context(),
                                        max_output_tokens: new_model_max_output(),
                                        supports_streaming: new_model_streaming(),
                                        supports_json_mode: new_model_json(),
                                        supports_tools: false,
                                        supports_vision: false,
                                    },
                                ));
                            }
                            show_add_model.set(false);
                        },
                        "{strings.ai_model_add}"
                    }
                },
                div { class: "form-group",
                    label { "Provider ID" }
                    select {
                        class: "select-input",
                        value: "{new_model_provider()}",
                        onchange: move |e| new_model_provider.set(e.value()),
                        option { value: "", "Select Provider..." }
                        for p in providers_list {
                            option { value: "{p.id}", "{p.display_name} ({p.id})" }
                        }
                    }
                }
                div { class: "form-group",
                    label { "Registered ID" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. gemini-flash",
                        value: "{new_model_id()}",
                        oninput: move |e| new_model_id.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Model ID" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. gemini-2.5-flash",
                        value: "{new_model_name()}",
                        oninput: move |e| new_model_name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Display Name" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. Gemini 2.5 Flash",
                        value: "{new_model_display()}",
                        oninput: move |e| new_model_display.set(e.value()),
                    }
                }
                label { class: "form-checkbox",
                    input {
                        r#type: "checkbox",
                        checked: "{new_model_streaming()}",
                        onchange: move |e| new_model_streaming.set(e.checked()),
                    }
                    "Supports Streaming"
                }
                label { class: "form-checkbox",
                    input {
                        r#type: "checkbox",
                        checked: "{new_model_json()}",
                        onchange: move |e| new_model_json.set(e.checked()),
                    }
                    "Supports JSON mode"
                }
            }
        }
        if show_add_route() {
            Modal {
                title: strings.ai_task_route_add.to_string(),
                close_label: strings.modal_close.to_string(),
                on_close: move |_| show_add_route.set(false),
                footer: rsx! {
                    button {
                        class: "button",
                        onclick: move |_| show_add_route.set(false),
                        "{strings.modal_close}"
                    }
                    button {
                        class: "button primary",
                        onclick: move |_| {
                            let task = new_route_task().trim().to_owned();
                            let primary = new_route_primary().trim().to_owned();
                            if task.is_empty() || primary.is_empty() { return; }
                            let fallbacks = new_route_fallbacks()
                                .split(',')
                                .map(|s| s.trim().to_owned())
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>();
                            if let Some(on_create) = on_create_route {
                                on_create.call(NewAiTaskRouteDto {
                                    task,
                                    primary,
                                    fallbacks,
                                    timeout_ms: 30000,
                                    max_attempts: 3,
                                    concurrency: 2,
                                    max_output_tokens: None,
                                });
                            }
                            show_add_route.set(false);
                        },
                        "{strings.ai_task_route_add}"
                    }
                },
                div { class: "form-group",
                    label { "Task Name" }
                    select {
                        class: "select-input",
                        value: "{new_route_task()}",
                        onchange: move |e| new_route_task.set(e.value()),
                        option { value: "research_explanation", "research_explanation" }
                        option { value: "briefing_synthesis", "briefing_synthesis" }
                        option { value: "telegram_alert_summary", "telegram_alert_summary" }
                        option { value: "market_briefing", "market_briefing" }
                        option { value: "news_classification", "news_classification" }
                        option { value: "news_summary", "news_summary" }
                        option { value: "daily_digest", "daily_digest" }
                    }
                }
                div { class: "form-group",
                    label { "Primary Model (provider:model)" }
                    select {
                        class: "select-input",
                        value: "{new_route_primary()}",
                        onchange: move |e| new_route_primary.set(e.value()),
                        option { value: "", "Select Primary Model..." }
                        for m in registered_models {
                            option { value: "{m.provider_id}:{m.id}", "{m.provider_id}:{m.id} ({m.display_name})" }
                        }
                    }
                }
                div { class: "form-group",
                    label { "Fallbacks (comma separated provider:model)" }
                    input {
                        class: "text-input",
                        placeholder: "e.g. openai:gpt-4o-mini",
                        value: "{new_route_fallbacks()}",
                        oninput: move |e| new_route_fallbacks.set(e.value()),
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

fn format_tokens(value: Option<u32>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
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
            api_key_env: "TM_TRADING_AI_PRIMARY_KEY".to_owned(),
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

    #[test]
    fn a_model_list_failure_keeps_the_provider_table() {
        let mut state = AiProviderPanelState::default();
        state.list_loaded(AiProviderProfileListDto {
            generated_at: Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap(),
            count: 1,
            profiles: vec![profile("primary", "HEALTHY")],
        });
        state.models_failed("down".to_owned());
        assert!(state.models.is_none());
        assert_eq!(state.models_error.as_deref(), Some("down"));
        assert_eq!(state.profiles.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn loaded_models_clear_the_previous_error() {
        let mut state = AiProviderPanelState::default();
        state.models_failed("down".to_owned());
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        state.models_loaded(AiModelListDto {
            generated_at: now,
            count: 1,
            models: vec![AiModelDto {
                id: "primary-large".to_owned(),
                provider_id: "primary".to_owned(),
                model_id: "model-a".to_owned(),
                display_name: "Primary Large".to_owned(),
                context_window: Some(128_000),
                max_output_tokens: Some(8_192),
                supports_streaming: true,
                supports_json_mode: true,
                supports_tools: false,
                supports_vision: false,
                enabled: true,
                health_state: "UNKNOWN".to_owned(),
                last_health_check_at: None,
                last_error_code: None,
                created_at: now,
                updated_at: now,
            }],
        });
        assert!(state.models_error.is_none());
        assert_eq!(state.models.as_ref().unwrap()[0].model_id, "model-a");
    }

    #[test]
    fn a_model_test_replaces_only_the_tested_model() {
        let mut state = AiProviderPanelState::default();
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        let mut other = model("secondary-small", now);
        other.provider_id = "secondary".to_owned();
        state.models_loaded(AiModelListDto {
            generated_at: now,
            count: 2,
            models: vec![model("primary-large", now), other],
        });
        state.model_test_started("primary-large".to_owned());
        state.model_test_succeeded(model_with_health("primary-large", now, "UNHEALTHY"));
        let models = state.models.as_ref().unwrap();
        assert!(state.testing_model_id.is_none());
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].health_state, "UNHEALTHY");
        assert_eq!(models[1].provider_id, "secondary");
        assert_eq!(models[1].health_state, "UNKNOWN");
    }

    #[test]
    fn a_failed_model_test_keeps_the_models_and_reports_the_error() {
        let mut state = AiProviderPanelState::default();
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        state.models_loaded(AiModelListDto {
            generated_at: now,
            count: 1,
            models: vec![model("primary-large", now)],
        });
        state.model_test_started("primary-large".to_owned());
        state.model_test_failed("primary-large", "down".to_owned());
        assert!(state.testing_model_id.is_none());
        assert_eq!(state.model_test_error.as_deref(), Some("down"));
        assert_eq!(
            state.models.as_ref().unwrap()[0].health_state,
            "UNKNOWN".to_owned()
        );
    }

    fn model(id: &str, now: DateTime<Utc>) -> AiModelDto {
        model_with_health(id, now, "UNKNOWN")
    }

    #[test]
    fn a_route_list_failure_keeps_the_models_and_routes() {
        let mut state = AiProviderPanelState::default();
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        state.models_loaded(AiModelListDto {
            generated_at: now,
            count: 1,
            models: vec![model("primary-large", now)],
        });
        state.routes_failed("down".to_owned());
        assert!(state.routes.is_none());
        assert_eq!(state.routes_error.as_deref(), Some("down"));
        assert_eq!(state.models.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn loaded_routes_clear_the_previous_error() {
        let mut state = AiProviderPanelState::default();
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap();
        state.routes_failed("down".to_owned());
        state.routes_loaded(AiTaskRouteListDto {
            generated_at: now,
            count: 1,
            routes: vec![AiTaskRouteDto {
                task: "market_briefing".to_owned(),
                primary: "primary:primary-large".to_owned(),
                fallbacks: vec!["primary:secondary-small".to_owned()],
                enabled: true,
                timeout_ms: 5_000,
                max_attempts: 2,
                concurrency: 1,
                max_output_tokens: Some(1_024),
                providers_in_reach: 2,
                created_at: now,
                updated_at: now,
            }],
        });
        assert!(state.routes_error.is_none());
        assert_eq!(state.routes.as_ref().unwrap()[0].providers_in_reach, 2usize);
    }

    fn model_with_health(id: &str, now: DateTime<Utc>, health: &str) -> AiModelDto {
        AiModelDto {
            id: id.to_owned(),
            provider_id: "primary".to_owned(),
            model_id: "model-a".to_owned(),
            display_name: "Primary Large".to_owned(),
            context_window: Some(128_000),
            max_output_tokens: Some(8_192),
            supports_streaming: true,
            supports_json_mode: true,
            supports_tools: false,
            supports_vision: false,
            enabled: true,
            health_state: health.to_owned(),
            last_health_check_at: None,
            last_error_code: None,
            created_at: now,
            updated_at: now,
        }
    }
}
