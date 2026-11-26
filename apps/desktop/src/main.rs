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
    AiBriefingQueryDto, AiJobQueryDto, AiJobTaskDto, MarketFundingDto, MarketIntelligenceDto,
    MarketLiquidationDto, MarketMoverDto, MarketOverviewDto, MarketPulseDto, MarketStateDto,
    MarketSymbolDto, NewAiJobDto, NewAiModelDto, NewAiProviderProfileDto, NewAiTaskRouteDto,
    NewsQueryDto, NotificationLogQueryDto, NotificationOutboxQueryDto, NotificationSummaryQueryDto,
    OperationActionDto, OperationResponse, OperationStatusDto, ResearchEventQueryDto,
    SendTestNotificationRequest, ServiceSnapshot, WatchlistDto,
};

use ai_briefings::{AiBriefingPanelState, AiBriefingSlideover};
use ai_providers::{AiProviderPanel, AiProviderPanelState};
use i18n::{Locale, UiStrings};
use news::{NewsItemSlideover, NewsPanel, NewsPanelState};
use notifications::{NotificationCenterPanel, NotificationPanelState};
use signal_center::{ResearchEventSlideover, ResearchPanelState};
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
                .with_decorations(false)
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(1420.0, 900.0))
                .with_min_inner_size(dioxus::desktop::LogicalSize::new(1080.0, 700.0)),
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
    IntelligenceSlideover,
    FundingLiquidationSlideover,
    NewsListSlideover,
    ServicesFleetSlideover,
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
    let mut market_funding = use_signal(|| Option::<MarketFundingDto>::None);
    let mut market_liquidations = use_signal(Vec::<MarketLiquidationDto>::new);
    let mut market_intelligence = use_signal(|| Option::<MarketIntelligenceDto>::None);
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

                let is_bg_tick = tick == 1 || tick.is_multiple_of(5);

                // Market & Research & News & AI Briefings (2s cycle)
                let watchlists_res = client.list_watchlists().await;
                let fallback_symbols: Vec<String> = if watchlists_res
                    .as_ref()
                    .map(|list| list.is_empty())
                    .unwrap_or(true)
                {
                    std::env::var("TM_TRADING_MARKET_SYMBOLS")
                        .unwrap_or_else(|_| "BTCUSDT,ETHUSDT,SOLUSDT".to_owned())
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                } else {
                    Vec::new()
                };
                if let Ok(list) = watchlists_res {
                    watchlists.set(list);
                }
                let (overview_res, pulse_res) = tokio::join!(
                    client.market_overview(&fallback_symbols, "1m", 60),
                    client.market_pulse(&fallback_symbols, "1m", 60)
                );
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
                    let liq_symbols: Vec<String> =
                        fallback_symbols.iter().take(3).cloned().collect();
                    let liq_futs = liq_symbols.iter().map(|sym| client.market_liquidation(sym));
                    let (
                        sources_res,
                        clusters_res,
                        profiles_res,
                        models_res,
                        routes_res,
                        funding_res,
                        liquidations_res,
                        intelligence_res,
                    ) = tokio::join!(
                        client.list_news_sources(),
                        client.list_news_clusters(&cluster_query),
                        client.list_ai_provider_profiles(),
                        client.list_ai_models(),
                        client.list_ai_task_routes(),
                        client.market_funding(),
                        futures_util::future::join_all(liq_futs),
                        client.market_intelligence()
                    );
                    if let Ok(data) = funding_res {
                        market_funding.set(Some(data));
                    }
                    let liq_list: Vec<MarketLiquidationDto> = liquidations_res
                        .into_iter()
                        .filter_map(Result::ok)
                        .collect();
                    if !liq_list.is_empty() {
                        market_liquidations.set(liq_list);
                    }
                    if let Ok(data) = intelligence_res {
                        market_intelligence.set(Some(data));
                    }
                    match sources_res {
                        Ok(list) => news.write().sources_loaded(list),
                        Err(error) => news.write().sources_failed(error.to_string()),
                    }
                    match clusters_res {
                        Ok(list) => news.write().clusters_loaded(list),
                        Err(error) => news.write().clusters_failed(error.to_string()),
                    }
                    match profiles_res {
                        Ok(list) => ai_providers.write().list_loaded(list),
                        Err(error) => ai_providers.write().list_failed(error.to_string()),
                    }
                    match models_res {
                        Ok(list) => ai_providers.write().models_loaded(list),
                        Err(error) => ai_providers.write().models_failed(error.to_string()),
                    }
                    match routes_res {
                        Ok(list) => ai_providers.write().routes_loaded(list),
                        Err(error) => ai_providers.write().routes_failed(error.to_string()),
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
                    match outbox_res {
                        Ok(list) => notifications.write().outbox_loaded(list),
                        Err(error) => notifications.write().outbox_failed(error.to_string()),
                    }
                    match summary_res {
                        Ok(summary) => notifications.write().summary_loaded(summary),
                        Err(error) => notifications.write().summary_failed(error.to_string()),
                    }
                    match logs_res {
                        Ok(logs) => notifications.write().logs_loaded(logs),
                        Err(error) => notifications.write().logs_failed(error.to_string()),
                    }
                    match diag_res {
                        Ok(diag) => system_diagnostics.write().diagnostics_loaded(diag),
                        Err(error) => system_diagnostics
                            .write()
                            .diagnostics_failed(error.to_string()),
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

    // Derived final-state values for immediate display
    let ready_services_count = rows
        .iter()
        .filter(|s| s.readiness == tm_trading_contracts::ReadinessDto::Ready)
        .count();
    let total_services_count = rows.len();
    let pulse_state_label = pulse
        .as_ref()
        .map_or("N/A", |p| market_state_label(p.state, &strings()));
    let outbox_count = notifications_state
        .outbox
        .as_ref()
        .map_or(0, |items| items.len());

    let desktop_ctx = try_consume_context::<dioxus::desktop::DesktopContext>();
    let desktop_drag = desktop_ctx.clone();
    let desktop_dbl = desktop_ctx.clone();
    let desktop_min = desktop_ctx.clone();
    let desktop_max = desktop_ctx.clone();
    let desktop_close = desktop_ctx;

    let movers_count = pulse.as_ref().map_or(0, |p| p.movers.len());
    let pulse_timeframe = pulse.as_ref().map_or("1m", |p| p.timeframe.as_str());
    let latest_signal = research_events.first().cloned();
    let latest_news = news_state
        .items
        .as_ref()
        .and_then(|items| items.first().cloned());
    let latest_briefing = ai_briefings_state
        .briefings
        .as_ref()
        .and_then(|items| items.first().cloned());
    let fleet_pill_class =
        if total_services_count > 0 && ready_services_count == total_services_count {
            "titlebar-fleet ok"
        } else if total_services_count == 0 {
            "titlebar-fleet"
        } else {
            "titlebar-fleet warn"
        };

    rsx! {
        AppStyles {}
        div { class: "shell",
            // Custom Frameless TitleBar (minimalist: brand, final status, tools, window controls)
            header { class: "custom-titlebar",
                div {
                    class: "titlebar-drag-zone",
                    onmousedown: move |_| {
                        if let Some(ctx) = desktop_drag.as_ref() {
                            ctx.drag();
                        }
                    },
                    ondoubleclick: move |_| {
                        if let Some(ctx) = desktop_dbl.as_ref() {
                            ctx.toggle_maximized();
                        }
                    },
                    div { class: "titlebar-brand",
                        span { class: "titlebar-logo", "TM" }
                        span { class: "titlebar-title", "TM TRADING" }
                    }
                    div { class: "titlebar-status",
                        div { class: "connection",
                            span { class: "{connection_dot(&connection_state)}" }
                            span { "{connection_label(&connection_state, &strings())}" }
                        }
                        span { class: "{fleet_pill_class}",
                            "{ready_services_count}/{total_services_count} READY"
                        }
                        span { class: "titlebar-refresh", "{refreshed_label}" }
                    }
                }
                div {
                    class: "titlebar-controls",
                    onmousedown: move |event| event.stop_propagation(),
                    div { class: "titlebar-tools",
                        button {
                            class: "button compact",
                            type: "button",
                            title: "{strings().language}",
                            onclick: move |_| {
                                let next = if locale() == Locale::English {
                                    Locale::Vietnamese
                                } else {
                                    Locale::English
                                };
                                locale.set(next);
                            },
                            if locale() == Locale::English { "EN" } else { "VI" }
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
                    div { class: "window-buttons",
                        button {
                            class: "win-btn",
                            type: "button",
                            title: "Minimize",
                            aria_label: "Minimize",
                            onclick: move |_| {
                                if let Some(ctx) = desktop_min.as_ref() {
                                    ctx.window.set_minimized(true);
                                }
                            },
                            "─"
                        }
                        button {
                            class: "win-btn",
                            type: "button",
                            title: "Maximize / Restore",
                            aria_label: "Maximize",
                            onclick: move |_| {
                                if let Some(ctx) = desktop_max.as_ref() {
                                    ctx.toggle_maximized();
                                }
                            },
                            "□"
                        }
                        button {
                            class: "win-btn win-close",
                            type: "button",
                            title: "Close",
                            aria_label: "Close",
                            onclick: move |_| {
                                if let Some(ctx) = desktop_close.as_ref() {
                                    ctx.close();
                                }
                            },
                            "✕"
                        }
                    }
                }
            }

            // 4-Column Final-State KPI Strip
            section { class: "kpi-bento-grid",
                button {
                    class: "kpi-card interactive",
                    type: "button",
                    onclick: move |_| active_overlay.set(ActiveOverlay::WatchlistModal),
                    div { class: "kpi-card-top",
                        span { class: "kpi-card-label", "{strings().kpi_market}" }
                        span { class: "kpi-card-icon", "📈" }
                    }
                    span { class: "kpi-card-value accent", "{pulse_state_label}" }
                    span { class: "kpi-card-sub", "{movers_count} symbols · {pulse_timeframe}" }
                }
                {
                    let signal_id = latest_signal.as_ref().map(|s| s.id.clone());
                    let signal_val = latest_signal
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |s| s.symbol.clone());
                    let signal_sub = latest_signal.as_ref().map_or_else(
                        || strings().research_events_count.to_owned(),
                        |s| format!("{} · {}", s.method, s.timeframe),
                    );
                    rsx! {
                        button {
                            class: "kpi-card interactive",
                            type: "button",
                            onclick: move |_| {
                                if let Some(event_id) = signal_id.clone() {
                                    research_open_sender.send(UiAction::OpenResearchEvent { event_id });
                                }
                            },
                            div { class: "kpi-card-top",
                                span { class: "kpi-card-label", "{strings().kpi_signals}" }
                                span { class: "kpi-card-icon", "🎯" }
                            }
                            span { class: "kpi-card-value", "{signal_val}" }
                            span { class: "kpi-card-sub", "{signal_sub}" }
                        }
                    }
                }
                {
                    let news_id = latest_news.as_ref().map(|n| n.id.clone());
                    let news_val = latest_news
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |n| n.source_id.clone());
                    let news_sub = latest_news.as_ref().map_or_else(
                        || "Market & macro feed".to_owned(),
                        |n| n.title.clone(),
                    );
                    rsx! {
                        button {
                            class: "kpi-card interactive",
                            type: "button",
                            onclick: move |_| {
                                if let Some(id) = news_id.clone() {
                                    news_open_sender.send(UiAction::OpenNewsItem { news_id: id });
                                }
                            },
                            div { class: "kpi-card-top",
                                span { class: "kpi-card-label", "{strings().kpi_news}" }
                                span { class: "kpi-card-icon", "📰" }
                            }
                            span { class: "kpi-card-value", "{news_val}" }
                            span { class: "kpi-card-sub", "{news_sub}" }
                        }
                    }
                }
                {
                    let briefing_id = latest_briefing.as_ref().map(|b| b.id.clone());
                    let ai_val = latest_briefing
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |b| b.model_id.clone());
                    let ai_sub = format!("Outbox: {outbox_count}");
                    rsx! {
                        button {
                            class: "kpi-card interactive",
                            type: "button",
                            onclick: move |_| {
                                if let Some(id) = briefing_id.clone() {
                                    ai_briefing_open_sender.send(UiAction::OpenAiBriefing { briefing_id: id });
                                } else {
                                    active_overlay.set(ActiveOverlay::AiConfigSlideover);
                                }
                            },
                            div { class: "kpi-card-top",
                                span { class: "kpi-card-label", "{strings().ai_briefing}" }
                                span { class: "kpi-card-icon", "🤖" }
                            }
                            span { class: "kpi-card-value", "{ai_val}" }
                            span { class: "kpi-card-sub", "{ai_sub}" }
                        }
                    }
                }
            }

            // Unified Bento Operations Workspace
            main { class: "content bento-content",
                // Row 1: Live Market Pulse (Left) + Service Control Fleet (Right)
                div { class: "bento-row-top",
                    div { class: "column-primary",
                        section { class: "panel market-panel",
                            div { class: "panel-head",
                                div { class: "panel-head-actions",
                                    h2 { "{strings().market_pulse}" }
                                    if let Some(pulse) = pulse.as_ref() {
                                        span { class: "pill {market_state_class(pulse.state)}",
                                            "{pulse.timeframe} · {market_state_label(pulse.state, &strings())}"
                                        }
                                    }
                                }
                                button {
                                    class: "button compact",
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
                                                    th { "{strings().market_status_column}" }
                                                }
                                            }
                                            tbody {
                                                for mover in pulse.movers.iter().take(8) {
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
                            } else {
                                div { class: "empty",
                                    p { class: "empty-title", "{strings().connecting}" }
                                    p { "{strings().market_empty_hint}" }
                                }
                            }
                        }
                    }
                    div { class: "column-secondary",
                        section { class: "panel",
                            div { class: "panel-head",
                                div { class: "panel-head-actions",
                                    h2 { "{strings().services_and_agents}" }
                                    span { class: "panel-meta", "{ready_services_count}/{total_services_count} READY" }
                                }
                                button {
                                    class: "button compact",
                                    type: "button",
                                    onclick: move |_| active_overlay.set(ActiveOverlay::ServicesFleetSlideover),
                                    "⚙️ {strings().view_details}"
                                }
                            }
                            if rows.is_empty() {
                                div { class: "empty",
                                    p { class: "empty-title", "{strings().no_services}" }
                                    p { "{strings().no_services_hint}" }
                                }
                            } else {
                                div { class: "service-chip-grid",
                                    for service in rows.iter() {
                                        {
                                            let inst_id = service.service_instance_id.clone();
                                            let short_name = short_service_name(&service.service_definition_id);
                                            let dot_class = match (service.observed_state, service.readiness) {
                                                (
                                                    tm_trading_contracts::ObservedStateDto::Running,
                                                    tm_trading_contracts::ReadinessDto::Ready,
                                                ) => "dot connected",
                                                (
                                                    tm_trading_contracts::ObservedStateDto::Running
                                                    | tm_trading_contracts::ObservedStateDto::Starting
                                                    | tm_trading_contracts::ObservedStateDto::Stopping,
                                                    _,
                                                ) => "dot connecting",
                                                _ => "dot disconnected",
                                            };
                                            let tooltip = format!(
                                                "{} ({} / {})",
                                                service.service_definition_id,
                                                service.observed_state.as_str(),
                                                service.readiness.as_str()
                                            );
                                            rsx! {
                                                button {
                                                    key: "{inst_id}",
                                                    class: "service-chip",
                                                    type: "button",
                                                    title: "{tooltip}",
                                                    onclick: move |_| {
                                                        active_overlay.set(ActiveOverlay::ServiceDetailsSlideover(inst_id.clone()));
                                                    },
                                                    span { class: "{dot_class}" }
                                                    span { class: "service-chip-name", "{short_name}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        section { class: "panel",
                            div { class: "panel-head",
                                div { class: "panel-head-actions",
                                    h2 { "{strings().funding_liquidation_title}" }
                                    if !market_liquidations.read().is_empty() {
                                        span { class: "panel-meta",
                                            "{market_liquidations.read().len()} symbols"
                                        }
                                    }
                                }
                                button {
                                    class: "button compact",
                                    type: "button",
                                    onclick: move |_| active_overlay.set(ActiveOverlay::FundingLiquidationSlideover),
                                    "📊 {strings().view_details}"
                                }
                            }
                            div { class: "funding-liq-grid",
                                div { class: "funding-liq-item",
                                    span { class: "kpi-card-label", "{strings().top_funding_extremes}" }
                                    if let Some(f) = market_funding.read().as_ref() {
                                        {
                                            let top_list: Vec<String> = f
                                                .extremes
                                                .iter()
                                                .chain(f.snapshot.iter())
                                                .take(3)
                                                .map(|r| format!("{} {:+.2}%", r.symbol, r.pct))
                                                .collect();
                                            let headline = if top_list.is_empty() {
                                                "—".to_owned()
                                            } else {
                                                top_list.join(" · ")
                                            };
                                            rsx! {
                                                span { class: "latest-card-title", "{headline}" }
                                                span { class: "latest-card-meta",
                                                    "{f.extremes.len()} extremes · {f.snapshot.len()} pairs"
                                                }
                                            }
                                        }
                                    } else {
                                        span { class: "latest-card-meta", "—" }
                                    }
                                }
                                div { class: "funding-liq-item",
                                    span { class: "kpi-card-label", "{strings().open_interest_zones}" }
                                    if !market_liquidations.read().is_empty() {
                                        for liq in market_liquidations.read().iter() {
                                            {
                                                let oi_b = liq.open_interest_usd / 1_000_000_000.0;
                                                let long_z = liq.zones.iter().find(|z| z.side.eq_ignore_ascii_case("long"));
                                                let short_z = liq.zones.iter().find(|z| z.side.eq_ignore_ascii_case("short"));
                                                let row_text = format!(
                                                    "{}: OI ${oi_b:.2}B · L {} · S {}",
                                                    liq.symbol,
                                                    long_z.map_or_else(|| "—".to_owned(), |z| format!("${:.0}", z.price)),
                                                    short_z.map_or_else(|| "—".to_owned(), |z| format!("${:.0}", z.price)),
                                                );
                                                rsx! {
                                                    span { class: "latest-card-meta", "{row_text}" }
                                                }
                                            }
                                        }
                                    } else {
                                        span { class: "latest-card-meta", "—" }
                                    }
                                }
                            }
                        }
                        section { class: "panel",
                            div { class: "panel-head",
                                div { class: "panel-head-actions",
                                    h2 { "{strings().market_intelligence_title}" }
                                    if let Some(intel) = market_intelligence.read().as_ref() {
                                        span { class: "panel-meta",
                                            "{intel.regime.season.to_uppercase()} {strings().season_suffix}"
                                        }
                                    }
                                }
                                button {
                                    class: "button compact",
                                    type: "button",
                                    onclick: move |_| active_overlay.set(ActiveOverlay::IntelligenceSlideover),
                                    "📊 {strings().view_details}"
                                }
                            }
                            div { class: "funding-liq-grid",
                                div { class: "funding-liq-item",
                                    span { class: "kpi-card-label", "{strings().regime_sentiment_title}" }
                                    if let Some(intel) = market_intelligence.read().as_ref() {
                                        {
                                            let fng_text = match (
                                                intel.regime.fng_value,
                                                intel.regime.fng_classification.as_deref(),
                                            ) {
                                                (Some(v), Some(c)) => format!("F&G {v} ({c})"),
                                                (Some(v), None) => format!("F&G {v}"),
                                                _ => "F&G —".to_owned(),
                                            };
                                            let dom_text = format!(
                                                "BTC.D: {} · ASI: {}",
                                                intel.regime.btc_dominance.map_or_else(|| "—".to_owned(), |d| format!("{d:.1}%")),
                                                intel.regime.asi_d90.map_or_else(|| "—".to_owned(), |a| a.to_string()),
                                            );
                                            rsx! {
                                                span { class: "latest-card-title", "{fng_text}" }
                                                span { class: "latest-card-meta", "{dom_text}" }
                                            }
                                        }
                                    } else {
                                        span { class: "latest-card-meta", "—" }
                                    }
                                }
                                div { class: "funding-liq-item",
                                    span { class: "kpi-card-label", "{strings().confluence_mover_title}" }
                                    if let Some(intel) = market_intelligence.read().as_ref() {
                                        {
                                            let conf_text = intel.confluence.first().map_or_else(
                                                || "—".to_owned(),
                                                |c| format!("{} {:+.2} ({})", c.symbol, c.total_score, c.bias),
                                            );
                                            let mover_text = intel.scanner_movers.first().map_or_else(
                                                || "Scanner: —".to_owned(),
                                                |m| format!("Top 24h: {} ({:+.2}%)", m.symbol, m.pct_24h),
                                            );
                                            let cross_text = intel.cross_scans.first().map_or_else(
                                                || "Cross: —".to_owned(),
                                                |c| format!("{} {}: {:.3}%", c.instrument, c.pair, c.rel_spread_pct),
                                            );
                                            let macro_text = intel.macro_series.first().map_or_else(
                                                || format!("{}: {}", strings().alerts_matched_label, intel.alert_evaluation.matched_events),
                                                |m| format!("{}: {} {} · {}: {}", m.series_id, m.value, m.unit, strings().alerts_matched_label, intel.alert_evaluation.matched_events),
                                            );
                                            let platform_text = intel.platform.reconcile.first().map_or_else(
                                                || format!("{}: {} · {}: {}d", strings().extract_eligible_label, intel.platform.news_extract_eligible, strings().retention_label, intel.platform.retention_days),
                                                |r| format!("{} {}: {} · {}: {}", strings().reconcile_label, r.symbol, r.verdict.to_uppercase(), strings().extract_eligible_label, intel.platform.news_extract_eligible),
                                            );
                                            rsx! {
                                                span { class: "latest-card-title", "{conf_text}" }
                                                span { class: "latest-card-meta", "{mover_text}" }
                                                span { class: "latest-card-meta", "{cross_text} · {macro_text}" }
                                                span { class: "latest-card-meta", "{platform_text}" }
                                            }
                                        }
                                    } else {
                                        span { class: "latest-card-meta", "—" }
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
                }

                // Row 2: Latest Final Snapshots (Latest Signal + Latest News + Latest AI Briefing)
                div { class: "bento-row-latest",
                    section { class: "panel",
                        div { class: "panel-head",
                            h2 { "{strings().signal_center}" }
                            if let Some(sig) = latest_signal.as_ref() {
                                span { class: "panel-meta", "{sig.timeframe} · {sig.method}" }
                            }
                        }
                        if let Some(sig) = latest_signal.clone() {
                            {
                                let sig_id = sig.id.clone();
                                let dir_label = match sig.direction {
                                    1 => "BULL",
                                    -1 => "BEAR",
                                    _ => "NEUTRAL",
                                };
                                let dir_class = match sig.direction {
                                    1 => "pill state-ok",
                                    -1 => "pill state-failed",
                                    _ => "pill state-partial",
                                };
                                let observed_time = sig.observed_at.format("%H:%M:%S UTC");
                                rsx! {
                                    div {
                                        class: "latest-card-body interactive",
                                        onclick: move |_| {
                                            research_open_sender.send(UiAction::OpenResearchEvent {
                                                event_id: sig_id.clone(),
                                            });
                                        },
                                        div { class: "latest-card-header",
                                            span { class: "latest-card-title", "{sig.symbol} · {sig.event_type}" }
                                            span { class: "{dir_class}", "{dir_label}" }
                                        }
                                        p { class: "latest-card-meta",
                                            "Score: {sig.score:.2} · {observed_time}"
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "latest-card-empty", "—" }
                        }
                    }

                    section { class: "panel",
                        div { class: "panel-head",
                            div { class: "panel-head-actions",
                                h2 { "{strings().news_events}" }
                                if let Some(item) = latest_news.as_ref() {
                                    span { class: "panel-meta", "{item.source_id}" }
                                }
                            }
                            button {
                                class: "button compact",
                                type: "button",
                                onclick: move |_| active_overlay.set(ActiveOverlay::NewsListSlideover),
                                "📰 {strings().view_details}"
                            }
                        }
                        if let Some(err) = news_state.list_error.as_ref() {
                            div { class: "latest-card-empty error-text", "{err}" }
                        } else if let Some(item) = latest_news.clone() {
                            {
                                let item_id = item.id.clone();
                                let ingested_time = item.ingested_at.format("%H:%M:%S UTC");
                                rsx! {
                                    div {
                                        class: "latest-card-body interactive",
                                        onclick: move |_| {
                                            news_open_sender.send(UiAction::OpenNewsItem {
                                                news_id: item_id.clone(),
                                            });
                                        },
                                        div { class: "latest-card-header",
                                            span { class: "latest-card-title", "{item.title}" }
                                        }
                                        p { class: "latest-card-meta",
                                            "{item.market_group} · {ingested_time}"
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "latest-card-empty", "—" }
                        }
                    }

                    section { class: "panel",
                        div { class: "panel-head",
                            h2 { "{strings().ai_briefing}" }
                            if let Some(sig) = latest_signal.clone() {
                                button {
                                    class: "button compact",
                                    type: "button",
                                    onclick: move |_| {
                                        ai_briefing_request_sender.send(UiAction::RequestAiBriefing {
                                            research_event_id: sig.id.clone(),
                                        });
                                    },
                                    "✨ {strings().ai_briefing_request_button}"
                                }
                            }
                        }
                        if let Some(briefing) = latest_briefing.clone() {
                            {
                                let b_id = briefing.id.clone();
                                let generated_time = briefing.generated_at.format("%H:%M:%S UTC");
                                let headline = briefing
                                    .facts
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| briefing.model_id.clone());
                                rsx! {
                                    div {
                                        class: "latest-card-body interactive",
                                        onclick: move |_| {
                                            ai_briefing_open_sender.send(UiAction::OpenAiBriefing {
                                                briefing_id: b_id.clone(),
                                            });
                                        },
                                        div { class: "latest-card-header",
                                            span { class: "latest-card-title", "{headline}" }
                                        }
                                        p { class: "latest-card-meta",
                                            "{briefing.model_id} · {generated_time}"
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "latest-card-empty", "—" }
                        }
                    }
                }
            }

            // Overlays: Deep Configuration & Operations (Modals & Slideovers)
            if active_overlay() == ActiveOverlay::NewsListSlideover {
                Slideover {
                    title: strings().news_events.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    NewsPanel {
                        state: news_state.clone(),
                        strings: strings(),
                        on_open: move |news_id| {
                            news_open_sender.send(UiAction::OpenNewsItem { news_id });
                        },
                    }
                }
            }
            if active_overlay() == ActiveOverlay::FundingLiquidationSlideover {
                Slideover {
                    title: strings().funding_liquidation_title.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    div { class: "research-detail",
                        div { class: "card",
                            h3 { "{strings().top_funding_extremes}" }
                            if let Some(f) = market_funding.read().as_ref() {
                                div { class: "table-wrap",
                                    table { class: "services",
                                        thead {
                                            tr {
                                                th { "{strings().symbol_col}" }
                                                th { class: "numeric", "{strings().funding_col} %" }
                                                th { class: "numeric", "{strings().price_col}" }
                                            }
                                        }
                                        tbody {
                                            for r in f.extremes.iter().chain(f.snapshot.iter()).take(10) {
                                                tr {
                                                    td { div { class: "service-name", "{r.symbol}" } }
                                                    td { class: "numeric", "{r.pct:+.4}%" }
                                                    td { class: "numeric", "${r.price:.4}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                p { class: "muted", "—" }
                            }
                        }
                        for liq in market_liquidations.read().iter() {
                            div { class: "card",
                                h3 { "{liq.symbol} — ${liq.mark_price:.2} (OI: ${liq.open_interest_usd / 1_000_000_000.0:.2}B)" }
                                div { class: "table-wrap",
                                    table { class: "services",
                                        thead {
                                            tr {
                                                th { "{strings().side_col}" }
                                                th { class: "numeric", "{strings().leverage_col}" }
                                                th { class: "numeric", "{strings().liq_price_col}" }
                                                th { class: "numeric", "{strings().distance_col}" }
                                            }
                                        }
                                        tbody {
                                            for z in liq.zones.iter().take(8) {
                                                tr {
                                                    td { "{z.side}" }
                                                    td { class: "numeric", "{z.lev:.0}x" }
                                                    td { class: "numeric", "${z.price:.2}" }
                                                    td { class: "numeric", "{z.pct:+.2}%" }
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
            if active_overlay() == ActiveOverlay::IntelligenceSlideover {
                Slideover {
                    title: strings().market_intelligence_title.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
                    if let Some(intel) = market_intelligence.read().as_ref() {
                        div { class: "research-detail",
                            div { class: "card",
                                h3 { "{strings().confluence_rankings_title}" }
                                if intel.confluence.is_empty() {
                                    p { class: "muted", "—" }
                                } else {
                                    div { class: "table-wrap",
                                        table { class: "services",
                                            thead {
                                                tr {
                                                    th { "{strings().symbol_col}" }
                                                    th { class: "numeric", "{strings().method_col}" }
                                                    th { class: "numeric", "{strings().regime_col}" }
                                                    th { class: "numeric", "{strings().funding_col}" }
                                                    th { class: "numeric", "{strings().total_col}" }
                                                    th { "{strings().bias_col}" }
                                                }
                                            }
                                            tbody {
                                                for row in intel.confluence.iter() {
                                                    tr {
                                                        td { div { class: "service-name", "{row.symbol}" } }
                                                        td { class: "numeric", "{row.method_score:+.2}" }
                                                        td { class: "numeric", "{row.regime_score:+.2}" }
                                                        td { class: "numeric", "{row.funding_score:+.2}" }
                                                        td { class: "numeric", "{row.total_score:+.2}" }
                                                        td { span { class: "pill", "{row.bias}" } }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "card",
                                h3 { "{strings().scanner_movers_title}" }
                                if intel.scanner_movers.is_empty() {
                                    p { class: "muted", "—" }
                                } else {
                                    div { class: "table-wrap",
                                        table { class: "services",
                                            thead {
                                                tr {
                                                    th { "{strings().symbol_col}" }
                                                    th { class: "numeric", "{strings().price_col}" }
                                                    th { class: "numeric", "{strings().change_24h_col}" }
                                                    th { class: "numeric", "{strings().volume_usd_col}" }
                                                }
                                            }
                                            tbody {
                                                for m in intel.scanner_movers.iter() {
                                                    tr {
                                                        td { div { class: "service-name", "{m.symbol}" } }
                                                        td { class: "numeric", "{m.last_price:.4}" }
                                                        td { class: "numeric", "{m.pct_24h:+.2}%" }
                                                        td { class: "numeric", "${m.quote_volume / 1_000_000.0:.1}M" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "card",
                                h3 { "{strings().cross_platform_title}" }
                                if !intel.cross_scans.is_empty() {
                                    div { class: "table-wrap",
                                        table { class: "services",
                                            thead {
                                                tr {
                                                    th { "{strings().instrument_col}" }
                                                    th { "{strings().pair_col}" }
                                                    th { class: "numeric", "{strings().abs_spread_col}" }
                                                    th { class: "numeric", "{strings().rel_spread_col}" }
                                                }
                                            }
                                            tbody {
                                                for c in intel.cross_scans.iter() {
                                                    tr {
                                                        td { "{c.instrument}" }
                                                        td { "{c.pair}" }
                                                        td { class: "numeric", "{c.abs_spread:.2}" }
                                                        td { class: "numeric", "{c.rel_spread_pct:.3}%" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                p { class: "muted",
                                    "{strings().alerts_matched_label}: {intel.alert_evaluation.matched_events} · {strings().extract_eligible_label}: {intel.platform.news_extract_eligible} · {strings().retention_label}: {intel.platform.retention_days}d"
                                }
                                for r in intel.platform.reconcile.iter() {
                                    p { class: "muted",
                                        "{strings().reconcile_label} {r.symbol} ({r.timeframe}): {r.verdict} (checked={r.checked}, mismatched={r.mismatched}, missing={r.missing_local})"
                                    }
                                }
                                for m in intel.macro_series.iter() {
                                    p { class: "muted",
                                        "Macro {m.series_id} ({m.date}): {m.value} {m.unit}"
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "empty",
                            p { class: "empty-title", "{strings().connecting}" }
                        }
                    }
                }
            }
            if active_overlay() == ActiveOverlay::ServicesFleetSlideover {
                Slideover {
                    title: strings().services_and_agents.to_owned(),
                    close_label: strings().modal_close.to_owned(),
                    wide: true,
                    on_close: move |_| active_overlay.set(ActiveOverlay::None),
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
                                        th { "{strings().observed_column}" }
                                        th { "{strings().readiness_column}" }
                                        th { class: "numeric", "{strings().restarts_column}" }
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
            }
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
    let change_class = match mover.change_percent {
        Some(v) if v > 0.0 => "numeric delta-up",
        Some(v) if v < 0.0 => "numeric delta-down",
        _ => "numeric delta-flat",
    };
    let change_text = mover.change_percent.map_or_else(
        || "-".to_owned(),
        |v| {
            if v > 0.0 {
                format!("+{v:.2}%")
            } else {
                format!("{v:.2}%")
            }
        },
    );
    rsx! {
        tr {
            td { div { class: "service-name", "{mover.symbol}" } }
            td { class: "numeric", "{format_number(mover.last_close)}" }
            td { class: "{change_class}", "{change_text}" }
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
    let observed = service.observed_state.as_str();
    let readiness = service.readiness.as_str();
    let observed_class = format!("pill state-{}", observed.to_lowercase());
    let readiness_class = format!("pill state-{}", readiness.to_lowercase());
    let restart_count = service.restart_count;
    let definition_id = service.service_definition_id.clone();
    let instance_id = service.service_instance_id.clone();
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
            td { span { class: observed_class, "{observed}" } }
            td { span { class: readiness_class, "{readiness}" } }
            td { class: "numeric", "{restart_count}" }
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
                                        strings,
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
fn short_service_name(definition_id: &str) -> &str {
    let trimmed = definition_id
        .strip_prefix("tm-trading-")
        .unwrap_or(definition_id);
    let trimmed = trimmed.strip_suffix("-worker").unwrap_or(trimmed);
    if trimmed.is_empty() {
        definition_id
    } else {
        trimmed
    }
}

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
