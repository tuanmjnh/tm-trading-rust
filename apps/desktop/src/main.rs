// A release build is a Windows GUI application: opening it must not allocate a console
// window. Debug builds keep the console so startup logs stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai_briefings;
mod ai_providers;
mod dropdown;
mod i18n;
mod modal;
mod news;
mod notifications;
mod signal_center;
mod slideover;
mod system_diagnostics;
mod webview2_check;

use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use dropdown::{ActionDropdown, DropdownItem};
use futures_util::StreamExt;
use modal::Modal;
use slideover::Slideover;
use tm_trading_api_client::{ManagementApiClient, new_idempotency_key};
use tm_trading_contracts::{
    AiBriefingQueryDto, AiJobQueryDto, AiJobTaskDto, MarketMoverDto, MarketOverviewDto,
    MarketPulseDto, MarketStateDto, MarketSymbolDto, NewAiJobDto, NewAiModelDto,
    NewAiProviderProfileDto, NewAiTaskRouteDto, NewsQueryDto, NotificationLogQueryDto,
    NotificationOutboxQueryDto, NotificationSummaryQueryDto, OperationActionDto, OperationResponse,
    OperationStatusDto, ResearchEventQueryDto, SendTestNotificationRequest, ServiceSnapshot,
    WatchlistDto,
};

use ai_briefings::{AiBriefingPanel, AiBriefingPanelState, AiBriefingSlideover};
use ai_providers::{AiProviderPanel, AiProviderPanelState};
use i18n::{Locale, UiStrings};
use news::{NewsItemSlideover, NewsPanel, NewsPanelState};
use notifications::{NotificationCenterPanel, NotificationPanelState};
use signal_center::{ResearchEventSlideover, ResearchPanelState, SignalCenterPanel};
use system_diagnostics::{SystemDiagnosticsPanel, SystemDiagnosticsPanelState};
use webview2_check::detect_webview2_runtime;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_API_URL: &str = "http://127.0.0.1:8787";

