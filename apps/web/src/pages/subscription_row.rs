//! Existing subscription actions with a compact primary/secondary hierarchy.
use crate::i18n::{Language, t};
use crate::pages::subscription_types::SubscriptionDto;
use dioxus::prelude::*;

/// Render one subscription and dispatch the existing parent-owned actions.
#[component]
pub fn SubscriptionRow(
    lang: Language,
    sub: SubscriptionDto,
    copied: bool,
    on_copy: EventHandler<String>,
    on_edit: EventHandler<SubscriptionDto>,
    on_rotate: EventHandler<SubscriptionDto>,
    on_regenerate: EventHandler<SubscriptionDto>,
    on_temp_link: EventHandler<SubscriptionDto>,
    on_delete: EventHandler<SubscriptionDto>,
) -> Element {
    let copy_id = sub.id.clone();
    let edit = sub.clone();
    let rotate = sub.clone();
    let regenerate = sub.clone();
    let temp_link = sub.clone();
    let delete = sub.clone();
    rsx! {
        tr { class: "border-b border-stone-100 align-top dark:border-stone-800", role: "row",
            td { class: "subscription-identity px-4 py-4 font-medium", role: "cell", "{sub.name}" }
            td { class: "subscription-slug px-4 py-4 text-stone-600 dark:text-stone-400", role: "cell",
                span { class: "subscription-cell-label", aria_hidden: "true", {t(lang, "subs.slug")} } "{sub.slug}" }
            td { class: "px-4 py-4 text-stone-600 dark:text-stone-400", role: "cell",
                span { class: "subscription-cell-label", aria_hidden: "true", {t(lang, "subs.profile")} } "{sub.profile}" }
            td { class: "px-4 py-4 text-stone-600 dark:text-stone-400", role: "cell",
                span { class: "subscription-cell-label", aria_hidden: "true", {t(lang, "subs.short_code")} }
                span { class: "font-mono text-xs", {sub.short_code.as_deref().unwrap_or("—")} }
            }
            td { class: "px-4 py-4", role: "cell",
                span { class: "subscription-cell-label", aria_hidden: "true", {t(lang, "nodes.status")} }
                span { class: if sub.enabled { "subscription-status-enabled" } else { "text-sm text-stone-600 dark:text-stone-400" },
                    {t(lang, if sub.enabled { "nodes.enabled" } else { "nodes.disabled" })} }
            }
            td { class: "subscription-action-cell px-4 py-4", role: "cell",
                div { class: "subscription-actions",
                    button { class: "ui-button ui-button-accent", onclick: move |_| on_copy.call(copy_id.clone()),
                        if copied { {t(lang, "subs.copied")} } else { {t(lang, "common.copy")} } }
                    button { class: "ui-button", onclick: move |_| on_edit.call(edit.clone()), {t(lang, "common.edit")} }
                    details { class: "subscription-more",
                        summary { {t(lang, "subs.more_actions")} }
                        div { class: "subscription-secondary-actions",
                            button { class: "ui-button", onclick: move |_| on_temp_link.call(temp_link.clone()), {t(lang, "subs.temp_link")} }
                            button { class: "ui-button", onclick: move |_| on_regenerate.call(regenerate.clone()), {t(lang, "subs.regenerate_short_code")} }
                            button { class: "ui-button", onclick: move |_| on_rotate.call(rotate.clone()), {t(lang, "subs.rotate_token")} }
                            button { class: "ui-button ui-button-danger", onclick: move |_| on_delete.call(delete.clone()), {t(lang, "common.delete")} }
                        }
                    }
                }
            }
        }
    }
}
