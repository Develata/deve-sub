//! Filtered audit history with response generation guards and explicit cleanup.
#![cfg(target_family = "wasm")]
use super::{
    audit_cleanup::AuditCleanup,
    audit_table::AuditTable,
    audit_types::{ACTIONS, AuditLogDto, ListAuditLogsResponse, TARGET_TYPES, action_label, tr},
};
use crate::{
    api,
    i18n::{Language, t},
};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct AuditProps {
    lang: Signal<Language>,
}

pub fn AuditPage(props: AuditProps) -> Element {
    let l = *props.lang.read();
    let mut entries = use_signal(Vec::<AuditLogDto>::new);
    let mut loading = use_signal(|| true);
    let mut loading_more = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut cursor = use_signal(|| None::<String>);
    let mut generation = use_signal(|| 0u64);
    let mut applied = use_signal(|| "/audit-logs?limit=50".to_string());
    let mut action = use_signal(String::new);
    let mut target_type = use_signal(String::new);
    let mut since = use_signal(String::new);
    let mut before = use_signal(String::new);

    let mut fetch = move |path: String| {
        generation += 1;
        let revision = *generation.read();
        loading.set(true);
        loading_more.set(false);
        error.set(String::new());
        entries.set(Vec::new());
        cursor.set(None);
        applied.set(path.clone());
        spawn(async move {
            let result = api::get::<ListAuditLogsResponse>(&path).await;
            if revision != *generation.read() {
                return;
            }
            match result {
                Ok(value) => {
                    entries.set(value.entries);
                    cursor.set(value.next_cursor);
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };
    use_future(move || async move {
        fetch("/audit-logs?limit=50".into());
    });
    let apply = move |_| {
        let mut path = "/audit-logs?limit=50".to_string();
        for (key, value) in [
            ("action", action.read().clone()),
            ("target_type", target_type.read().clone()),
        ] {
            if !value.is_empty() {
                path.push_str(&format!("&{key}={value}"));
            }
        }
        for (key, value) in [
            ("since", since.read().clone()),
            ("before", before.read().clone()),
        ] {
            if !value.is_empty() {
                path.push_str(&format!("&{key}={value}T00:00:00Z"));
            }
        }
        fetch(path);
    };
    let more = move |_| {
        if *loading.read() || *loading_more.read() {
            return;
        }
        let Some(next) = cursor.read().clone() else {
            return;
        };
        let revision = *generation.read();
        let path = format!("{}&cursor={next}", applied.read());
        loading_more.set(true);
        error.set(String::new());
        spawn(async move {
            let result = api::get::<ListAuditLogsResponse>(&path).await;
            if revision != *generation.read() {
                return;
            }
            match result {
                Ok(value) => {
                    entries.write().extend(value.entries);
                    cursor.set(value.next_cursor);
                }
                Err(e) => error.set(e.message),
            }
            loading_more.set(false);
        });
    };
    rsx! {
        div { class: "min-w-0 space-y-4",
            h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.audit")} }
            AuditCleanup { lang: props.lang, on_cleaned: move |_| { let path = applied.read().clone(); fetch(path); } }
            div { class: "flex flex-wrap items-end gap-3",
                label { class: "text-xs text-stone-500 dark:text-stone-400", {t(l, "audit.action")}
                    select { class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", value: "{action}", onchange: move |e| action.set(e.value()),
                        option { value: "", {t(l, "audit.all")} }
                        for a in ACTIONS.iter().copied() { option { value: "{a}", "{action_label(l, a)}" } }
                    }
                }
                label { class: "text-xs text-stone-500 dark:text-stone-400", {t(l, "audit.target_type")}
                    select { class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", value: "{target_type}", onchange: move |e| target_type.set(e.value()),
                        option { value: "", {t(l, "audit.all")} }
                        for value in TARGET_TYPES.iter().copied() { option { value: "{value}", "{value}" } }
                    }
                }
                label { class: "text-xs text-stone-500 dark:text-stone-400", {tr(l, "开始日期（UTC）", "From date (UTC)")}
                    input { r#type: "date", class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", value: "{since}", onchange: move |e| since.set(e.value()) }
                }
                label { class: "text-xs text-stone-500 dark:text-stone-400", {tr(l, "结束日期（不含，UTC）", "Before date (UTC)")}
                    input { r#type: "date", class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", value: "{before}", onchange: move |e| before.set(e.value()) }
                }
                button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700", onclick: apply, {t(l, "audit.apply")} }
                button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm dark:border-stone-700", onclick: move |_| { let path = applied.read().clone(); fetch(path); }, {tr(l, "刷新日志", "Refresh logs")} }
            }
            if !error.read().is_empty() { p { role: "alert", class: "text-sm text-red-600 dark:text-red-400", "{error}" } }
            if *loading.read() { p { role: "status", {t(l, "common.loading")} } }
            else if entries.read().is_empty() { p { class: "p-8 text-center text-sm text-stone-500", {t(l, "audit.empty")} } }
            else { AuditTable { entries: entries.read().clone(), lang: l } }
            if cursor.read().is_some() && !*loading.read() {
                div { class: "flex justify-center",
                    button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm disabled:opacity-50 dark:border-stone-700", disabled: *loading_more.read(), onclick: more,
                        if *loading_more.read() { {t(l, "common.loading")} } else { {t(l, "audit.load_more")} }
                    }
                }
            }
        }
    }
}
