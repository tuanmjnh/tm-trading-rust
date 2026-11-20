use dioxus::prelude::*;

/// A single accessible slideover overlay.
#[component]
pub fn Slideover(
    title: String,
    close_label: String,
    on_close: EventHandler<()>,
    children: Element,
) -> Element {
    // Callbacks are Copy, so each handler can take its own value.
    let (close_button, close_key, close_icon) = (on_close, on_close, on_close);

    rsx! {
        div { class: "slideover-layer",
            button {
                class: "slideover-backdrop",
                type: "button",
                aria_hidden: "true",
                tabindex: "-1",
                onclick: move |_| close_button.call(()),
            }
            section {
                class: "slideover",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "slideover-title",
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        event.prevent_default();
                        close_key.call(());
                    }
                },
                header { class: "slideover-header",
                    h2 { id: "slideover-title", "{title}" }
                    button {
                        class: "slideover-close",
                        type: "button",
                        autofocus: true,
                        aria_label: "{close_label}",
                        onclick: move |_| close_icon.call(()),
                        "×"
                    }
                }
                div { class: "slideover-body", {children} }
            }
        }
    }
}
