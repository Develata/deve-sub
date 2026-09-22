//! Sources page — list, create, edit, delete, refresh subscription sources (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::source_types::{
    CreateSourceRequest, ListSourcesResponse, RefreshJobAcceptedResponse, SourceDto,
    SourceRefreshJobDto, SourceResponse, SourceTypeDto, SourceTypePresentation,
    UpdateSourceRequest,
};
use crate::pages::util::sleep_ms;
use super::source_modals::{Modal, SourceModals};

#[derive(Props, Clone, PartialEq)]
pub struct SourcesProps {
    lang: Signal<Language>,
}

pub fn SourcesPage(props: SourcesProps) -> Element {
    let l = *props.lang.read();
    let mut sources = use_signal(Vec::<SourceDto>::new);
    let mut next_cursor = use_signal(|| None::<String>);
    let mut page_busy = use_signal(|| false);
    let mut list_revision = use_signal(|| 0_u64);
    let mut page_error = use_signal(String::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut refreshing_ids = use_signal(std::collections::HashSet::<String>::new);
    let mut refresh_failed = use_signal(|| false);
    let mut refresh_msg = use_signal(|| String::new());
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new());
    let mut saving = use_signal(|| false);

    let mut f_name = use_signal(String::new);
    let mut f_url = use_signal(String::new);
    let mut f_type = use_signal(|| SourceTypeDto::Auto);
    let mut f_auto = use_signal(|| false);
    let mut f_interval = use_signal(|| "3600".to_string());
    let mut f_keep = use_signal(|| true);
    let mut f_enabled = use_signal(|| true);
    let mut f_filter_rules = use_signal(|| Option::<_>::None);

    let mut fetch_sources = move || {
        *list_revision.write() += 1;
        let revision = *list_revision.read();
        spawn(async move {
            loading.set(true);
            let result = crate::api::get::<ListSourcesResponse>("/sources").await;
            if revision != *list_revision.read() { return; }
            match result {
                Ok(resp) => {
                    next_cursor.set(resp.next_cursor);
                    sources.set(resp.sources);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let load_more = move |_| {
        if *page_busy.read() || *loading.read() { return; }
        let Some(cursor) = next_cursor.read().clone() else { return; };
        let revision = *list_revision.read();
        page_busy.set(true); page_error.set(String::new());
        spawn(async move {
            match crate::api::get::<ListSourcesResponse>(&format!("/sources?cursor={cursor}")).await {
                Ok(resp) if revision == *list_revision.read() => {
                    next_cursor.set(resp.next_cursor); sources.write().extend(resp.sources);
                }
                Ok(_) => {}
                Err(e) if revision == *list_revision.read() => page_error.set(e.message),
                Err(_) => {}
            }
            page_busy.set(false);
        });
    };

    use_future(move || async move {
        fetch_sources();
    });

    let open_create = move |_| {
        f_name.set(String::new());
        f_url.set(String::new());
        f_type.set(SourceTypeDto::Auto);
        f_auto.set(false);
        f_interval.set("3600".to_string());
        f_keep.set(true);
        f_enabled.set(true);
        f_filter_rules.set(None);
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let mut open_edit = move |source: SourceDto| {
        f_name.set(source.name.clone());
        // WHY: list/detail URLs are redacted; an empty edit draft preserves the secret.
        f_url.set(String::new());
        f_type.set(source.source_type);
        f_auto.set(source.auto_update);
        f_interval.set(source.update_interval_secs.to_string());
        f_keep.set(source.keep_on_fail);
        f_enabled.set(source.enabled);
        f_filter_rules.set(source.filter_rules.clone());
        form_error.set(String::new());
        modal.set(Modal::Edit(source));
    };

    let mut open_delete = move |source: SourceDto| {
        form_error.set(String::new());
        modal.set(Modal::Delete(source));
    };

    let close_modal = move |()| {
        if *saving.read() { return; }
        modal.set(Modal::None);
    };

    let mut do_refresh = move |id: String| {
        if !refreshing_ids.write().insert(id.clone()) { return; }
        refresh_failed.set(false);
        refresh_msg.set(String::new());
        spawn(async move {
            let path = format!("/sources/{id}/refresh");
            let accepted = match crate::api::send::<RefreshJobAcceptedResponse, serde_json::Value>(
                "POST", &path, None,
            )
            .await
            {
                Ok(a) => a,
                Err(e) => {
                    refresh_msg.set(format!("{id}: {}", e.message));
                    refresh_failed.set(true);
                    refreshing_ids.write().remove(&id);
                    return;
                }
            };

            let job_path = format!("/sources/refresh-jobs/{}", accepted.job_id);
            let deadline = js_sys::Date::now() + 30_000.0;
            let mut polls = 0u32;
            let job = loop {
                polls += 1;
                if polls > 30 || js_sys::Date::now() > deadline {
                    let msg = t(l, "sources.refresh_timeout");
                    refresh_failed.set(true);
                    refresh_msg.set(format!("{id}: {msg}"));
                    refreshing_ids.write().remove(&id);
                    return;
                }
                match crate::api::get::<SourceRefreshJobDto>(&job_path).await {
                    Ok(j) if matches!(j.status.as_str(), "completed" | "failed" | "cancelled") => {
                        break j;
                    }
                    Ok(_) => sleep_ms(1000).await,
                    Err(e) => {
                        refresh_msg.set(format!("{id}: {}", e.message));
                    refresh_failed.set(true);
                        refreshing_ids.write().remove(&id);
                        return;
                    }
                }
            };

            let msg = match job.status.as_str() {
                "completed" => {
                    if job.not_modified {
                        format!("{} {}", t(l, "sources.node_count"), t(l, "common.success"),)
                    } else {
                        format!(
                            "{} {}, +{} ~{} -{}",
                            t(l, "sources.node_count"),
                            t(l, "common.success"),
                            job.new_nodes,
                            job.reactivated_nodes,
                            job.missing_nodes,
                        )
                    }
                }
                "cancelled" => t(l, "common.cancelled").to_string(),
                _ => job
                    .error_message
                    .unwrap_or_else(|| t(l, "common.error").to_string()),
            };
            refresh_failed.set(job.status != "completed");
            refresh_msg.set(format!("{id}: {msg}"));
            fetch_sources();
            refreshing_ids.write().remove(&id);
        });
    };

    let do_submit = move |()| {
        if *saving.read() { return; }
        form_error.set(String::new());
        let state = (*modal.read()).clone();
        match state {
            Modal::Create => {
                let name = f_name.read().trim().to_string();
                let url = f_url.read().trim().to_string();
                if name.is_empty() || url.is_empty() {
                    form_error.set(t(l, "sources.required").to_string());
                    return;
                }
                let Ok(interval) = f_interval.read().parse::<u64>() else {
                    form_error.set(t(l, "sources.interval_invalid").to_string());
                    return;
                };
                let req = CreateSourceRequest {
                    name,
                    source_type: *f_type.read(),
                    url,
                    auto_update: *f_auto.read(),
                    update_interval_secs: interval,
                    keep_on_fail: *f_keep.read(),
                    filter_rules: f_filter_rules.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    match crate::api::send::<SourceResponse, CreateSourceRequest>(
                        "POST",
                        "/sources",
                        Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_sources();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Edit(source) => {
                let id = source.id.clone();
                let name = f_name.read().trim().to_string();
                let url = f_url.read().trim().to_string();
                if name.is_empty() {
                    form_error.set(t(l, "sources.name_required").to_string());
                    return;
                }
                let Ok(interval) = f_interval.read().parse::<u64>() else {
                    form_error.set(t(l, "sources.interval_invalid").to_string());
                    return;
                };
                let req = UpdateSourceRequest {
                    name,
                    source_type: *f_type.read(),
                    url: (!url.is_empty()).then_some(url),
                    auto_update: *f_auto.read(),
                    update_interval_secs: interval,
                    enabled: *f_enabled.read(),
                    keep_on_fail: *f_keep.read(),
                    filter_rules: f_filter_rules.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    let path = format!("/sources/{id}");
                    match crate::api::send::<SourceResponse, UpdateSourceRequest>(
                        "PUT",
                        &path,
                        Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_sources();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Delete(source) => {
                let id = source.id.clone();
                saving.set(true);
                spawn(async move {
                    let path = format!("/sources/{id}");
                    match crate::api::delete(&path).await {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_sources();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::None => {}
        }
    };

    rsx! {
        div { class: "space-y-4",
            div { class: "flex flex-wrap items-center justify-between gap-3",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "sources.title")} }
                div { class: "flex gap-2",
                    button {
                        class: "rounded-md border border-stone-300 px-4 py-2 text-sm font-medium text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| fetch_sources(),
                        {t(l, "common.refresh")}
                    }
                    button {
                        class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                        onclick: open_create,
                        {t(l, "sources.add")}
                    }
                }
            }

            if !refresh_msg.read().is_empty() {
                div { role: "status", class: if *refresh_failed.read() { "rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-900/20 dark:text-red-400" } else { "rounded-md bg-green-50 p-3 text-sm text-green-700 dark:bg-green-900/20 dark:text-green-400" },
                    "{refresh_msg}"
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
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "sources.empty")} }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "source-table w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "sources.name")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "sources.url")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "sources.source_type")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for source in sources.read().iter() {
                                {
                                    let id = source.id.clone();
                                    let is_refreshing = refreshing_ids.read().contains(&id);
                                    let edit_src = source.clone();
                                    let del_src = source.clone();
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "source-identity px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{source.name}" }
                                            td { class: "source-address px-4 py-3 text-stone-500 dark:text-stone-400",
                                                span { class: "block max-w-xs truncate", "{source.url}" }
                                            }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400",
                                                {source.source_type.label(l)}
                                            }
                                            td { class: "px-4 py-3",
                                                if source.enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            td { class: "source-actions px-4 py-3 text-right",
                                                div { class: "flex justify-end gap-1",
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        disabled: is_refreshing,
                                                        onclick: move |_| do_refresh(id.clone()),
                                                        if is_refreshing { {t(l, "common.loading")} } else { {t(l, "common.refresh")} }
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_edit(edit_src.clone()),
                                                        {t(l, "common.edit")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-red-300 px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                                                        onclick: move |_| open_delete(del_src.clone()),
                                                        {t(l, "common.delete")}
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
            if !page_error.read().is_empty() { p { role: "alert", class: "text-sm text-red-600", "{page_error}" } }
            if next_cursor.read().is_some() { button { class: "node-control", disabled: *page_busy.read() || *loading.read(), onclick: load_more, {t(l, "nodes.load_more")} } }
        }

        SourceModals { lang: props.lang, modal, f_name, f_url, f_type, f_auto, f_interval, f_keep, f_enabled,
            form_error, saving, on_close: close_modal, on_submit: do_submit }
    }
}
