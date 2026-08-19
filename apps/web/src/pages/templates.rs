//! Templates page — list, create, edit, delete, versions, rollback, preview,
//! generate (UI-010).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::template_gen_modal::{TemplateGenModal, TemplateGenModalProps};
use crate::pages::template_modals::{TemplateModals, TemplateModalsProps};
use crate::pages::template_types::{
    CreateTemplateRequest, GenerationResultDto, GetTemplateResponse, ListTemplatesResponse,
    ListVersionsResponse, Modal, RollbackRequest, RollbackTemplateResponse, TemplateDto,
    TemplateResponse, TemplateVersionDto, UpdateTemplateRequest,
};
use crate::pages::template_versions::{TemplateVersions, TemplateVersionsProps};

#[derive(Props, Clone, PartialEq)]
pub struct TemplatesProps {
    lang: Signal<Language>,
}

pub fn TemplatesPage(props: TemplatesProps) -> Element {
    let l = *props.lang.read();
    let mut templates = use_signal(Vec::<TemplateDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new());
    let mut saving = use_signal(|| false);

    let mut f_name = use_signal(String::new);
    let mut f_desc = use_signal(String::new);
    let mut f_spec = use_signal(String::new);

    let mut versions = use_signal(Vec::<TemplateVersionDto>::new);
    let mut ver_loading = use_signal(|| false);
    let mut ver_error = use_signal(|| String::new);

    let mut gen_profile = use_signal(|| "mihomo".to_string());
    let mut gen_mode = use_signal(|| "lenient".to_string());
    let mut gen_result = use_signal(|| Option::<GenerationResultDto>::None);
    let mut gen_loading = use_signal(|| false);
    let mut gen_error = use_signal(|| String::new());
    let mut is_generate = use_signal(|| true);

    let fetch_templates = move || {
        spawn(async move {
            loading.set(true);
            match crate::api::get::<ListTemplatesResponse>("/templates").await {
                Ok(resp) => {
                    templates.set(resp.templates);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    use_future(move || async move {
        fetch_templates();
    });

    let open_create = move |_| {
        f_name.set(String::new());
        f_desc.set(String::new());
        f_spec.set(String::new());
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let open_edit = move |t: TemplateDto| {
        f_name.set(t.name.clone());
        f_desc.set(t.description.clone());
        f_spec.set(String::new());
        form_error.set(String::new());
        let tid = t.id.clone();
        spawn(async move {
            match crate::api::get::<GetTemplateResponse>(&format!("/templates/{tid}")).await {
                Ok(_) => {}
                Err(e) => form_error.set(e.message),
            }
        });
        let tid2 = t.id.clone();
        spawn(async move {
            match crate::api::get::<ListVersionsResponse>(&format!("/templates/{tid2}/versions")).await {
                Ok(resp) => {
                    if let Some(v) = resp.versions.into_iter().find(|v| v.is_active) {
                        f_spec.set(v.spec_yaml);
                    }
                }
                Err(e) => form_error.set(e.message),
            }
        });
        modal.set(Modal::Edit(t));
    };

    let open_delete = move |t: TemplateDto| {
        form_error.set(String::new());
        modal.set(Modal::Delete(t));
    };

    let open_versions = move |t: TemplateDto| {
        ver_error.set(String::new());
        versions.set(Vec::new());
        modal.set(Modal::Versions(t.clone()));
        ver_loading.set(true);
        let tid = t.id.clone();
        spawn(async move {
            match crate::api::get::<ListVersionsResponse>(&format!("/templates/{tid}/versions")).await {
                Ok(resp) => {
                    versions.set(resp.versions);
                    ver_error.set(String::new());
                }
                Err(e) => ver_error.set(e.message),
            }
            ver_loading.set(false);
        });
    };

    let open_rollback = move |v: TemplateVersionDto| {
        form_error.set(String::new());
        let current = modal.read().clone();
        if let Modal::Versions(t) = current {
            modal.set(Modal::Rollback { template: t, version: v });
        }
    };

    let open_generate = move |t: TemplateDto| {
        gen_profile.set("mihomo".to_string());
        gen_mode.set("lenient".to_string());
        gen_result.set(None);
        gen_error.set(String::new());
        is_generate.set(true);
        modal.set(Modal::Generate(t));
    };

    let open_preview = move |t: TemplateDto| {
        gen_profile.set("mihomo".to_string());
        gen_mode.set("lenient".to_string());
        gen_result.set(None);
        gen_error.set(String::new());
        is_generate.set(false);
        modal.set(Modal::Preview(t));
    };

    let close_modal = move |_| {
        modal.set(Modal::None);
    };

    let do_submit = move |_| {
        let state = (*modal.read()).clone();
        match state {
            Modal::Create => {
                let name = f_name.read().clone();
                if name.is_empty() || f_spec.read().is_empty() {
                    form_error.set("Name and spec YAML are required".to_string());
                    return;
                }
                let req = CreateTemplateRequest {
                    name,
                    description: f_desc.read().clone(),
                    spec_yaml: f_spec.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    match crate::api::send::<TemplateResponse, CreateTemplateRequest>(
                        "POST", "/templates", Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_templates();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Edit(t) => {
                let id = t.id.clone();
                let name = f_name.read().clone();
                if name.is_empty() || f_spec.read().is_empty() {
                    form_error.set("Name and spec YAML are required".to_string());
                    return;
                }
                let req = UpdateTemplateRequest {
                    name,
                    description: f_desc.read().clone(),
                    spec_yaml: f_spec.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    let path = format!("/templates/{id}");
                    match crate::api::send::<TemplateResponse, UpdateTemplateRequest>(
                        "PUT", &path, Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_templates();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Delete(t) => {
                let id = t.id.clone();
                saving.set(true);
                spawn(async move {
                    let path = format!("/templates/{id}");
                    match crate::api::delete(&path).await {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_templates();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            _ => {}
        }
    };

    let do_rollback = move |v: TemplateVersionDto| {
        let state = (*modal.read()).clone();
        let tid = match state {
            Modal::Rollback { template, .. } => template.id.clone(),
            _ => return,
        };
        let req = RollbackRequest { version_id: v.id.clone() };
        saving.set(true);
        spawn(async move {
            let path = format!("/templates/{tid}/rollback");
            match crate::api::send::<RollbackTemplateResponse, RollbackRequest>(
                "POST", &path, Some(&req),
            )
            .await
            {
                Ok(_) => {
                    modal.set(Modal::None);
                    fetch_templates();
                }
                Err(e) => form_error.set(e.message),
            }
            saving.set(false);
        });
    };

    let do_gen = move |_| {
        let state = (*modal.read()).clone();
        let tid = match &state {
            Modal::Generate(t) | Modal::Preview(t) => t.id.clone(),
            _ => return,
        };
        let profile = gen_profile.read().clone();
        let mode = gen_mode.read().clone();
        let action = if *is_generate.read() { "generate" } else { "preview" };
        gen_loading.set(true);
        gen_error.set(String::new());
        spawn(async move {
            let path = format!("/templates/{tid}/{action}?profile={profile}&mode={mode}");
            match crate::api::send::<GenerationResultDto, serde_json::Value>("POST", &path, None)
                .await
            {
                Ok(resp) => gen_result.set(Some(resp)),
                Err(e) => gen_error.set(e.message),
            }
            gen_loading.set(false);
        });
    };

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.templates")} }
                button {
                    class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: open_create,
                    {t(l, "tpl.add")}
                }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if templates.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "tpl.empty")} }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "tpl.name")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "tpl.version")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "tpl.updated_at")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for tmpl in templates.read().iter() {
                                {
                                    let id = tmpl.id.clone();
                                    let edit_t = tmpl.clone();
                                    let del_t = tmpl.clone();
                                    let ver_t = tmpl.clone();
                                    let gen_t = tmpl.clone();
                                    let prev_t = tmpl.clone();
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3",
                                                div { class: "font-medium text-stone-900 dark:text-stone-100", "{tmpl.name}" }
                                                if !tmpl.description.is_empty() {
                                                    div { class: "text-xs text-stone-500 dark:text-stone-400", "{tmpl.description}" }
                                                }
                                            }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400", "v{tmpl.active_version}" }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{tmpl.updated_at}" }
                                            td { class: "px-4 py-3 text-right",
                                                div { class: "flex flex-wrap justify-end gap-1",
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_edit(edit_t.clone()),
                                                        {t(l, "common.edit")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_versions(ver_t.clone()),
                                                        {t(l, "tpl.versions")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_preview(prev_t.clone()),
                                                        {t(l, "tpl.preview")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-amber-300 px-2 py-1 text-xs text-amber-700 hover:bg-amber-50 dark:border-amber-700 dark:text-amber-400 dark:hover:bg-amber-900/20",
                                                        onclick: move |_| open_generate(gen_t.clone()),
                                                        {t(l, "tpl.generate")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-red-300 px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                                                        onclick: move |_| open_delete(del_t.clone()),
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

        TemplateModals {
            lang: props.lang,
            modal,
            f_name,
            f_desc,
            f_spec,
            form_error,
            saving,
            on_close: close_modal,
            on_submit: do_submit,
        }

        TemplateVersions {
            lang: props.lang,
            modal,
            versions,
            loading: ver_loading,
            error: ver_error,
            form_error,
            saving,
            on_close: close_modal,
            on_rollback: open_rollback,
        }

        TemplateGenModal {
            lang: props.lang,
            modal,
            gen_profile,
            gen_mode,
            gen_result,
            gen_loading,
            gen_error,
            is_generate: *is_generate.read(),
            on_close: close_modal,
            on_run: do_gen,
        }
    }
}
