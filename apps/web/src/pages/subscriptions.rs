//! Subscriptions page — list, copy delivery link (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, t};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDto {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub profile: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSubscriptionsResponse {
    pub subscriptions: Vec<SubscriptionDto>,
    pub next_cursor: Option<String>,
}

#[derive(Props, Clone, PartialEq)]
pub struct SubscriptionsProps {
    lang: Signal<Language>,
}

pub fn SubscriptionsPage(props: SubscriptionsProps) -> Element {
    let l = *props.lang.read();
    let mut subs = use_signal(Vec::<SubscriptionDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut copied_id = use_signal(|| String::new());

    use_future(move || async move {
        match crate::api::get::<ListSubscriptionsResponse>("/subscriptions").await {
            Ok(resp) => {
                subs.set(resp.subscriptions);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    let copy_link = move |id: String, slug: String| {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default();
        let link = format!("{origin}/sub/{slug}");
        spawn(async move {
            let _ = copy_to_clipboard(&link).await;
            copied_id.set(id);
        });
    };

    rsx! {
        div { class: "space-y-4",
            h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.subscriptions")} }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if subs.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", "暂无订阅" }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "名称" }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "Profile" }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for sub in subs.read().iter() {
                                {
                                    let id = sub.id.clone();
                                    let slug = sub.slug.clone();
                                    let is_copied = *copied_id.read() == id;
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{sub.name}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400", "{sub.profile}" }
                                            td { class: "px-4 py-3",
                                                if sub.enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            td { class: "px-4 py-3 text-right",
                                                button {
                                                    class: "rounded-md border border-stone-300 px-3 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                    onclick: move |_| copy_link(id.clone(), slug.clone()),
                                                    if is_copied { "✓ 已复制" } else { {t(l, "common.copy")} }
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
        }
    }
}

async fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();
    let promise = clipboard.write_text(text);
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|e| format!("clipboard error: {e:?}"))
}
