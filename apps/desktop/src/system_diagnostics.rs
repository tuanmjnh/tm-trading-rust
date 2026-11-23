//! System Diagnostics and Support Bundle panel for desktop control center.

use dioxus::prelude::*;
use tm_trading_contracts::{SupportBundleDto, SystemDiagnosticsDto};

use crate::i18n::UiStrings;
use crate::webview2_check::WebView2RuntimeStatus;

#[derive(Clone, Default, PartialEq)]
pub struct SystemDiagnosticsPanelState {
    pub diagnostics: Option<SystemDiagnosticsDto>,
    pub webview2: WebView2RuntimeStatus,
    pub bundle: Option<SupportBundleDto>,
    pub is_generating_bundle: bool,
    pub error: Option<String>,
}

impl SystemDiagnosticsPanelState {
    pub fn new(webview2: WebView2RuntimeStatus) -> Self {
        Self {
            diagnostics: None,
            webview2,
            bundle: None,
            is_generating_bundle: false,
            error: None,
        }
    }

    pub fn diagnostics_loaded(&mut self, diagnostics: SystemDiagnosticsDto) {
        self.diagnostics = Some(diagnostics);
        self.error = None;
    }

    pub fn diagnostics_failed(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn bundle_generating(&mut self) {
        self.is_generating_bundle = true;
        self.error = None;
    }

    pub fn bundle_loaded(&mut self, bundle: SupportBundleDto) {
        self.bundle = Some(bundle);
        self.is_generating_bundle = false;
        self.error = None;
    }

    pub fn bundle_failed(&mut self, error: String) {
        self.is_generating_bundle = false;
        self.error = Some(error);
    }
}

#[component]
pub fn SystemDiagnosticsPanel(
    state: SystemDiagnosticsPanelState,
    strings: UiStrings,
    disabled: bool,
    on_generate_bundle: EventHandler<()>,
) -> Element {
    let webview2 = state.webview2;
    let diag = state.diagnostics;
    let bundle = state.bundle;
    let is_generating = state.is_generating_bundle;
    let error_msg = state.error;

    rsx! {
        section { class: "panel",
            div { class: "panel-head",
                h2 { "{strings.system_diagnostics_title}" }
                span { class: "panel-meta", "{strings.system_diagnostics_meta}" }
            }

            if let Some(error) = error_msg {
                div { class: "banner error",
                    p { "{error}" }
                }
            }

            // Cards row: WebView2 & Platform
            div { class: "diagnostics-grid",
                // WebView2 card
                div { class: "diag-card",
                    div { class: "diag-card-header",
                        h3 { "{strings.webview2_runtime_title}" }
                        if webview2.detected && webview2.meets_minimum {
                            span { class: "badge success", "{strings.status_healthy}" }
                        } else if webview2.detected {
                            span { class: "badge warning", "{strings.status_outdated}" }
                        } else {
                            span { class: "badge danger", "{strings.status_missing}" }
                        }
                    }
                    div { class: "diag-card-body",
                        div { class: "kv-pair",
                            span { class: "kv-label", "{strings.webview2_version}:" }
                            span { class: "kv-value", "{webview2.version.as_deref().unwrap_or(\"-\")}" }
                        }
                        div { class: "kv-pair",
                            span { class: "kv-label", "{strings.webview2_type}:" }
                            span { class: "kv-value", "{webview2.install_type}" }
                        }
                        div { class: "kv-pair",
                            span { class: "kv-label", "{strings.webview2_source}:" }
                            span { class: "kv-value", "{webview2.source}" }
                        }
                        p { class: "diag-advisory", "{webview2.advisory}" }
                    }
                }

                // Server Runtime Diagnostics Card
                div { class: "diag-card",
                    div { class: "diag-card-header",
                        h3 { "{strings.server_runtime_title}" }
                        if diag.is_some() {
                            span { class: "badge success", "{strings.status_online}" }
                        } else {
                            span { class: "badge warning", "{strings.connecting}" }
                        }
                    }
                    div { class: "diag-card-body",
                        if let Some(d) = diag.as_ref() {
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.server_version}:" }
                                span { class: "kv-value", "{d.version.service_version}" }
                            }
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.server_environment}:" }
                                span { class: "kv-value", "{d.os}/{d.arch}" }
                            }
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.server_uptime}:" }
                                span { class: "kv-value", "{d.uptime_seconds} {strings.seconds_unit}" }
                            }
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.storage_engine}:" }
                                span { class: "kv-value", "{d.storage.storage_kind} ({d.storage.database_name.as_deref().unwrap_or(\"-\")})" }
                            }
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.ai_workers}:" }
                                span { class: "kv-value", "{d.workers.len()} {strings.logs_unit}" }
                            }
                            div { class: "kv-pair",
                                span { class: "kv-label", "{strings.notifications_label}:" }
                                span { class: "kv-value", "{d.notifications.outbox_queued} {strings.pending_label}, {d.notifications.outbox_dead} dead" }
                            }
                        } else {
                            p { class: "meta-subtle", "{strings.loading_server_diagnostics}" }
                        }
                    }
                }
            }

            // Support Bundle Generator Section
            div { class: "support-bundle-section",
                div { class: "bundle-header-row",
                    div {
                        h3 { "{strings.support_bundle_title}" }
                        p { class: "meta-subtle", "{strings.support_bundle_desc}" }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: disabled || is_generating,
                        onclick: move |_| on_generate_bundle.call(()),
                        if is_generating {
                            "{strings.generating_bundle}..."
                        } else {
                            "{strings.generate_support_bundle}"
                        }
                    }
                }

                if let Some(b) = bundle.as_ref() {
                    div { class: "bundle-result",
                        div { class: "bundle-meta-bar",
                            span { class: "badge success", "{strings.bundle_redacted_safe}" }
                            span { class: "meta-subtle", "Bundle ID: {b.bundle_id}" }
                            span { class: "meta-subtle", "Generated: {b.generated_at}" }
                        }
                        div { class: "json-viewer",
                            pre {
                                "{serde_json::to_string_pretty(&b).unwrap_or_default()}"
                            }
                        }
                    }
                }
            }
        }
    }
}
