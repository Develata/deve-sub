//! Templates page — list V3 subscription templates.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, t};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub active_version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTemplatesResponse {
    pub templates: Vec<TemplateDto>,
    pub next_cursor: Option<String>,
}

#[derive(Props, Clone, PartialEq)]
pub struct TemplatesProps {
    lang: Signal<Language>,
}

pub fn TemplatesPage(props: TemplatesProps) -> Element {
    let l = *props.lang.read();
    let mut templates = use_signal(Vec::<TemplateDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());

    use_future(move || async move {
        match crate::api::get::<ListTemplatesResponse>("/templates").await {
            Ok(resp) => {
                templates.set(resp.templates);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    rsx! {
        div { class: "space-y-4",
            h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.templates")} }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if templates.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", "暂无模板" }
                }
            } else {
                div { class: "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3",
                    for tmpl in templates.read().iter() {
                        div {
                            key: "{tmpl.id}",
                            class: "rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900",
                            h3 { class: "font-medium text-stone-900 dark:text-stone-100", "{tmpl.name}" }
                            p { class: "mt-1 text-sm text-stone-500 dark:text-stone-400",
                                if tmpl.description.is_empty() { "—" } else { "{tmpl.description}" }
                            }
                            div { class: "mt-3 flex items-center justify-between",
                                span { class: "text-xs text-stone-400 dark:text-stone-500", "v{tmpl.active_version}" }
                                span { class: "text-xs text-stone-400 dark:text-stone-500", "{tmpl.updated_at}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
