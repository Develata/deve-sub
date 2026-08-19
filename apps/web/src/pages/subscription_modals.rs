//! Modal dialogs for the subscriptions page (create/edit/delete/token/temp-link).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::subscription_types::{Modal, PROFILES};
use crate::pages::templates::TemplateDto;
use crate::pages::util::copy_to_clipboard;

#[derive(Props, Clone)]
pub struct SubscriptionModalsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    templates: Signal<Vec<TemplateDto>>,
    f_name: Signal<String>,
    f_slug: Signal<String>,
    f_template: Signal<String>,
    f_profile: Signal<String>,
    f_traffic: Signal<Option<u64>>,
    f_expires: Signal<Option<String>>,
    f_enabled: Signal<bool>,
    f_temp_expiry: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_submit: EventHandler<()>,
}

pub fn SubscriptionModals(props: SubscriptionModalsProps) -> Element {
    let l = *props.lang.read();
    let is_form_modal = matches!(*props.modal.read(), Modal::Create | Modal::Edit(_));
    let is_delete_modal = matches!(*props.modal.read(), Modal::Delete(_));
    let is_token_modal = matches!(*props.modal.read(), Modal::TokenDisplay(_));
    let is_temp_link_modal = matches!(*props.modal.read(), Modal::TempLink(_));
    let is_edit = matches!(*props.modal.read(), Modal::Edit(_));
    let token_val = match &*props.modal.read() {
        Modal::TokenDisplay(t) => t.clone(),
        _ => String::new(),
    };

    rsx! {
        if is_form_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                        if is_edit { {t(l, "subs.edit_title")} } else { {t(l, "subs.add")} }
                    }
                    div { class: "mt-4 space-y-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", "名称" }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "text", value: "{props.f_name}", oninput: move |e| props.f_name.set(e.value()) }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.slug")} }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "text", value: "{props.f_slug}", oninput: move |e| props.f_slug.set(e.value()) }
                        }
                        if !is_edit {
                            div {
                                label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.template")} }
                                if props.templates.read().is_empty() {
                                    p { class: "mt-1 text-sm text-red-500", {t(l, "subs.no_template")} }
                                } else {
                                    select {
                                        class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                        value: "{props.f_template}",
                                        onchange: move |e| props.f_template.set(e.value()),
                                        for tmpl in props.templates.read().iter() {
                                            option { value: "{tmpl.id}", "{tmpl.name}" }
                                        }
                                    }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.profile")} }
                            select {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{props.f_profile}",
                                onchange: move |e| props.f_profile.set(e.value()),
                                for p in PROFILES.iter().copied() {
                                    option { value: "{p}", "{p}" }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.traffic_limit")} }
                            input {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                r#type: "number",
                                value: "{props.f_traffic.read().map(|v| v.to_string()).unwrap_or_default()}",
                                oninput: move |e| {
                                    let v = e.value();
                                    props.f_traffic.set(if v.is_empty() { None } else { v.parse::<u64>().ok() });
                                },
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.expires_at")} }
                            input {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                r#type: "text",
                                placeholder: "2025-12-31T23:59:59Z",
                                value: "{props.f_expires.read().clone().unwrap_or_default()}",
                                oninput: move |e| {
                                    let v = e.value();
                                    props.f_expires.set(if v.is_empty() { None } else { Some(v) });
                                },
                            }
                        }
                        if is_edit {
                            label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                                input { r#type: "checkbox", checked: *props.f_enabled.read(), onchange: move |e| props.f_enabled.set(e.checked()) }
                                {t(l, "nodes.enabled")}
                            }
                        }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }

        if is_token_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", "Token" }
                    p { class: "mt-2 text-sm font-medium text-red-600 dark:text-red-400", {t(l, "subs.token_warning")} }
                    div { class: "mt-4 flex items-center gap-2",
                        input {
                            class: "block w-full rounded-md border border-stone-300 px-3 py-2 font-mono text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            readonly: true,
                            value: "{token_val}",
                        }
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: move |_| {
                                let tv = token_val.clone();
                                spawn(async move { let _ = copy_to_clipboard(&tv).await; });
                            },
                            {t(l, "common.copy")}
                        }
                    }
                    div { class: "mt-6 flex justify-end",
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700", onclick: move |_| props.on_close.call(()), "OK" }
                    }
                }
            }
        }

        if is_temp_link_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "subs.temp_link")} }
                    div { class: "mt-4",
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "subs.temp_link_expiry")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            placeholder: "2025-12-31T23:59:59Z",
                            value: "{props.f_temp_expiry}",
                            oninput: move |e| props.f_temp_expiry.set(e.value()),
                        }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }

        if is_delete_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "common.delete")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400", {t(l, "subs.delete_confirm")} }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "common.delete")} }
                        }
                    }
                }
            }
        }
    }
}
