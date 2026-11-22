// A release build is a Windows GUI application: opening it must not allocate a console
// window. Debug builds keep the console so startup logs stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai_briefings;
mod ai_providers;
mod i18n;
mod news;
mod signal_center;
mod slideover;

use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use futures_util::StreamExt;
use tm_trading_api_client::{ManagementApiClient, new_idempotency_key};
use tm_trading_contracts::{
    AiBriefingQueryDto, AiBriefingTaskDto, AiJobQueryDto, MarketMoverDto, MarketOverviewDto,
    MarketPulseDto, MarketStateDto, MarketSymbolDto, NewAiJobDto, NewsQueryDto, OperationActionDto,
    OperationResponse, OperationStatusDto, ResearchEventQueryDto, ServiceSnapshot, WatchlistDto,
};

use ai_briefings::{AiBriefingPanel, AiBriefingPanelState, AiBriefingSlideover};
use ai_providers::{AiProviderPanel, AiProviderPanelState};
use i18n::{Locale, UiStrings};
use news::{NewsItemSlideover, NewsPanel, NewsPanelState};
use signal_center::{ResearchEventSlideover, ResearchPanelState, SignalCenterPanel};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_API_URL: &str = "http://127.0.0.1:8787";

fn main() {
    let log_format =
        tm_trading_config::LogFormat::from_env().unwrap_or(tm_trading_config::LogFormat::Compact);
    let _ = tm_trading_observability::init(log_format);

    let config = dioxus::desktop::Config::default()
        .with_data_directory(data_directory())
        .with_window(
            dioxus::desktop::WindowBuilder::new()
                .with_title("TM Trading Control Center")
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 820.0)),
        );
    dioxus::LaunchBuilder::new().with_cfg(config).launch(App);
}

