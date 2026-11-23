use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tm_trading_contracts::{
    NotificationChannelDto, NotificationLogItemDto, NotificationLogListDto,
    NotificationOutboxItemDto, NotificationOutboxListDto, NotificationOutboxSummaryDto,
    NotificationPriorityDto, NotificationStateDto, SendTestNotificationResponse,
};

use crate::i18n::UiStrings;

/// UI state for the Notification Center panel.
#[derive(Clone, Default, PartialEq)]
pub struct NotificationPanelState {
    pub summary: Option<NotificationOutboxSummaryDto>,
    pub summary_error: Option<String>,
    pub outbox: Option<Vec<NotificationOutboxItemDto>>,
    pub outbox_error: Option<String>,
    pub logs: Option<Vec<NotificationLogItemDto>>,
    pub logs_error: Option<String>,
    pub test_outcome: Option<SendTestNotificationResponse>,
    pub test_error: Option<String>,
}

impl NotificationPanelState {
    pub fn summary_loaded(&mut self, summary: NotificationOutboxSummaryDto) {
        self.summary = Some(summary);
        self.summary_error = None;
    }

    pub fn summary_failed(&mut self, error: String) {
        self.summary_error = Some(error);
    }

    pub fn outbox_loaded(&mut self, list: NotificationOutboxListDto) {
        self.outbox = Some(list.items);
        self.outbox_error = None;
    }

    pub fn outbox_failed(&mut self, error: String) {
        self.outbox_error = Some(error);
    }

    pub fn logs_loaded(&mut self, list: NotificationLogListDto) {
        self.logs = Some(list.logs);
        self.logs_error = None;
    }

    pub fn logs_failed(&mut self, error: String) {
        self.logs_error = Some(error);
    }

    pub fn test_completed(&mut self, response: SendTestNotificationResponse) {
        self.test_outcome = Some(response);
        self.test_error = None;
    }

    pub fn test_failed(&mut self, error: String) {
        self.test_error = Some(error);
    }
}

