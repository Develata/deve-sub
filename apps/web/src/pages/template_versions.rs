//! Versions list modal and rollback confirmation for the templates page.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::template_types::{Modal, TemplateVersionDto};

#[derive(Props, Clone)]
pub struct TemplateVersionsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    versions: Signal<Vec<TemplateVersionDto>>,
    loading: Signal<bool>,
    error: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_rollback: EventHandler<TemplateVersionDto>,
}

pub fn TemplateVersions(props: TemplateVersionsProps) -> Element {
    let l = *props.lang.read();
    let is_versions_modal = matches!(*props.modal.read(), Modal::Versions(_));
    let is_rollback_modal = matches!(*props.modal.read(), Modal::Rollback { .. });

    let rollback_version = match &*props.modal.read() {
        Modal::Rollback { version, .. } => Some(version.clone()),
        _ => None,
    };

    rsx! {
        if is_versions_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-2xl rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "tpl.versions_title")} }

                    if *props.loading.read() {
                        div { class: "mt-4 flex items-center justify-center py-8",
                            div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                        }
                    } else if !props.error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.error}" }
                    } else {
                        div { class: "mt-4 max-h-96 overflow-y-auto",
                            if props.versions.read().is_empty() {
                                p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "tpl.no_versions")} }
                            } else {
                                table { class: "w-full text-sm",
                                    thead {
                                        tr { class: "border-b border-stone-200 dark:border-stone-800",
                                            th { class: "px-3 py-2 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "tpl.version")} }
                                            th { class: "px-3 py-2 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "tpl.created_at")} }
                                            th { class: "px-3 py-2 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                            th { class: "px-3 py-2 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                                        }
                                    }
                                    tbody {
                                        for ver in props.versions.read().iter() {
                                            {
                                                let v = ver.clone();
                                                let is_active = ver.is_active;
                                                rsx! {
                                                    tr { class: "border-b border-stone-100 dark:border-stone-800",
                                                        td { class: "px-3 py-2 font-mono text-xs", "v{ver.version}" }
                                                        td { class: "px-3 py-2 text-xs text-stone-500 dark:text-stone-400", "{ver.created_at}" }
                                                        td { class: "px-3 py-2",
                                                            if is_active {
                                                                span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "tpl.active")} }
                                                            } else {
                                                                span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "tpl.inactive")} }
                                                            }
                                                        }
                                                        td { class: "px-3 py-2 text-right",
                                                            if !is_active {
                                                                button {
                                                                    class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                                    onclick: move |_| props.on_rollback.call(v.clone()),
                                                                    {t(l, "tpl.rollback")}
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

                    div { class: "mt-6 flex justify-end",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.close")} }
                    }
                }
            }
        }

        if is_rollback_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "tpl.rollback_title")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400",
                        {t(l, "tpl.rollback_confirm")}
                        if let Some(v) = &rollback_version {
                            span { class: "font-mono", " v{v.version}" }
                        }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                            disabled: *props.saving.read(),
                            onclick: move |_| {
                                if let Some(v) = &rollback_version {
                                    props.on_rollback.call(v.clone());
                                }
                            },
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "tpl.rollback")} }
                        }
                    }
                }
            }
        }
    }
}