/// Resolves the WebView2 profile directory.
///
/// The default sits next to the executable, like the Agent state directory, so the
/// desktop app does not depend on a writable user profile folder.
fn data_directory() -> PathBuf {
    if let Ok(directory) = std::env::var("TM_TRADING_DESKTOP_DATA_DIR") {
        return PathBuf::from(directory);
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .map(|directory| directory.join("data").join("desktop"))
        .unwrap_or_else(|| PathBuf::from("data").join("desktop"))
}

/// Renders the stylesheet once; the document head cannot be updated afterwards.
#[component]
fn AppStyles() -> Element {
    rsx! {
        document::Style {
            { include_str!("../assets/main.css") }
        }
    }
}

/// Work requested by the user interface.
#[derive(Clone)]
enum UiAction {
    Command {
        service_instance_id: String,
        action: OperationActionDto,
    },
    Watchlist(WatchlistAction),
    OpenResearchEvent {
        event_id: String,
    },
    CloseResearchEvent,
    OpenNewsItem {
        news_id: String,
    },
    CloseNewsItem,
    OpenAiBriefing {
        briefing_id: String,
    },
    CloseAiBriefing,
    RequestAiBriefing {
        research_event_id: String,
    },
    TestAiProvider {
        id: String,
    },
    TestAiModel {
        id: String,
    },
}

#[derive(Clone)]
enum WatchlistAction {
    Create {
        name: String,
        symbols: Vec<String>,
    },
    ReplaceSymbols {
        watchlist_id: String,
        symbols: Vec<String>,
    },
    Delete {
        watchlist_id: String,
    },
}

#[derive(Clone, PartialEq)]
enum ConnectionState {
    Connecting,
    Connected,
    Disconnected(String),
}

#[component]
fn App() -> Element {
    let client = use_hook(|| {
        let base_url =
            std::env::var("TM_TRADING_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
        let token = std::env::var("TM_TRADING_LOCAL_API_TOKEN").unwrap_or_default();
        match ManagementApiClient::new(&base_url, &token) {
            Ok(client) => Some(client),
            Err(error) => {
                tracing::error!(%error, "management API client could not be created");
                None
            }
        }
    });

    let mut locale = use_signal(Locale::from_env);
    let strings = use_memo(move || UiStrings::for_locale(locale()));
    let services = use_signal(Vec::<ServiceSnapshot>::new);
    let connection = use_signal(|| ConnectionState::Connecting);
    let operation = use_signal(|| Option::<OperationResponse>::None);
    let command_error = use_signal(|| Option::<String>::None);
    let refreshed_at = use_signal(|| Option::<String>::None);
    let pending_operation = use_signal(|| Option::<String>::None);
    let watchlists = use_signal(Vec::<WatchlistDto>::new);
    let mut new_list_name = use_signal(String::new);
    let watchlist_error = use_signal(|| Option::<String>::None);
    let market_overview = use_signal(|| Option::<MarketOverviewDto>::None);
    let market_pulse = use_signal(|| Option::<MarketPulseDto>::None);
    let market_error = use_signal(|| Option::<String>::None);
    let research = use_signal(ResearchPanelState::default);
    let news = use_signal(NewsPanelState::default);
    let ai_briefings = use_signal(AiBriefingPanelState::default);
    let ai_providers = use_signal(AiProviderPanelState::default);

    let client_task = client.clone();
    let actions = use_coroutine(move |mut actions: UnboundedReceiver<UiAction>| {
        let client = client_task.clone();
        // Signals are Copy handles; shadowing keeps the coroutine body reading naturally.
        let (mut services, mut connection) = (services, connection);
        let (mut operation, mut command_error) = (operation, command_error);
        let (mut refreshed_at, mut pending_operation) = (refreshed_at, pending_operation);
        let (mut watchlists, mut watchlist_error) = (watchlists, watchlist_error);
        let (mut market_overview, mut market_pulse, mut market_error) =
            (market_overview, market_pulse, market_error);
        let mut research = research;
        let mut news = news;
        let mut ai_briefings = ai_briefings;
        let mut ai_providers = ai_providers;
        async move {
            loop {
                tokio::select! {
                    biased;
                    action = actions.next() => {
                        match action {
                            Some(UiAction::Command { service_instance_id, action }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                let key = new_idempotency_key();
                                match client.create_operation(&service_instance_id, action, &key).await {
                                    Ok(created) => {
                                        command_error.set(None);
                                        operation.set(Some(created.clone()));
                                        pending_operation.set(Some(created.operation_id.clone()));
                                    }
                                    Err(error) => command_error.set(Some(error.to_string())),
                                }
                            }
                            Some(UiAction::Watchlist(action)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                let result = match action {
                                    WatchlistAction::Create { name, symbols } => client
                                        .create_watchlist(&name, &symbols)
                                        .await
                                        .map(|_| ()),
                                    WatchlistAction::ReplaceSymbols {
                                        watchlist_id,
                                        symbols,
                                    } => client
                                        .replace_watchlist_symbols(&watchlist_id, &symbols)
                                        .await
                                        .map(|_| ()),
                                    WatchlistAction::Delete { watchlist_id } => {
                                        client.delete_watchlist(&watchlist_id).await
                                    }
                                };
                                match result {
                                    Ok(()) => {
                                        watchlist_error.set(None);
                                        if let Ok(list) = client.list_watchlists().await {
                                            watchlists.set(list);
                                        }
                                    }
                                    Err(error) => watchlist_error.set(Some(error.to_string())),
                                }
                            }
                            Some(UiAction::OpenResearchEvent { event_id }) => {
                                news.write().close();
                                ai_briefings.write().close();
                                research.write().open(event_id);
                            }
                            Some(UiAction::CloseResearchEvent) => {
                                research.write().close();
                            }
                            Some(UiAction::OpenNewsItem { news_id }) => {
                                research.write().close();
                                ai_briefings.write().close();
                                news.write().open(news_id);
                            }
                            Some(UiAction::CloseNewsItem) => {
                                news.write().close();
                            }
                            Some(UiAction::OpenAiBriefing { briefing_id }) => {
                                research.write().close();
                                news.write().close();
                                ai_briefings.write().open(briefing_id);
                            }
                            Some(UiAction::CloseAiBriefing) => {
                                ai_briefings.write().close();
                            }
                            Some(UiAction::RequestAiBriefing { research_event_id }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                let request = NewAiJobDto {
                                    task: AiBriefingTaskDto::ResearchExplanation,
                                    research_event_ids: vec![research_event_id],
                                    news_item_ids: Vec::new(),
                                    idempotency_key: None,
                                };
                                match client.create_ai_job(&request).await {
                                    Ok(job) => ai_briefings.write().request_created(job),
                                    Err(error) => {
                                        tracing::error!(%error, "failed to queue AI briefing request");
                                        ai_briefings.write().request_failed();
                                    }
                                }
                            }
                            Some(UiAction::TestAiProvider { id }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                ai_providers.write().test_started(id.clone());
                                match client.test_ai_provider_profile(&id).await {
                                    Ok(profile) => ai_providers.write().test_succeeded(profile),
                                    Err(error) => {
                                        ai_providers.write().test_failed(&id, error.to_string())
                                    }
                                }
                            }
                            Some(UiAction::TestAiModel { id }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                ai_providers.write().model_test_started(id.clone());
                                match client.test_ai_model(&id).await {
                                    Ok(model) => ai_providers.write().model_test_succeeded(model),
                                    Err(error) => {
                                        ai_providers.write().model_test_failed(&id, error.to_string())
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                    () = tokio::time::sleep(REFRESH_INTERVAL) => {}
                }

                let Some(client) = client.as_ref() else {
                    break;
                };
                match client.list_services().await {
                    Ok(list) => {
                        services.set(list);
                        connection.set(ConnectionState::Connected);
                    }
                    Err(error) => connection.set(ConnectionState::Disconnected(error.to_string())),
                }
                refreshed_at.set(Some(chrono::Local::now().format("%H:%M:%S").to_string()));

                if let Ok(list) = client.list_watchlists().await {
                    watchlists.set(list);
                }

                // An empty symbol list means "use my default watchlist" on the server.
                match client.market_overview(&[], "1m", 60).await {
                    Ok(overview) => {
                        market_overview.set(Some(overview));
                        market_error.set(None);
                    }
                    Err(error) => market_error.set(Some(error.to_string())),
                }
                if let Ok(pulse) = client.market_pulse(&[], "1m", 60).await {
                    market_pulse.set(Some(pulse));
                }

                let research_query = ResearchEventQueryDto {
                    limit: Some(50),
                    ..ResearchEventQueryDto::default()
                };
                match client.list_research_events(&research_query).await {
                    Ok(list) => research.write().list_loaded(list.events),
                    Err(error) => research.write().list_failed(error.to_string()),
                }
                let selected_research_id = research.read().selected_id.clone();
                if let Some(event_id) = selected_research_id {
                    match client.get_research_event(&event_id).await {
                        Ok(event) => research.write().detail_loaded(event),
                        Err(error) => research.write().detail_failed(&event_id, error.to_string()),
                    }
                }

                let news_query = NewsQueryDto {
                    limit: Some(50),
                    ..NewsQueryDto::default()
                };
                match client.list_news(&news_query).await {
                    Ok(list) => news.write().list_loaded(list),
                    Err(error) => news.write().list_failed(error.to_string()),
                }
                match client.list_news_sources().await {
                    Ok(list) => news.write().sources_loaded(list),
                    Err(error) => news.write().sources_failed(error.to_string()),
                }
                match client
                    .list_news_clusters(&tm_trading_contracts::NewsClusterQueryDto::default())
                    .await
                {
                    Ok(list) => news.write().clusters_loaded(list),
                    Err(error) => news.write().clusters_failed(error.to_string()),
                }
                let selected_news_id = news.read().selected_id.clone();
                if let Some(news_id) = selected_news_id {
                    match client.get_news_item(&news_id).await {
                        Ok(item) => news.write().detail_loaded(item),
                        Err(error) => news.write().detail_failed(&news_id, error.to_string()),
                    }
                }

                let briefing_query = AiBriefingQueryDto {
                    limit: Some(50),
                    ..AiBriefingQueryDto::default()
                };
                match client.list_ai_briefings(&briefing_query).await {
                    Ok(list) => ai_briefings.write().list_loaded(list.briefings),
                    Err(error) => {
                        tracing::error!(%error, "failed to list AI briefings");
                        ai_briefings.write().list_failed();
                    }
                }
                let selected_briefing_id = ai_briefings.read().selected_id.clone();
                if let Some(briefing_id) = selected_briefing_id {
                    match client.get_ai_briefing(&briefing_id).await {
                        Ok(briefing) => ai_briefings.write().detail_loaded(briefing),
                        Err(error) => {
                            tracing::error!(%error, briefing_id = %briefing_id, "failed to load AI briefing");
                            ai_briefings.write().detail_failed(&briefing_id);
                        }
                    }
                }

                let tracked_job_id = ai_briefings.read().tracked_job_id().map(str::to_owned);
                if let Some(job_id) = tracked_job_id {
                    match client.get_ai_job(&job_id).await {
                        Ok(job) => ai_briefings.write().tracked_job_loaded(job),
                        Err(error) => {
                            tracing::error!(%error, job_id = %job_id, "failed to load AI job");
                            ai_briefings.write().jobs_failed();
                        }
                    }
                } else {
                    let ai_job_query = AiJobQueryDto {
                        task: Some(AiBriefingTaskDto::ResearchExplanation),
                        limit: Some(20),
                    };
                    match client.list_ai_jobs(&ai_job_query).await {
                        Ok(list) => ai_briefings.write().jobs_loaded(list.jobs),
                        Err(error) => {
                            tracing::error!(%error, "failed to list AI jobs");
                            ai_briefings.write().jobs_failed();
                        }
                    }
                }

                match client.list_ai_provider_profiles().await {
                    Ok(list) => ai_providers.write().list_loaded(list),
                    Err(error) => ai_providers.write().list_failed(error.to_string()),
                }

                match client.list_ai_models().await {
                    Ok(list) => ai_providers.write().models_loaded(list),
                    Err(error) => ai_providers.write().models_failed(error.to_string()),
                }

                match client.list_ai_task_routes().await {
                    Ok(list) => ai_providers.write().routes_loaded(list),
                    Err(error) => ai_providers.write().routes_failed(error.to_string()),
                }

                if let Some(operation_id) = pending_operation() {
                    match client.get_operation(&operation_id).await {
                        Ok(current) => {
                            let finished = current.status.is_final();
                            operation.set(Some(current));
                            if finished {
                                pending_operation.set(None);
                            }
                        }
                        Err(error) => command_error.set(Some(error.to_string())),
                    }
                }
            }
        }
    });

    let rows = services();
    let lists = watchlists();
    let overview = market_overview();
    let pulse = market_pulse();
    let market_error_message = market_error();
    let watchlist_error_message = watchlist_error();
    let name_value = new_list_name();
    let list_sender = actions;
    let refreshed_label = format!(
        "{} {}",
        strings().refreshed_at,
        refreshed_at().unwrap_or_else(|| "-".to_owned())
    );
    let connection_state = connection();
    let current_operation = operation();
    let pending = pending_operation().is_some();
    let action_sender = actions;
    let research_state = research();
    let research_open_sender = actions;
    let research_close_sender = actions;
    let news_state = news();
    let news_open_sender = actions;
    let news_close_sender = actions;
    let ai_briefings_state = ai_briefings();
    let research_events = research_state.events.clone().unwrap_or_default();
    let ai_briefing_open_sender = actions;
    let ai_briefing_close_sender = actions;
    let ai_briefing_request_sender = actions;
    let ai_providers_state = ai_providers();
    let ai_test_sender = actions;
    let ai_model_test_sender = actions;
    let status_class = current_operation
        .as_ref()
        .map(|current| operation_status_class(current.status))
        .unwrap_or("status-progress")
        .to_owned();
    let status_label = current_operation
        .as_ref()
        .map_or("", |current| current.status.as_str())
        .to_owned();

    rsx! {
        AppStyles {}
        div { class: "shell",
            header { class: "topbar",
                div { class: "brand",
                    span { class: "brand-mark" }
                    div {
                        h1 { "{strings().app_title}" }
                        p { class: "brand-subtitle", "{strings().control_center}" }
                    }
                }
                div { class: "topbar-actions",
                    div { class: "connection",
                        span { class: "{connection_dot(&connection_state)}" }
                        span { "{connection_label(&connection_state, &strings())}" }
                    }
                    div { class: "locale-switch", role: "group", aria_label: "{strings().language}",
                        button {
                            class: if locale() == Locale::English { "locale active" } else { "locale" },
                            onclick: move |_| locale.set(Locale::English),
                            "EN"
                        }
                        button {
                            class: if locale() == Locale::Vietnamese { "locale active" } else { "locale" },
                            onclick: move |_| locale.set(Locale::Vietnamese),
                            "VI"
                        }
                    }
                }
            }
            main { class: "content",
                section { class: "panel",
                    div { class: "panel-head",
                        h2 { "{strings().services_and_agents}" }
                        span { class: "panel-meta", "{refreshed_label}" }
                    }
                    if rows.is_empty() {
                        div { class: "empty",
                            p { class: "empty-title", "{strings().no_services}" }
                            p { "{strings().no_services_hint}" }
                        }
                    } else {
                        div { class: "table-wrap",
                            table { class: "services",
                                thead {
                                    tr {
                                        th { "{strings().service_column}" }
                                        th { "{strings().agent_column}" }
                                        th { "{strings().desired_column}" }
                                        th { "{strings().observed_column}" }
                                        th { "{strings().readiness_column}" }
                                        th { class: "numeric", "{strings().restarts_column}" }
                                        th { class: "numeric", "{strings().last_exit_column}" }
                                        th { class: "actions-head", "{strings().actions_column}" }
                                    }
                                }
                                tbody {
                                    for service in rows.iter() {
                                        {
                                            let snapshot = service.clone();
                                            let service_id = snapshot.service_instance_id.clone();
                                            rsx! {
                                                ServiceRow {
                                                    service: snapshot,
                                                    strings: strings(),
                                                    disabled: pending,
                                                    on_command: move |action| {
                                                        action_sender.send(UiAction::Command {
                                                            service_instance_id: service_id.clone(),
                                                            action,
                                                        });
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section { class: "panel operation-panel",
                    div { class: "panel-head",
                        h2 { "{strings().operation}" }
                    }
                    if let Some(error) = command_error() {
                        div { class: "banner error", role: "alert", "{error}" }
                    }
                    if let Some(current) = current_operation.as_ref() {
                        div { class: "operation-body",
                            div { class: "operation-status",
                                span { class: status_class, "{status_label}" }
                            }
                            div { class: "operation-detail",
                                p { class: "operation-id", "{current.operation_id}" }
                                if let Some(summary) = current.result_summary.clone() {
                                    p { class: "operation-summary", "{summary}" }
                                } else if let Some(error) = current.redacted_error.clone() {
                                    p { class: "operation-summary error-text", "{error}" }
                                } else {
                                    p { class: "operation-summary muted", "{strings().operation_pending}" }
                                }
                            }
                        }
                    } else {
                        p { class: "muted", "{strings().operation_none}" }
                    }
                }
                section { class: "panel watchlists-panel",
                    div { class: "panel-head",
                        h2 { "{strings().watchlists}" }
                        span { class: "panel-meta", "{lists.len()}" }
                    }
                    if let Some(error) = watchlist_error_message.as_ref() {
                        div { class: "banner error", role: "alert", "{error}" }
                    }
                    div { class: "watchlist-create",
                        input {
                            class: "text-input",
                            value: "{name_value}",
                            placeholder: "{strings().watchlist_new_placeholder}",
                            aria_label: "{strings().watchlist_new_placeholder}",
                            oninput: move |event| new_list_name.set(event.value()),
                        }
                        button {
                            class: "button primary",
                            onclick: move |_| {
                                let name = new_list_name().trim().to_owned();
                                if name.is_empty() {
                                    return;
                                }
                                list_sender.send(UiAction::Watchlist(WatchlistAction::Create {
                                    name,
                                    symbols: Vec::new(),
                                }));
                                new_list_name.set(String::new());
                            },
                            "{strings().watchlist_add}"
                        }
                    }
                    if lists.is_empty() {
                        div { class: "empty",
                            p { class: "empty-title", "{strings().watchlist_empty}" }
                            p { "{strings().watchlist_empty_hint}" }
                        }
                    } else {
                        div { class: "watchlist-grid",
                            for list in lists.iter() {
                                {
                                    let watchlist = list.clone();
                                    let replace_id = watchlist.id.clone();
                                    let delete_id = watchlist.id.clone();
                                    let replace_sender = list_sender;
                                    let delete_sender = list_sender;
                                    rsx! {
                                        WatchlistCard {
                                            key: "{watchlist.id}",
                                            watchlist,
                                            strings: strings(),
                                            on_replace: move |symbols| {
                                                replace_sender.send(UiAction::Watchlist(
                                                    WatchlistAction::ReplaceSymbols {
                                                        watchlist_id: replace_id.clone(),
                                                        symbols,
                                                    },
                                                ));
                                            },
                                            on_delete: move |_| {
                                                delete_sender.send(UiAction::Watchlist(
                                                    WatchlistAction::Delete {
                                                        watchlist_id: delete_id.clone(),
                                                    },
                                                ));
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section { class: "panel market-panel",
                    div { class: "panel-head",
                        h2 { "{strings().market_overview}" }
                        if let Some(overview) = overview.as_ref() {
                            span { class: "panel-meta",
                                "{overview.timeframe} · {market_state_label(overview.state, &strings())}"
                            }
                        }
                    }
                    if let Some(error) = market_error_message.as_ref() {
                        div { class: "banner error", role: "alert", "{error}" }
                    }
                    if let Some(overview) = overview.as_ref() {
                        if overview.symbols.is_empty() {
                            div { class: "empty",
                                p { class: "empty-title", "{strings().market_empty}" }
                                p { "{strings().market_empty_hint}" }
                            }
                        } else {
                            div { class: "table-wrap",
                                table { class: "services",
                                    thead {
                                        tr {
                                            th { "{strings().market_symbol_column}" }
                                            th { class: "numeric", "{strings().market_last_close_column}" }
                                            th { class: "numeric", "{strings().market_change_column}" }
                                            th { class: "numeric", "{strings().market_volume_column}" }
                                            th { class: "numeric", "{strings().market_age_column}" }
                                            th { "{strings().market_status_column}" }
                                        }
                                    }
                                    tbody {
                                        for item in overview.symbols.iter() {
                                            MarketSymbolRow {
                                                key: "{item.symbol}",
                                                symbol: item.clone(),
                                                strings: strings(),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section { class: "panel market-panel",
                    div { class: "panel-head",
                        h2 { "{strings().market_pulse}" }
                        if let Some(pulse) = pulse.as_ref() {
                            span { class: "panel-meta",
                                "{pulse.timeframe} · {market_state_label(pulse.state, &strings())}"
                            }
                        }
                    }
                    if let Some(pulse) = pulse.as_ref() {
                        if pulse.movers.is_empty() {
                            div { class: "empty",
                                p { class: "empty-title", "{strings().market_empty}" }
                                p { "{strings().market_empty_hint}" }
                            }
                        } else {
                            div { class: "table-wrap",
                                table { class: "services",
                                    thead {
                                        tr {
                                            th { "{strings().market_symbol_column}" }
                                            th { class: "numeric", "{strings().market_last_close_column}" }
                                            th { class: "numeric", "{strings().market_change_column}" }
                                            th { class: "numeric", "{strings().market_volume_ratio_column}" }
                                            th { class: "numeric", "{strings().market_volatility_column}" }
                                            th { "{strings().market_status_column}" }
                                        }
                                    }
                                    tbody {
                                        for mover in pulse.movers.iter() {
                                            MarketMoverRow {
                                                key: "{mover.symbol}",
                                                mover: mover.clone(),
                                                strings: strings(),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                SignalCenterPanel {
                    state: research_state.clone(),
                    strings: strings(),
                    on_open: move |event_id| {
                        research_open_sender.send(UiAction::OpenResearchEvent { event_id });
                    },
                }
                NewsPanel {
                    state: news_state.clone(),
                    strings: strings(),
                    on_open: move |news_id| {
                        news_open_sender.send(UiAction::OpenNewsItem { news_id });
                    },
                }
                AiBriefingPanel {
                    state: ai_briefings_state.clone(),
                    strings: strings(),
                    research_events,
                    on_open: move |briefing_id| {
                        ai_briefing_open_sender.send(UiAction::OpenAiBriefing { briefing_id });
                    },
                    on_request: move |research_event_id| {
                        ai_briefing_request_sender.send(UiAction::RequestAiBriefing {
                            research_event_id,
                        });
                    },
                }
                AiProviderPanel {
                    state: ai_providers_state.clone(),
                    strings: strings(),
                    on_test: move |id| {
                        ai_test_sender.send(UiAction::TestAiProvider { id });
                    },
                    on_test_model: move |id| {
                        ai_model_test_sender.send(UiAction::TestAiModel { id });
                    },
                }
            }
            if research_state.selected_id.is_some() {
                ResearchEventSlideover {
                    state: research_state,
                    strings: strings(),
                    on_close: move |_| {
                        research_close_sender.send(UiAction::CloseResearchEvent);
                    },
                }
            }
            if news_state.selected_id.is_some() {
                NewsItemSlideover {
                    state: news_state,
                    strings: strings(),
                    on_close: move |_| {
                        news_close_sender.send(UiAction::CloseNewsItem);
                    },
                }
            }
            if ai_briefings_state.selected_id.is_some() {
                AiBriefingSlideover {
                    state: ai_briefings_state,
                    strings: strings(),
                    on_close: move |_| {
                        ai_briefing_close_sender.send(UiAction::CloseAiBriefing);
                    },
                }
            }
        }
    }
}

#[component]
fn MarketSymbolRow(symbol: MarketSymbolDto, strings: UiStrings) -> Element {
    rsx! {
        tr {
            td { div { class: "service-name", "{symbol.symbol}" } }
            td { class: "numeric", "{format_number(symbol.last_close)}" }
            td { class: "numeric", "{format_percent(symbol.change_percent)}" }
            td { class: "numeric", "{format_number(symbol.volume)}" }
            td { class: "numeric", "{format_age(symbol.data_age_seconds)}" }
            td {
                span { class: "pill {market_state_class(symbol.state)}",
                    "{market_state_label(symbol.state, &strings)}"
                }
            }
        }
    }
}

#[component]
fn MarketMoverRow(mover: MarketMoverDto, strings: UiStrings) -> Element {
    rsx! {
        tr {
            td { div { class: "service-name", "{mover.symbol}" } }
            td { class: "numeric", "{format_number(mover.last_close)}" }
            td { class: "numeric", "{format_percent(mover.change_percent)}" }
            td { class: "numeric", "{format_number(mover.volume_ratio)}" }
            td { class: "numeric", "{format_percent(mover.volatility_percent)}" }
            td {
                span { class: "pill {market_state_class(mover.state)}",
                    "{market_state_label(mover.state, &strings)}"
                }
            }
        }
    }
}

#[component]
fn ServiceRow(
    service: ServiceSnapshot,
    strings: UiStrings,
    disabled: bool,
    on_command: EventHandler<OperationActionDto>,
) -> Element {
    let running = service.observed_state == tm_trading_contracts::ObservedStateDto::Running;
    let desired = service.desired_state.as_str();
    let observed = service.observed_state.as_str();
    let readiness = service.readiness.as_str();
    let desired_class = format!("pill state-{}", desired.to_lowercase());
    let observed_class = format!("pill state-{}", observed.to_lowercase());
    let readiness_class = format!("pill state-{}", readiness.to_lowercase());
    let restart_count = service.restart_count;
    let last_exit = match service.last_exit_code {
        Some(code) => code.to_string(),
        None => "-".to_owned(),
    };
    let definition_id = service.service_definition_id.clone();
    let instance_id = service.service_instance_id.clone();
    let agent_id = service.agent_id.clone();

    rsx! {
        tr {
            td {
                div { class: "service-name", "{definition_id}" }
                div { class: "service-instance", title: "{instance_id}", "{instance_id}" }
            }
            td { class: "muted", "{agent_id}" }
            td { span { class: desired_class, "{desired}" } }
            td { span { class: observed_class, "{observed}" } }
            td { span { class: readiness_class, "{readiness}" } }
            td { class: "numeric", "{restart_count}" }
            td { class: "numeric", "{last_exit}" }
            td { class: "actions",
                button {
                    class: "button primary",
                    disabled: disabled || running,
                    onclick: move |_| on_command.call(OperationActionDto::Start),
                    "{strings.start}"
                }
                button {
                    class: "button",
                    disabled: disabled || !running,
                    onclick: move |_| on_command.call(OperationActionDto::Stop),
                    "{strings.stop}"
                }
                button {
                    class: "button",
                    disabled: disabled || !running,
                    onclick: move |_| on_command.call(OperationActionDto::Restart),
                    "{strings.restart}"
                }
            }
        }
    }
}

#[component]
fn WatchlistCard(
    watchlist: WatchlistDto,
    strings: UiStrings,
    on_replace: EventHandler<Vec<String>>,
    on_delete: EventHandler<()>,
) -> Element {
    let mut draft = use_signal(|| watchlist.symbols.join(", "));
    let name = watchlist.name.clone();
    let symbols = watchlist.symbols.clone();
    let count = symbols.len();
    let is_default = watchlist.is_default;

    rsx! {
        article { class: "watchlist-card",
            header { class: "watchlist-card-head",
                h3 { "{name}" }
                div { class: "watchlist-card-meta",
                    if is_default {
                        span { class: "pill state-running", "{strings.watchlist_default}" }
                    }
                    span { class: "muted", "{count} {strings.watchlist_symbols_count}" }
                }
            }
            div { class: "chips",
                for symbol in symbols.iter() {
                    span { class: "chip", "{symbol}" }
                }
            }
            input {
                class: "text-input",
                value: "{draft}",
                placeholder: "{strings.watchlist_symbols_placeholder}",
                aria_label: "{strings.watchlist_symbols_placeholder}",
                oninput: move |event| draft.set(event.value()),
            }
            div { class: "watchlist-card-actions",
                button {
                    class: "button primary",
                    onclick: move |_| on_replace.call(parse_symbols(&draft())),
                    "{strings.watchlist_save}"
                }
                button {
                    class: "button",
                    onclick: move |_| on_delete.call(()),
                    "{strings.watchlist_delete}"
                }
            }
        }
    }
}

/// Splits a comma-separated symbol field; the server does the real validation.
fn parse_symbols(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn format_number(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"))
}

fn format_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}%"))
}

fn format_age(seconds: Option<i64>) -> String {
    seconds.map_or_else(|| "-".to_owned(), |seconds| format!("{seconds}s"))
}

fn market_state_label(state: MarketStateDto, strings: &UiStrings) -> &'static str {
    match state {
        MarketStateDto::Ok => strings.market_state_ok,
        MarketStateDto::Partial => strings.market_state_partial,
        MarketStateDto::Stale => strings.market_state_stale,
        MarketStateDto::Empty => strings.market_state_empty,
    }
}

fn market_state_class(state: MarketStateDto) -> &'static str {
    match state {
        MarketStateDto::Ok => "state-ok",
        MarketStateDto::Partial => "state-partial",
        MarketStateDto::Stale => "state-stale",
        MarketStateDto::Empty => "state-empty",
    }
}

fn connection_label(state: &ConnectionState, strings: &UiStrings) -> String {
    match state {
        ConnectionState::Connecting => strings.connecting.to_owned(),
        ConnectionState::Connected => strings.connected.to_owned(),
        ConnectionState::Disconnected(_) => strings.disconnected.to_owned(),
    }
}

fn connection_dot(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::Connecting => "dot connecting",
        ConnectionState::Connected => "dot connected",
        ConnectionState::Disconnected(_) => "dot disconnected",
    }
}

fn operation_status_class(status: OperationStatusDto) -> &'static str {
    match status {
        OperationStatusDto::Succeeded => "status-succeeded",
        OperationStatusDto::Failed | OperationStatusDto::Expired => "status-failed",
        OperationStatusDto::Cancelled => "status-cancelled",
        _ => "status-progress",
    }
}

#[cfg(test)]
mod tests {
    use super::parse_symbols;

    #[test]
    fn splits_and_trims_symbols() {
        assert_eq!(parse_symbols(" btcusdt, ,ETHUSDT "), ["btcusdt", "ETHUSDT"]);
        assert!(parse_symbols("  ").is_empty());
    }
}
