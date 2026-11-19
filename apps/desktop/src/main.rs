// A release build is a Windows GUI application: opening it must not allocate a console
// window. Debug builds keep the console so startup logs stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;

use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use futures_util::StreamExt;
use tm_trading_api_client::{ManagementApiClient, new_idempotency_key};
use tm_trading_contracts::{
    OperationActionDto, OperationResponse, OperationStatusDto, ServiceSnapshot,
};

use i18n::{Locale, UiStrings};

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

    let client_task = client.clone();
    let actions = use_coroutine(move |mut actions: UnboundedReceiver<UiAction>| {
        let client = client_task.clone();
        // Signals are Copy handles; shadowing keeps the coroutine body reading naturally.
        let (mut services, mut connection) = (services, connection);
        let (mut operation, mut command_error) = (operation, command_error);
        let (mut refreshed_at, mut pending_operation) = (refreshed_at, pending_operation);
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
    let refreshed_label = format!(
        "{} {}",
        strings().refreshed_at,
        refreshed_at().unwrap_or_else(|| "-".to_owned())
    );
    let connection_state = connection();
    let current_operation = operation();
    let pending = pending_operation().is_some();
    let action_sender = actions;
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
