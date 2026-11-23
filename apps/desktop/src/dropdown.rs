use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct DropdownItem {
    pub label: String,
    pub class: Option<String>,
    pub disabled: bool,
    pub id: String,
}

/// A compact, accessible dropdown action menu for table rows and cards.
#[component]
pub fn ActionDropdown(
    items: Vec<DropdownItem>,
    on_select: EventHandler<String>,
    #[props(default = "Actions".to_owned())] trigger_label: String,
    #[props(default = false)] disabled: bool,
) -> Element {
    let mut is_open = use_signal(|| false);

    rsx! {
        div { class: "action-dropdown-wrap",
            button {
                class: if is_open() { "button dropdown-trigger active" } else { "button dropdown-trigger" },
                disabled: disabled,
                type: "button",
                aria_expanded: "{is_open()}",
                onclick: move |event| {
                    event.stop_propagation();
                    is_open.toggle();
                },
                "{trigger_label} ▾"
            }
            if is_open() {
                div {
                    class: "dropdown-backdrop-transparent",
                    onclick: move |_| is_open.set(false),
                }
                div { class: "dropdown-menu", role: "menu",
                    for item in items.into_iter() {
                        {
                            let item_id = item.id.clone();
                            let item_class = format!("dropdown-menu-item {}", item.class.unwrap_or_default());
                            let is_item_disabled = item.disabled;
                            rsx! {
                                button {
                                    class: "{item_class}",
                                    role: "menuitem",
                                    disabled: is_item_disabled,
                                    onclick: move |_| {
                                        is_open.set(false);
                                        on_select.call(item_id.clone());
                                    },
                                    "{item.label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
