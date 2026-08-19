//! Audit log page — filterable, cursor-paginated audit log viewer (AUDIT-001).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::audit_types::{
    ACTIONS, AuditLogDto, ListAuditLogsResponse, TARGET_TYPES,
};

#[derive(Props, Clone, PartialEq)]
pub struct AuditProps {
    lang: Signal<Language>,
}

pub fn AuditPage(props: AuditProps) -> Element {
    let l = *props.lang.read();
    let mut entries = use_signal(Vec::<AuditLogDto>::new);
    let mut loading = use_signal(|| true);
    let mut loading_more = use_signal(|| false);
    let mut error = use_signal(|| String::new());
    let mut cursor = use_signal(|| Option::<String>::None);

    let mut f_action = use_signal(String::new);
    let mut f_target_type = use_signal(String::new);

    let build_query = move |cursor_val: Option<&str>| -> String {
        let mut params: Vec<String> = Vec::new();
        params.push("limit=50".to_string());
        if let Some(c) = cursor_val {
            if !c.is_empty() {
                params.push(format!("cursor={c}"));
            }
        }
        let action = f_action.read().clone();
        if !action.is_empty() {
            params.push(format!("action={action}"));
        }
        let tt = f_target_type.read().clone();
        if !tt.is_empty() {
            params.push(format!("target_type={tt}"));
        }
        format!("/audit-logs?{}", params.join("&"))
    };

    let fetch_initial = move || {
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            let path = build_query(None);
            match crate::api::get::<ListAuditLogsResponse>(&path).await {
                Ok(resp) => {
                    entries.set(resp.entries);
                    cursor.set(resp.next_cursor);
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    use_future(move || async move {
        fetch_initial();
    });

    let apply_filters = move |_| {
        entries.set(Vec::new());
        cursor.set(None);
        fetch_initial();
    };

    let load_more = move |_| {
        if *loading_more.read() {
            return;
        }
        let c = cursor.read().clone();
        let Some(c) = c else { return };
        loading_more.set(true);
        spawn(async move {
            let path = build_query(Some(&c));
            match crate::api::get::<ListAuditLogsResponse>(&path).await {
                Ok(resp) => {
                    let mut current = entries.read().clone();
                    current.extend(resp.entries);
                    entries.set(current);
                    cursor.set(resp.next_cursor);
                }
                Err(e) => error.set(e.message),
            }
            loading_more.set(false);
        });
    };

    rsx! {
        div { class: "space-y-4",
            h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.audit")} }

            div { class: "flex flex-wrap items-end gap-3",
                div {
                    label { class: "block text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.action")} }
                    select {
                        class: "mt-1 rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                        value: "{f_action}",
                        onchange: move |e| f_action.set(e.value()),
                        option { value: "", {t(l, "audit.all")} }
                        for a in ACTIONS.iter().copied() {
                            option { value: "{a}", "{a}" }
                        }
                    }
                }
                div {
                    label { class: "block text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.target_type")} }
                    select {
                        class: "mt-1 rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                        value: "{f_target_type}",
                        onchange: move |e| f_target_type.set(e.value()),
                        option { value: "", {t(l, "audit.all")} }
                        for tt in TARGET_TYPES.iter().copied() {
                            option { value: "{tt}", "{tt}" }
                        }
                    }
                }
                button {
                    class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: apply_filters,
                    {t(l, "audit.apply")}
                }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if entries.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "audit.empty")} }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.time")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.action")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.actor")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.target")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.details")} }
                            }
                        }
                        tbody {
                            for entry in entries.read().iter() {
                                {
                                    let actor = entry.actor_id.clone().unwrap_or_else(|| "system".to_string());
                                    let target = match (&entry.target_type, &entry.target_id) {
                                        (Some(tt), Some(tid)) => format!("{tt}:{tid}"),
                                        (Some(tt), None) => tt.clone(),
                                        _ => "—".to_string(),
                                    };
                                    rsx! {
                                        tr {
                                            key: "{entry.id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{entry.created_at}" }
                                            td { class: "px-4 py-3",
                                                span { class: "font-mono text-xs text-stone-700 dark:text-stone-300", "{entry.action}" }
                                            }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{actor}" }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{target}" }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400",
                                                if let Some(d) = &entry.details_json {
                                                    "{d}"
                                                } else {
                                                    "—"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if cursor.read().is_some() {
                    div { class: "flex justify-center pt-4",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *loading_more.read(),
                            onclick: load_more,
                            if *loading_more.read() { {t(l, "common.loading")} } else { {t(l, "audit.load_more")} }
                        }
                    }
                }
            }
        }
    }
}
