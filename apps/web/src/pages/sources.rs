//! Sources page — list, create, edit, delete, refresh subscription sources (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::source_types::{
    CreateSourceRequest, ListSourcesResponse, RefreshSourceResponse, SourceDto, SourceResponse,
    SourceTypeDto, UpdateSourceRequest,
};

#[derive(Clone, PartialEq)]
enum Modal {
    None,
    Create,
    Edit(SourceDto),
    Delete(SourceDto),
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
    let mut refresh_msg = use_signal(|| String::new());
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new);
    let mut saving = use_signal(|| false);

    let mut f_name = use_signal(String::new);
    let mut f_url = use_signal(String::new);
    let mut f_type = use_signal(|| SourceTypeDto::Auto);
    let mut f_auto = use_signal(|| false);
    let mut f_interval = use_signal(|| 3600u64);
    let mut f_keep = use_signal(|| true);
    let mut f_enabled = use_signal(|| true);
    let mut f_filter_rules = use_signal(|| Option::<_>::None);

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

    let open_create = move |_| {
        f_name.set(String::new());
        f_url.set(String::new());
        f_type.set(SourceTypeDto::Auto);
        f_auto.set(false);
        f_interval.set(3600);
        f_keep.set(true);
        f_enabled.set(true);
        f_filter_rules.set(None);
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let open_edit = move |source: SourceDto| {
        f_name.set(source.name.clone());
        f_url.set(source.url.clone());
        f_type.set(source.source_type);
        f_auto.set(source.auto_update);
        f_interval.set(source.update_interval_secs);
        f_keep.set(source.keep_on_fail);
        f_enabled.set(source.enabled);
        f_filter_rules.set(source.filter_rules.clone());
        form_error.set(String::new());
        modal.set(Modal::Edit(source));
    };

    let open_delete = move |source: SourceDto| {
        form_error.set(String::new());
        modal.set(Modal::Delete(source));
    };

    let close_modal = move |_| {
        modal.set(Modal::None);
    };

    let do_refresh = move |id: String| {
        refreshing_id.set(id.clone());
        refresh_msg.set(String::new());
        spawn(async move {
            let path = format!("/sources/{id}/refresh");
            match crate::api::send::<RefreshSourceResponse, serde_json::Value>("POST", &path, None)
                .await
            {
                Ok(resp) => {
                    let msg = if resp.not_modified {
                        format!(
                            "v{} (304) — {} {}",
                            resp.version,
                            resp.node_count,
                            t(l, "sources.node_count"),
                        )
                    } else {
                        format!(
                            "v{} — {} {}, +{} ~{} -{}",
                            resp.version,
                            resp.node_count,
                            t(l, "sources.node_count"),
                            resp.reconcile.new_nodes,
                            resp.reconcile.reactivated_nodes,
                            resp.reconcile.missing_nodes,
                        )
                    };
                    refresh_msg.set(msg);
                    fetch_sources();
                }
                Err(e) => error.set(e.message),
            }
            refreshing_id.set(String::new());
        });
    };

    let do_submit = move |_| {
        let state = (*modal.read()).clone();
        match state {
            Modal::Create => {
                let name = f_name.read().clone();
                let url = f_url.read().clone();
                if name.is_empty() || url.is_empty() {
                    form_error.set("Name and URL are required".to_string());
                    return;
                }
                let req = CreateSourceRequest {
                    name,
                    source_type: *f_type.read(),
                    url,
                    auto_update: *f_auto.read(),
                    update_interval_secs: *f_interval.read(),
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
                let name = f_name.read().clone();
                let url = f_url.read().clone();
                if name.is_empty() || url.is_empty() {
                    form_error.set("Name and URL are required".to_string());
                    return;
                }
                let req = UpdateSourceRequest {
                    name,
                    source_type: *f_type.read(),
                    url,
                    auto_update: *f_auto.read(),
                    update_interval_secs: *f_interval.read(),
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

    let is_form_modal = matches!(*modal.read(), Modal::Create | Modal::Edit(_));
    let is_delete_modal = matches!(*modal.read(), Modal::Delete(_));
    let is_edit = matches!(*modal.read(), Modal::Edit(_));

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
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
                div { class: "rounded-md bg-green-50 p-3 text-sm text-green-700 dark:bg-green-900/20 dark:text-green-400",
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
                    p { class: "text-sm text-stone-500 dark:text-stone-400", "暂无订阅源" }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
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
                                    let is_refreshing = *refreshing_id.read() == id;
                                    let edit_src = source.clone();
                                    let del_src = source.clone();
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{source.name}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400",
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
                                            td { class: "px-4 py-3 text-right",
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
        }

        if is_form_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: close_modal,
                div {
                    class: "w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                        if is_edit { {t(l, "sources.edit_title")} } else { {t(l, "sources.add")} }
                    }
                    div { class: "mt-4 space-y-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "sources.name")} }
                            input {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                r#type: "text",
                                value: "{f_name}",
                                oninput: move |e| f_name.set(e.value()),
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "sources.url")} }
                            input {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                r#type: "url",
                                value: "{f_url}",
                                oninput: move |e| f_url.set(e.value()),
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "sources.source_type")} }
                            select {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{f_type.read().as_str()}",
                                onchange: move |e| f_type.set(SourceTypeDto::from_str(&e.value())),
                                for st in SourceTypeDto::ALL.iter().copied() {
                                    option { value: "{st.as_str()}", {st.label(l)} }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "sources.update_interval")} }
                            input {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                r#type: "number",
                                value: "{f_interval}",
                                oninput: move |e| {
                                    let v = e.value().parse::<u64>().unwrap_or(3600);
                                    f_interval.set(v);
                                },
                            }
                        }
                        div { class: "flex items-center gap-4",
                            label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                                input {
                                    r#type: "checkbox",
                                    checked: *f_auto.read(),
                                    onchange: move |e| f_auto.set(e.checked()),
                                }
                                {t(l, "sources.auto_update")}
                            }
                            label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                                input {
                                    r#type: "checkbox",
                                    checked: *f_keep.read(),
                                    onchange: move |e| f_keep.set(e.checked()),
                                }
                                {t(l, "sources.keep_on_fail")}
                            }
                        }
                        if is_edit {
                            label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                                input {
                                    r#type: "checkbox",
                                    checked: *f_enabled.read(),
                                    onchange: move |e| f_enabled.set(e.checked()),
                                }
                                {t(l, "nodes.enabled")}
                            }
                        }
                    }

                    if !form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400",
                            "{form_error}"
                        }
                    }

                    div { class: "mt-6 flex justify-end gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: close_modal,
                            {t(l, "common.cancel")}
                        }
                        button {
                            class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                            disabled: *saving.read(),
                            onclick: do_submit,
                            if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }

        if is_delete_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: close_modal,
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "common.delete")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400", {t(l, "sources.delete_confirm")} }
                    if !form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400",
                            "{form_error}"
                        }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: close_modal,
                            {t(l, "common.cancel")}
                        }
                        button {
                            class: "rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50",
                            disabled: *saving.read(),
                            onclick: do_submit,
                            if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.delete")} }
                        }
                    }
                }
            }
        }
    }
}
