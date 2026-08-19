//! Form (create/edit) and delete modal dialogs for the templates page.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::template_types::Modal;

#[derive(Props, Clone)]
pub struct TemplateModalsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    f_name: Signal<String>,
    f_desc: Signal<String>,
    f_spec: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_submit: EventHandler<()>,
}

pub fn TemplateModals(props: TemplateModalsProps) -> Element {
    let l = *props.lang.read();
    let is_form_modal = matches!(*props.modal.read(), Modal::Create | Modal::Edit(_));
    let is_delete_modal = matches!(*props.modal.read(), Modal::Delete(_));
    let is_edit = matches!(*props.modal.read(), Modal::Edit(_));

    rsx! {
        if is_form_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-2xl rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                        if is_edit { {t(l, "tpl.edit_title")} } else { {t(l, "tpl.add")} }
                    }
                    div { class: "mt-4 space-y-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.name")} }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "text", value: "{props.f_name}", oninput: move |e| props.f_name.set(e.value()) }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.description")} }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "text", value: "{props.f_desc}", oninput: move |e| props.f_desc.set(e.value()) }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.spec_yaml")} }
                            textarea {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 font-mono text-xs dark:border-stone-700 dark:bg-stone-800",
                                rows: "20",
                                value: "{props.f_spec}",
                                oninput: move |e| props.f_spec.set(e.value()),
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

        if is_delete_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "common.delete")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400", {t(l, "tpl.delete_confirm")} }
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
