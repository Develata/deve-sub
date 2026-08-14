//! Sources page — list, add, refresh, delete subscription sources (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, t};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDto {
    pub id: String,
    pub name: String,
    pub url: String,
    pub source_type: String,
    pub enabled: bool,
    pub auto_update: bool,
    pub update_interval_secs: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSourcesResponse {
    pub sources: Vec<SourceDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshSourceResponse {
    pub snapshot_id: String,
    pub version: u64,
    pub not_modified: bool,
    pub node_count: u64,
}

#[derive(Props, Clone, PartialEq)]
pub struct SourcesProps {
    lang: Signal<Language>,
}

pub fn SourcesPage(props: SourcesProps) -> Element {
    let l = *props.lang.read();
    let mut sources = use_signal(Vec::<SourceDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut refreshing_id = use_signal(|| String::new());

    let fetch_sources = move || {
        spawn(async move {
            loading.set(true);
            match crate::api::get::<ListSourcesResponse>("/sources").await {
                Ok(resp) => {
                    sources.set(resp.sources);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    use_future(move || async move {
        fetch_sources();
    });

    let mut do_refresh = move |id: String| {
        refreshing_id.set(id.clone());
        spawn(async move {
            let path = format!("/sources/{id}/refresh");
            match crate::api::send::<RefreshSourceResponse, serde_json::Value>("POST", &path, None)
                .await
            {
                Ok(_) => {
                    fetch_sources();
                }
                Err(e) => {
                    error.set(e.message);
                }
            }
            refreshing_id.set(String::new());
        });
    };

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "sources.title")} }
                button {
                    class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: move |_| fetch_sources(),
                    {t(l, "common.refresh")}
                }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if sources.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", "暂无订阅源" }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "sources.name")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "sources.url")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for source in sources.read().iter() {
                                {
                                    let id = source.id.clone();
                                    let is_refreshing = *refreshing_id.read() == id;
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{source.name}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400",
                                                span { class: "block max-w-xs truncate", "{source.url}" }
                                            }
                                            td { class: "px-4 py-3",
                                                if source.enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            td { class: "px-4 py-3 text-right",
                                                button {
                                                    class: "rounded-md border border-stone-300 px-3 py-1 text-xs text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                    disabled: is_refreshing,
                                                    onclick: move |_| do_refresh(id.clone()),
                                                    if is_refreshing { {t(l, "common.loading")} } else { {t(l, "common.refresh")} }
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
