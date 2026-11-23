use dioxus::prelude::*;

/// A professional, accessible modal dialog component.
#[component]
pub fn Modal(
    title: String,
    close_label: String,
    on_close: EventHandler<()>,
    children: Element,
    #[props(default)] footer: Option<Element>,
) -> Element {
    let (close_btn, close_key, close_icon) = (on_close, on_close, on_close);

    rsx! {
        div { class: "modal-layer",
            div {
                class: "modal-backdrop",
                tabindex: "-1",
                onclick: move |_| close_btn.call(()),
            }
            div {
                class: "modal-box",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "modal-title",
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        event.prevent_default();
                        close_key.call(());
                    }
                },
                header { class: "modal-header",
                    h2 { id: "modal-title", "{title}" }
                    button {
                        class: "modal-close-btn",
                        type: "button",
                        autofocus: true,
                        aria_label: "{close_label}",
                        onclick: move |_| close_icon.call(()),
                        "×"
                    }
                }
                div { class: "modal-body", {children} }
                if let Some(footer_elem) = footer {
                    footer { class: "modal-footer", {footer_elem} }
                }
            }
        }
    }
}