fn main() {
    let log_format =
        tm_trading_config::LogFormat::from_env().unwrap_or(tm_trading_config::LogFormat::Compact);
    let _ = tm_trading_observability::init(log_format);

    let webview2_status = detect_webview2_runtime();
    if !webview2_status.detected || !webview2_status.meets_minimum {
        tracing::warn!(
            status = ?webview2_status,
            "WebView2 runtime verification: {}",
            webview2_status.advisory
        );
    } else {
        tracing::info!(
            version = ?webview2_status.version,
            "WebView2 runtime verification passed."
        );
    }

    let config = dioxus::desktop::Config::default()
        .with_data_directory(data_directory())
        .with_window(
            dioxus::desktop::WindowBuilder::new()
                .with_title("TM Trading Control Center")
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(1380.0, 880.0)),
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
    CreateAiProviderProfile(NewAiProviderProfileDto),
    DeleteAiProviderProfile(String),
    CreateAiModel {
        provider_id: String,
        request: NewAiModelDto,
    },
    DeleteAiModel(String),
    CreateAiTaskRoute(NewAiTaskRouteDto),
    DeleteAiTaskRoute(String),
    SendTestNotification {
        chat_id: Option<String>,
        message: Option<String>,
    },
    GenerateSupportBundle,
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

/// Controls which modal or slideover overlay is currently active.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum ActiveOverlay {
    #[default]
    None,
    WatchlistModal,
    AiConfigSlideover,
    NotificationsSlideover,
    DiagnosticsSlideover,
    ServiceDetailsSlideover(String),
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
    let mut active_overlay = use_signal(ActiveOverlay::default);
    let strings = use_memo(move || UiStrings::for_locale(locale()));
    let services = use_signal(Vec::<ServiceSnapshot>::new);
    let connection = use_signal(|| ConnectionState::Connecting);
    let operation = use_signal(|| Option::<OperationResponse>::None);
    let command_error = use_signal(|| Option::<String>::None);
    let refreshed_at = use_signal(|| Option::<String>::None);
    let pending_operation = use_signal(|| Option::<String>::None);
    let watchlists = use_signal(Vec::<WatchlistDto>::new);
    let watchlist_error = use_signal(|| Option::<String>::None);
    let market_overview = use_signal(|| Option::<MarketOverviewDto>::None);
    let market_pulse = use_signal(|| Option::<MarketPulseDto>::None);
    let market_error = use_signal(|| Option::<String>::None);
    let research = use_signal(ResearchPanelState::default);
    let news = use_signal(NewsPanelState::default);
    let ai_briefings = use_signal(AiBriefingPanelState::default);
    let ai_providers = use_signal(AiProviderPanelState::default);
    let notifications = use_signal(NotificationPanelState::default);
    let system_diagnostics =
        use_signal(|| SystemDiagnosticsPanelState::new(detect_webview2_runtime()));

    let client_task = client.clone();
    let actions = use_coroutine(move |mut actions: UnboundedReceiver<UiAction>| {
        let client = client_task.clone();
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
        let mut notifications = notifications;
        let mut system_diagnostics = system_diagnostics;
        async move {
            let mut tick: u64 = 0;
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
                                    task: AiJobTaskDto::ResearchExplanation,
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
                            Some(UiAction::CreateAiProviderProfile(request)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.create_ai_provider_profile(&request).await {
                                    Ok(_) => {
                                        if let Ok(list) = client.list_ai_provider_profiles().await {
                                            ai_providers.write().list_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().list_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::DeleteAiProviderProfile(id)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.delete_ai_provider_profile(&id).await {
                                    Ok(()) => {
                                        if let Ok(list) = client.list_ai_provider_profiles().await {
                                            ai_providers.write().list_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().list_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::CreateAiModel { provider_id, request }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.create_ai_model(&provider_id, &request).await {
                                    Ok(_) => {
                                        if let Ok(list) = client.list_ai_models().await {
                                            ai_providers.write().models_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().models_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::DeleteAiModel(id)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.delete_ai_model(&id).await {
                                    Ok(()) => {
                                        if let Ok(list) = client.list_ai_models().await {
                                            ai_providers.write().models_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().models_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::CreateAiTaskRoute(request)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.create_ai_task_route(&request).await {
                                    Ok(_) => {
                                        if let Ok(list) = client.list_ai_task_routes().await {
                                            ai_providers.write().routes_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().routes_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::DeleteAiTaskRoute(task)) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                match client.delete_ai_task_route(&task).await {
                                    Ok(()) => {
                                        if let Ok(list) = client.list_ai_task_routes().await {
                                            ai_providers.write().routes_loaded(list);
                                        }
                                    }
                                    Err(error) => ai_providers.write().routes_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::SendTestNotification { chat_id, message }) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                let req = SendTestNotificationRequest { chat_id, message };
                                match client.send_test_notification(&req).await {
                                    Ok(res) => notifications.write().test_completed(res),
                                    Err(error) => notifications.write().test_failed(error.to_string()),
                                }
                            }
                            Some(UiAction::GenerateSupportBundle) => {
                                let Some(client) = client.as_ref() else {
                                    break;
                                };
                                system_diagnostics.write().bundle_generating();
                                match client.create_support_bundle().await {
                                    Ok(bundle) => system_diagnostics.write().bundle_loaded(bundle),
                                    Err(error) => system_diagnostics.write().bundle_failed(error.to_string()),
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
                tick = tick.wrapping_add(1);

                // Service Fleet List (Primary 2s cycle)
                match client.list_services().await {
                    Ok(list) => {
                        services.set(list);
                        connection.set(ConnectionState::Connected);
                    }
                    Err(error) => connection.set(ConnectionState::Disconnected(error.to_string())),
                }
                refreshed_at.set(Some(chrono::Local::now().format("%H:%M:%S").to_string()));

                let is_bg_tick = tick == 1 || tick % 5 == 0;

                // Market & Research & News & AI Briefings (2s cycle)
                let (watchlists_res, overview_res, pulse_res) = tokio::join!(
                    client.list_watchlists(),
                    client.market_overview(&[], "1m", 60),
                    client.market_pulse(&[], "1m", 60)
                );
                if let Ok(list) = watchlists_res {
                    watchlists.set(list);
                }
                match overview_res {
                    Ok(overview) => {
                        market_overview.set(Some(overview));
                        market_error.set(None);
                    }
                    Err(error) => market_error.set(Some(error.to_string())),
                }
                if let Ok(pulse) = pulse_res {
                    market_pulse.set(Some(pulse));
                }

                let research_query = ResearchEventQueryDto {
                    limit: Some(25),
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
                    limit: Some(25),
                    ..NewsQueryDto::default()
                };
                let briefing_query = AiBriefingQueryDto {
                    limit: Some(20),
                    ..AiBriefingQueryDto::default()
                };
                let (news_res, briefings_res) = tokio::join!(
                    client.list_news(&news_query),
                    client.list_ai_briefings(&briefing_query)
                );
                match news_res {
                    Ok(list) => news.write().list_loaded(list),
                    Err(error) => news.write().list_failed(error.to_string()),
                }
                match briefings_res {
                    Ok(list) => ai_briefings.write().list_loaded(list.briefings),
                    Err(error) => {
                        tracing::error!(%error, "failed to list AI briefings");
                        ai_briefings.write().list_failed();
                    }
                }

                let selected_news_id = news.read().selected_id.clone();
                if let Some(news_id) = selected_news_id {
                    match client.get_news_item(&news_id).await {
                        Ok(item) => news.write().detail_loaded(item),
                        Err(error) => news.write().detail_failed(&news_id, error.to_string()),
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
                } else if is_bg_tick {
                    let ai_job_query = AiJobQueryDto {
                        task: Some(AiJobTaskDto::ResearchExplanation),
                        limit: Some(20),
                    };
                    if let Ok(list) = client.list_ai_jobs(&ai_job_query).await {
                        ai_briefings.write().jobs_loaded(list.jobs);
                    }
                }

                // Background Polling (Every ~10s: AI Profiles, Routes, Notifications, Diagnostics)
                if is_bg_tick {
                    let cluster_query = tm_trading_contracts::NewsClusterQueryDto::default();
                    let (sources_res, clusters_res, profiles_res, models_res, routes_res) = tokio::join!(
                        client.list_news_sources(),
                        client.list_news_clusters(&cluster_query),
                        client.list_ai_provider_profiles(),
                        client.list_ai_models(),
                        client.list_ai_task_routes()
                    );
                    if let Ok(list) = sources_res {
                        news.write().sources_loaded(list);
                    }
                    if let Ok(list) = clusters_res {
                        news.write().clusters_loaded(list);
                    }
                    if let Ok(list) = profiles_res {
                        ai_providers.write().list_loaded(list);
                    }
                    if let Ok(list) = models_res {
                        ai_providers.write().models_loaded(list);
                    }
                    if let Ok(list) = routes_res {
                        ai_providers.write().routes_loaded(list);
                    }

                    let outbox_query = NotificationOutboxQueryDto {
                        limit: Some(20),
                        ..NotificationOutboxQueryDto::default()
                    };
                    let log_query = NotificationLogQueryDto {
                        limit: Some(20),
                        ..NotificationLogQueryDto::default()
                    };
                    let summary_query = NotificationSummaryQueryDto::default();
                    let (outbox_res, summary_res, logs_res, diag_res) = tokio::join!(
                        client.list_notification_outbox(&outbox_query),
                        client.notification_outbox_summary(&summary_query),
                        client.list_notification_logs(&log_query),
                        client.system_diagnostics()
                    );
                    if let Ok(list) = outbox_res {
                        notifications.write().outbox_loaded(list);
                    }
                    if let Ok(summary) = summary_res {
                        notifications.write().summary_loaded(summary);
                    }
                    if let Ok(logs) = logs_res {
                        notifications.write().logs_loaded(logs);
                    }
                    if let Ok(diag) = diag_res {
                        system_diagnostics.write().diagnostics_loaded(diag);
                    }
                }

                // Finalize Pending Operation Poll
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
    let pulse = market_pulse();
    let watchlist_error_message = watchlist_error();
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
    let ai_create_provider_sender = actions;
    let ai_delete_provider_sender = actions;
    let ai_create_model_sender = actions;
    let ai_delete_model_sender = actions;
    let ai_create_route_sender = actions;
    let ai_delete_route_sender = actions;
    let notifications_state = notifications();
    let notifications_test_sender = actions;
    let system_diagnostics_state = system_diagnostics();
    let diagnostics_bundle_sender = actions;
    let status_class = current_operation
        .as_ref()
        .map(|current| operation_status_class(current.status))
        .unwrap_or("status-progress")
        .to_owned();
    let status_label = current_operation
        .as_ref()
        .map_or("", |current| current.status.as_str())
        .to_owned();

    // Derived KPI values for immediate display
    let ready_services_count = rows
        .iter()
        .filter(|s| s.readiness == tm_trading_contracts::ReadinessDto::Ready)
        .count();
    let total_services_count = rows.len();
    let pulse_state_label = pulse
        .as_ref()
        .map_or("N/A", |p| market_state_label(p.state, &strings()));
    let signals_count = research_events.len();
    let news_count = news_state.items.as_ref().map_or(0, |items| items.len());
    let routes_count = ai_providers_state
        .routes
        .as_ref()
        .map_or(0, |routes| routes.len());
    let outbox_count = notifications_state
        .outbox
        .as_ref()
        .map_or(0, |items| items.len());
    let webview2_version = system_diagnostics_state
        .webview2
        .version
        .as_deref()
        .unwrap_or("System default");

    rsx! {
        AppStyles {}
        div { class: "shell",
            // Unified Topbar: Brand, Connectivity, Locale & Quick Actions Dropdown
            header { class: "topbar",
                div { class: "brand",
                    span { class: "brand-mark" }
                    div {
                        h1 { "{strings().app_title}" }
                        p { class: "brand-subtitle", "UNIFIED OPERATIONS DESK" }
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
                    ActionDropdown {
                        trigger_label: format!("⚙️ {}", strings().quick_settings),
                        disabled: false,
                        items: vec![
                            DropdownItem {
                                id: "ai_config".to_owned(),
                                label: format!("🤖 {}", strings().tab_ai_settings),
                                class: None,
                                disabled: false,
                            },
                            DropdownItem {
                                id: "notifications".to_owned(),
                                label: format!("🔔 {}", strings().tab_notifications),
                                class: None,
                                disabled: false,
                            },
                            DropdownItem {
                                id: "diagnostics".to_owned(),
                                label: format!("🛠️ {}", strings().tab_diagnostics),
                                class: None,
                                disabled: false,
                            },
                            DropdownItem {
                                id: "watchlist".to_owned(),
                                label: format!("📋 {}", strings().watchlists_manage),
                                class: None,
                                disabled: false,
                            },
                        ],
                        on_select: move |action_id: String| {
                            match action_id.as_str() {
                                "ai_config" => active_overlay.set(ActiveOverlay::AiConfigSlideover),
                                "notifications" => active_overlay.set(ActiveOverlay::NotificationsSlideover),
                                "diagnostics" => active_overlay.set(ActiveOverlay::DiagnosticsSlideover),
                                "watchlist" => active_overlay.set(ActiveOverlay::WatchlistModal),
                                _ => {}
                            }
                        },
                    }
                }
            }

            // Executive KPI Ribbon: Dense, high-contrast operational status cards
            div { class: "kpi-ribbon",
                div {
                    class: "kpi-chip interactive",
                    title: "Click to inspect services",
                    onclick: move |_| {
                        if let Some(first) = rows.first() {
                            active_overlay.set(ActiveOverlay::ServiceDetailsSlideover(first.service_instance_id.clone()));
                        }
                    },
                    span { class: "kpi-title", "{strings().kpi_services}" }
                    span { class: "kpi-value", "{ready_services_count}/{total_services_count}" }
                }
                div {
                    class: "kpi-chip interactive",
                    title: "Click to manage watchlists",
                    onclick: move |_| active_overlay.set(ActiveOverlay::WatchlistModal),
                    span { class: "kpi-title", "{strings().kpi_market}" }
                    span { class: "kpi-value", "{pulse_state_label}" }
                }
                div { class: "kpi-chip",
                    span { class: "kpi-title", "{strings().kpi_signals}" }
                    span { class: "kpi-value", "{signals_count}" }
                }
                div { class: "kpi-chip",
                    span { class: "kpi-title", "{strings().kpi_news}" }
                    span { class: "kpi-value", "{news_count}" }
                }
                div {
                    class: "kpi-chip interactive",
                    title: "Click to configure AI providers and task routes",
                    onclick: move |_| active_overlay.set(ActiveOverlay::AiConfigSlideover),
                    span { class: "kpi-title", "{strings().kpi_ai_routes}" }
                    span { class: "kpi-value", "{routes_count}" }
                }
                div {
                    class: "kpi-chip interactive",
                    title: "Click to open notification outbox and test channel",
                    onclick: move |_| active_overlay.set(ActiveOverlay::NotificationsSlideover),
                    span { class: "kpi-title", "{strings().kpi_outbox}" }
                    span { class: "kpi-value", "{outbox_count}" }
                }
                div {
                    class: "kpi-chip interactive",
                    title: "Click to open system diagnostics and support bundle",
                    onclick: move |_| active_overlay.set(ActiveOverlay::DiagnosticsSlideover),
                    span { class: "kpi-title", "{strings().kpi_runtime}" }
                    span { class: "kpi-value", "{webview2_version}" }
                }
            }

            // Unified Operations Content (Single Bento / Split Grid Layout)
            main { class: "content",
                // Section 1: Core Service Control Fleet (Left) + Market Pulse & Movers (Right)
                div { class: "dashboard-split-grid", style: "margin-bottom: 20px;",
                    div { class: "column-primary",
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
                                                            on_details: move |inst_id| {
                                                                active_overlay.set(ActiveOverlay::ServiceDetailsSlideover(inst_id));
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
                        if let Some(current) = current_operation.as_ref() {
                            section { class: "panel operation-panel",
                                div { class: "panel-head",
                                    h2 { "{strings().operation}" }
                                }
                                if let Some(error) = command_error() {
                                    div { class: "banner error", role: "alert", "{error}" }
                                }
                                div { class: "operation-body",
                                    div { class: "operation-status",
                                        span { class: status_class.clone(), "{status_label}" }
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
                            }
                        }
                    }
                    div { class: "column-secondary",
                        section { class: "panel market-panel",
                            div { class: "panel-head",
                                div { class: "flex items-center gap-2",
                                    h2 { "{strings().market_pulse}" }
                                    if let Some(pulse) = pulse.as_ref() {
                                        span { class: "panel-meta",
                                            "{pulse.timeframe} · {market_state_label(pulse.state, &strings())}"
                                        }
                                    }
                                }
                                button {
                                    class: "button",
                                    onclick: move |_| active_overlay.set(ActiveOverlay::WatchlistModal),
                                    "📋 {strings().watchlists_manage}"
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
                                                for mover in pulse.movers.iter().take(6) {
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
                    }
                }

                // Section 2: Signal Center (Left) + News & AI Briefings (Right)
                div { class: "dashboard-split-grid",
                    div { class: "column-primary",
                        SignalCenterPanel {
                            state: research_state.clone(),
                            strings: strings(),
                            on_open: move |event_id| {
                                research_open_sender.send(UiAction::OpenResearchEvent { event_id });
                            },
                        }
                    }
                    div { class: "column-secondary",
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
                    }
                }
            }

            // Bottom Telemetry Bar: Host Agent status, MongoDB connection, Outbox health
            footer { class: "telemetry-bar",
                div { class: "telemetry-group",
                    div { class: "telemetry-item",
                        span { class: "telemetry-pill live", "{strings().telemetry_live}" }
                        span { "{strings().telemetry_agent}: 127.0.0.1" }
                    }
                    div { class: "telemetry-item",
                        span { class: "telemetry-pill live", "{strings().telemetry_live}" }
                        span { "{strings().telemetry_mongodb}: Connected" }
                    }
                    div {
                        class: "telemetry-item interactive",
                        title: "Open Notification Center",
                        onclick: move |_| active_overlay.set(ActiveOverlay::NotificationsSlideover),
                        span { "🔔 {strings().telemetry_outbox}: {outbox_count} {strings().logs_unit}" }
                    }
                }
                div { class: "telemetry-actions",
                    button {
                        class: "button",
                        onclick: move |_| active_overlay.set(ActiveOverlay::DiagnosticsSlideover),
                        "🛠️ {strings().tab_diagnostics}"
                    }
                    button {
                        class: "button",
                        onclick: move |_| active_overlay.set(ActiveOverlay::AiConfigSlideover),
                        "🤖 {strings().tab_ai_settings}"
                    }
                }
            }

            // Overlays: Deep Configuration & Operations (Modals & Slideovers)
            if let ActiveOverlay::ServiceDetailsSlideover(inst_id) = active_overlay() {
                {
                    let selected_svc = rows.iter().find(|s| s.service_instance_id == inst_id).cloned();
                    let service_inst_id = inst_id.clone();
                    rsx! {
                        ServiceDetailsSlideover {
                            service: selected_svc,
                            strings: strings(),
                            pending,
                            on_close: move |_| active_overlay.set(ActiveOverlay::None),
                            on_command: move |action| {
                                action_sender.send(UiAction::Command {
                                    service_instance_id: service_inst_id.clone(),
                                    action,
                                });
                            },
                        }
                    }
                }
            }
            if active_overlay() == ActiveOverlay::WatchlistModal {
                WatchlistModal {
                    watchlists: lists.clone(),
                    strings: strings(),
                    error: watchlist_error_message.clone(),
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    on_create: move |name| {
                        list_sender.send(UiAction::Watchlist(WatchlistAction::Create {
                            name,
                            symbols: Vec::new(),
                        }));
                    },
                    on_replace: move |(watchlist_id, symbols)| {
                        list_sender.send(UiAction::Watchlist(WatchlistAction::ReplaceSymbols {
                            watchlist_id,
                            symbols,
                        }));
                    },
                    on_delete: move |watchlist_id| {
                        list_sender.send(UiAction::Watchlist(WatchlistAction::Delete {
                            watchlist_id,
                        }));
                    },
                }
            }
            if active_overlay() == ActiveOverlay::AiConfigSlideover {
                Slideover {
                    title: strings().tab_ai_settings.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    AiProviderPanel {
                        state: ai_providers_state.clone(),
                        strings: strings(),
                        on_test: move |id| {
                            ai_test_sender.send(UiAction::TestAiProvider { id });
                        },
                        on_test_model: move |id| {
                            ai_model_test_sender.send(UiAction::TestAiModel { id });
                        },
                        on_create_provider: move |req| {
                            ai_create_provider_sender.send(UiAction::CreateAiProviderProfile(req));
                        },
                        on_delete_provider: move |id| {
                            ai_delete_provider_sender.send(UiAction::DeleteAiProviderProfile(id));
                        },
                        on_create_model: move |(provider_id, request)| {
                            ai_create_model_sender.send(UiAction::CreateAiModel {
                                provider_id,
                                request,
                            });
                        },
                        on_delete_model: move |id| {
                            ai_delete_model_sender.send(UiAction::DeleteAiModel(id));
                        },
                        on_create_route: move |req| {
                            ai_create_route_sender.send(UiAction::CreateAiTaskRoute(req));
                        },
                        on_delete_route: move |task| {
                            ai_delete_route_sender.send(UiAction::DeleteAiTaskRoute(task));
                        },
                    }
                }
            }
            if active_overlay() == ActiveOverlay::NotificationsSlideover {
                Slideover {
                    title: strings().tab_notifications.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    NotificationCenterPanel {
                        state: notifications_state,
                        strings: strings(),
                        on_send_test: move |(chat_id, message)| {
                            notifications_test_sender.send(UiAction::SendTestNotification {
                                chat_id,
                                message,
                            });
                        },
                    }
                }
            }
            if active_overlay() == ActiveOverlay::DiagnosticsSlideover {
                Slideover {
                    title: strings().tab_diagnostics.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    SystemDiagnosticsPanel {
                        state: system_diagnostics_state,
                        strings: strings(),
                        disabled: pending,
                        on_generate_bundle: move |_| {
                            diagnostics_bundle_sender.send(UiAction::GenerateSupportBundle);
                        },
                    }
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

#[allow(dead_code)]
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
    on_details: EventHandler<String>,
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
    let details_id_cell = service.service_instance_id.clone();
    let details_id_menu = service.service_instance_id.clone();

    rsx! {
        tr {
            td {
                class: "clickable-cell",
                onclick: move |_| on_details.call(details_id_cell.clone()),
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
                if running {
                    button {
                        class: "button primary",
                        disabled: disabled,
                        onclick: move |_| on_command.call(OperationActionDto::Restart),
                        "{strings.restart}"
                    }
                } else {
                    button {
                        class: "button primary",
                        disabled: disabled,
                        onclick: move |_| on_command.call(OperationActionDto::Start),
                        "{strings.start}"
                    }
                }
                ActionDropdown {
                    trigger_label: "•••".to_owned(),
                    disabled: disabled,
                    items: vec![
                        DropdownItem {
                            id: "details".to_owned(),
                            label: strings.view_details.to_owned(),
                            class: None,
                            disabled: false,
                        },
                        DropdownItem {
                            id: "start".to_owned(),
                            label: strings.start.to_owned(),
                            class: Some("action-success".to_owned()),
                            disabled: running,
                        },
                        DropdownItem {
                            id: "restart".to_owned(),
                            label: strings.restart.to_owned(),
                            class: Some("action-warning".to_owned()),
                            disabled: !running,
                        },
                        DropdownItem {
                            id: "stop".to_owned(),
                            label: strings.stop.to_owned(),
                            class: Some("action-danger".to_owned()),
                            disabled: !running,
                        },
                    ],
                    on_select: move |action_id: String| {
                        match action_id.as_str() {
                            "details" => on_details.call(details_id_menu.clone()),
                            "start" => on_command.call(OperationActionDto::Start),
                            "stop" => on_command.call(OperationActionDto::Stop),
                            "restart" => on_command.call(OperationActionDto::Restart),
                            _ => {}
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn ServiceDetailsSlideover(
    service: Option<ServiceSnapshot>,
    strings: UiStrings,
    pending: bool,
    on_close: EventHandler<()>,
    on_command: EventHandler<OperationActionDto>,
) -> Element {
    let Some(svc) = service else {
        return rsx! {};
    };
    let running = svc.observed_state == tm_trading_contracts::ObservedStateDto::Running;
    let desired = svc.desired_state.as_str();
    let observed = svc.observed_state.as_str();
    let readiness = svc.readiness.as_str();
    let desired_class = format!("pill state-{}", desired.to_lowercase());
    let observed_class = format!("pill state-{}", observed.to_lowercase());
    let readiness_class = format!("pill state-{}", readiness.to_lowercase());
    let restarts = svc.restart_count;
    let last_exit = svc
        .last_exit_code
        .map_or_else(|| "-".to_owned(), |c| c.to_string());
    let last_heartbeat = svc
        .last_heartbeat_at
        .map_or_else(|| "-".to_owned(), |t| t.format("%H:%M:%S UTC").to_string());
    let (cmd_start, cmd_restart, cmd_stop) = (on_command, on_command, on_command);

    rsx! {
        Slideover {
            title: format!("{}: {}", strings.service_details, svc.service_definition_id),
            close_label: strings.modal_close.to_owned(),
            on_close: move |_| on_close.call(()),
            div { class: "research-detail",
                div { class: "card",
                    h3 { "{svc.service_definition_id}" }
                    p { class: "muted", "{strings.agent_column}: {svc.agent_id}" }
                    div { class: "detail-grid",
                        div { class: "detail-col",
                            span { class: "label", "Instance ID" }
                            span { class: "mono text-sm", "{svc.service_instance_id}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "{strings.desired_column}" }
                            span { class: desired_class, "{desired}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "{strings.observed_column}" }
                            span { class: observed_class, "{observed}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "{strings.readiness_column}" }
                            span { class: readiness_class, "{readiness}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "{strings.restarts_column}" }
                            span { class: "mono", "{restarts}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "{strings.last_exit_column}" }
                            span { class: "mono", "{last_exit}" }
                        }
                        div { class: "detail-col",
                            span { class: "label", "Last Heartbeat" }
                            span { class: "mono text-xs", "{last_heartbeat}" }
                        }
                    }
                }
                div { class: "card",
                    h3 { "{strings.actions_column}" }
                    div { class: "button-row",
                        if running {
                            button {
                                class: "button warning",
                                disabled: pending,
                                onclick: move |_| cmd_restart.call(OperationActionDto::Restart),
                                "{strings.restart}"
                            }
                            button {
                                class: "button danger",
                                disabled: pending,
                                onclick: move |_| cmd_stop.call(OperationActionDto::Stop),
                                "{strings.stop}"
                            }
                        } else {
                            button {
                                class: "button primary",
                                disabled: pending,
                                onclick: move |_| cmd_start.call(OperationActionDto::Start),
                                "{strings.start}"
                            }
                        }
                    }
                }
                div { class: "card",
                    h3 { "Diagnostic Information" }
                    p { class: "text-sm text-muted",
                        "Worker logs are continuously streamed to 'target/logs/{svc.service_definition_id}.log'. For exit code 78, verify all required environment variables in '.env'."
                    }
                }
            }
        }
    }
}

#[component]
fn WatchlistModal(
    watchlists: Vec<WatchlistDto>,
    strings: UiStrings,
    error: Option<String>,
    on_close: EventHandler<()>,
    on_create: EventHandler<String>,
    on_replace: EventHandler<(String, Vec<String>)>,
    on_delete: EventHandler<String>,
) -> Element {
    let mut new_name = use_signal(String::new);
    let (close_handler, create_handler) = (on_close, on_create);

    rsx! {
        Modal {
            title: strings.watchlists_manage.to_owned(),
            close_label: strings.modal_close.to_owned(),
            on_close: move |_| close_handler.call(()),
            div { class: "watchlist-modal-content",
                if let Some(err) = error {
                    div { class: "banner error", role: "alert", "{err}" }
                }
                div { class: "watchlist-create", style: "margin-bottom: 20px;",
                    input {
                        class: "text-input",
                        value: "{new_name()}",
                        placeholder: "{strings.watchlist_new_placeholder}",
                        aria_label: "{strings.watchlist_new_placeholder}",
                        oninput: move |event| new_name.set(event.value()),
                    }
                    button {
                        class: "button primary",
                        onclick: move |_| {
                            let name = new_name().trim().to_owned();
                            if !name.is_empty() {
                                create_handler.call(name);
                                new_name.set(String::new());
                            }
                        },
                        "{strings.watchlist_add}"
                    }
                }
                if watchlists.is_empty() {
                    div { class: "empty",
                        p { class: "empty-title", "{strings.watchlist_empty}" }
                        p { "{strings.watchlist_empty_hint}" }
                    }
                } else {
                    div { class: "watchlist-grid",
                        for list in watchlists.iter() {
                            {
                                let watchlist = list.clone();
                                let list_id = watchlist.id.clone();
                                let del_id = watchlist.id.clone();
                                let replace_cb = on_replace;
                                let delete_cb = on_delete;
                                rsx! {
                                    WatchlistCard {
                                        key: "{watchlist.id}",
                                        watchlist,
                                        strings: strings.clone(),
                                        on_replace: move |symbols| {
                                            replace_cb.call((list_id.clone(), symbols));
                                        },
                                        on_delete: move |_| {
                                            delete_cb.call(del_id.clone());
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

#[allow(dead_code)]
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
    use super::*;

    #[test]
    fn splits_and_trims_symbols() {
        assert_eq!(parse_symbols(" btcusdt, ,ETHUSDT "), ["btcusdt", "ETHUSDT"]);
        assert!(parse_symbols("  ").is_empty());
    }

    #[test]
    fn app_virtual_dom_renders_without_stack_overflow() {
        let mut dom = VirtualDom::new(App);
        dom.rebuild_in_place();
    }
}