/// Notification Center panel displaying outbox stats, test ping form, outbox queue, and audit ledger.
#[component]
pub fn NotificationCenterPanel(
    state: NotificationPanelState,
    strings: UiStrings,
    on_send_test: EventHandler<(Option<String>, Option<String>)>,
) -> Element {
    let mut custom_chat_id = use_signal(String::new);
    let mut custom_message = use_signal(String::new);

    let summary = state.summary.as_ref();
    let queued_count = summary.map_or(0, |s| s.queued);
    let sent_count = summary.map_or(0, |s| s.sent);
    let accepted_count = summary.map_or(0, |s| s.accepted);
    let failed_count = summary.map_or(0, |s| s.failed);
    let dead_count = summary.map_or(0, |s| s.dead);

    let chat_id_val = custom_chat_id();
    let msg_val = custom_message();

    rsx! {
        section { class: "panel notification-panel",
            div { class: "panel-head signal-center-head",
                div {
                    h2 { "{strings.notifications}" }
                    p { class: "panel-subtitle", "{strings.notifications_hint}" }
                }
                div { class: "signal-summary", aria_label: "{strings.notifications_summary}",
                    span { strong { "{queued_count}" } " {strings.notifications_queued_count}" }
                    span { strong { "{sent_count}" } " {strings.notifications_sent_count}" }
                    span { strong { "{accepted_count}" } " {strings.notifications_accepted_count}" }
                    span { strong { "{failed_count}" } " {strings.notifications_failed_count}" }
                    span { strong { "{dead_count}" } " {strings.notifications_dead_count}" }
                }
            }

            if let Some(error) = state.summary_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            if let Some(error) = state.outbox_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            if let Some(error) = state.logs_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }
            if let Some(error) = state.test_error.as_ref() {
                div { class: "banner error", role: "alert", "{error}" }
            }

            // Test Notification Action Box
            div { class: "watchlist-create", style: "margin-bottom: 20px;",
                input {
                    class: "text-input",
                    style: "max-width: 280px;",
                    value: "{chat_id_val}",
                    placeholder: "{strings.notifications_test_chat_placeholder}",
                    aria_label: "{strings.notifications_test_chat_placeholder}",
                    oninput: move |event| custom_chat_id.set(event.value()),
                }
                input {
                    class: "text-input",
                    style: "flex: 1;",
                    value: "{msg_val}",
                    placeholder: "{strings.notifications_test_msg_placeholder}",
                    aria_label: "{strings.notifications_test_msg_placeholder}",
                    oninput: move |event| custom_message.set(event.value()),
                }
                button {
                    class: "button primary",
                    onclick: move |_| {
                        let chat = custom_chat_id().trim().to_owned();
                        let chat_opt = if chat.is_empty() { None } else { Some(chat) };
                        let msg = custom_message().trim().to_owned();
                        let msg_opt = if msg.is_empty() { None } else { Some(msg) };
                        on_send_test.call((chat_opt, msg_opt));
                    },
                    "{strings.notifications_test_send}"
                }
                if let Some(outcome) = state.test_outcome.as_ref() {
                    if outcome.ok {
                        span { class: "pill ok",
                            "{strings.notifications_test_delivered} (ID: {outcome.message_id.as_deref().unwrap_or(\"-\")})"
                        }
                    } else {
                        span { class: "pill danger",
                            "{strings.notifications_test_failed}: {outcome.error.as_deref().unwrap_or(\"error\")}"
                        }
                    }
                }
            }

            // Outbox Section
            div { class: "panel-head news-sources-head", style: "padding: 12px 0;",
                div {
                    h3 { "{strings.notifications_outbox_section}" }
                }
            }
            match state.outbox.as_ref() {
                Some(items) if items.is_empty() => rsx! {
                    div { class: "empty", style: "padding: 18px 0;",
                        p { class: "empty-title", "{strings.notifications_outbox_empty}" }
                    }
                },
                Some(items) => rsx! {
                    div { class: "table-wrap", style: "margin-bottom: 24px;",
                        table { class: "services",
                            thead {
                                tr {
                                    th { "{strings.notifications_col_id}" }
                                    th { "{strings.notifications_col_channel}" }
                                    th { "{strings.notifications_col_priority}" }
                                    th { "{strings.notifications_col_kind}" }
                                    th { "{strings.notifications_col_title}" }
                                    th { "{strings.notifications_col_state}" }
                                    th { class: "numeric", "{strings.notifications_col_attempts}" }
                                    th { "{strings.notifications_col_time}" }
                                }
                            }
                            tbody {
                                for item in items.iter() {
                                    tr { class: "signal-row",
                                        td {
                                            div { class: "signal-open", "{item.notify_id}" }
                                            if let Some(owner) = &item.owner_id {
                                                div { class: "signal-context", "Owner: {owner}" }
                                            }
                                        }
                                        td {
                                            span { class: "timeframe-chip", "{channel_label(item.channel)}" }
                                        }
                                        td {
                                            span { class: "pill {priority_class(item.priority)}",
                                                "{priority_label(item.priority)}"
                                            }
                                        }
                                        td {
                                            span { class: "chip", "{item.kind}" }
                                        }
                                        td {
                                            div { style: "font-weight: 500;", "{item.title.as_deref().unwrap_or(\"-\")}" }
                                            div { class: "signal-context", "{item.body.as_deref().unwrap_or(\"\")}" }
                                            if let Some(last_err) = &item.last_error {
                                                div { class: "error-text", style: "font-size: 11px; margin-top: 4px;", "Error: {last_err}" }
                                            }
                                        }
                                        td {
                                            span { class: "pill {state_class(item.state, item.dead)}",
                                                "{state_label(item.state, item.dead)}"
                                            }
                                        }
                                        td { class: "numeric", "{item.attempts}" }
                                        td { class: "signal-time", "{format_timestamp(item.at)}" }
                                    }
                                }
                            }
                        }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.loading}" }
                    }
                },
            }

            // Logs Section
            div { class: "panel-head news-sources-head", style: "padding: 12px 0;",
                div {
                    h3 { "{strings.notifications_logs_section}" }
                }
            }
            match state.logs.as_ref() {
                Some(logs) if logs.is_empty() => rsx! {
                    div { class: "empty", style: "padding: 18px 0;",
                        p { class: "empty-title", "{strings.notifications_logs_empty}" }
                    }
                },
                Some(logs) => rsx! {
                    div { class: "table-wrap",
                        table { class: "services",
                            thead {
                                tr {
                                    th { "{strings.notifications_col_time}" }
                                    th { "{strings.notifications_col_id}" }
                                    th { "{strings.notifications_col_priority}" }
                                    th { "{strings.notifications_col_kind}" }
                                    th { "{strings.notifications_col_channel}" }
                                    th { "{strings.notifications_col_title}" }
                                }
                            }
                            tbody {
                                for log in logs.iter() {
                                    tr { class: "signal-row",
                                        td { class: "signal-time", "{format_timestamp(log.sent_at)}" }
                                        td {
                                            div { class: "signal-open", "{log.notify_id}" }
                                        }
                                        td {
                                            span { class: "pill {priority_class(log.priority)}",
                                                "{priority_label(log.priority)}"
                                            }
                                        }
                                        td {
                                            span { class: "chip", "{log.kind}" }
                                        }
                                        td {
                                            div { class: "chips",
                                                for ch in log.delivered.iter() {
                                                    span { class: "chip", "{channel_label(*ch)}" }
                                                }
                                            }
                                        }
                                        td {
                                            div { style: "font-weight: 500;", "{log.title.as_deref().unwrap_or(\"-\")}" }
                                            div { class: "signal-context", "{log.body.as_deref().unwrap_or(\"\")}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                None => rsx! {
                    div { class: "empty loading-state", role: "status",
                        p { class: "empty-title", "{strings.loading}" }
                    }
                },
            }
        }
    }
}

fn channel_label(channel: NotificationChannelDto) -> &'static str {
    match channel {
        NotificationChannelDto::Telegram => "Telegram",
        NotificationChannelDto::WebPush => "Web Push",
        NotificationChannelDto::Electron => "Electron",
    }
}

fn priority_class(priority: NotificationPriorityDto) -> &'static str {
    match priority {
        NotificationPriorityDto::Info => "idle",
        NotificationPriorityDto::Normal => "info",
        NotificationPriorityDto::Important => "warn",
        NotificationPriorityDto::Critical => "danger",
    }
}

fn priority_label(priority: NotificationPriorityDto) -> &'static str {
    match priority {
        NotificationPriorityDto::Info => "INFO",
        NotificationPriorityDto::Normal => "NORMAL",
        NotificationPriorityDto::Important => "IMPORTANT",
        NotificationPriorityDto::Critical => "CRITICAL",
    }
}

fn state_class(state: NotificationStateDto, dead: bool) -> &'static str {
    if dead {
        return "danger";
    }
    match state {
        NotificationStateDto::Queued => "idle",
        NotificationStateDto::Sent => "info",
        NotificationStateDto::Accepted => "ok",
        NotificationStateDto::Failed => "danger",
    }
}

fn state_label(state: NotificationStateDto, dead: bool) -> &'static str {
    if dead {
        return "DEAD";
    }
    match state {
        NotificationStateDto::Queued => "QUEUED",
        NotificationStateDto::Sent => "SENT",
        NotificationStateDto::Accepted => "ACCEPTED",
        NotificationStateDto::Failed => "FAILED",
    }
}

fn format_timestamp(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
