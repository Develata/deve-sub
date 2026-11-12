//! Versions list modal and rollback confirmation for the templates page.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::template_types::{Modal, TemplateVersionDto};

#[derive(Props, Clone, PartialEq)]
pub struct TemplateVersionsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    versions: Signal<Vec<TemplateVersionDto>>,
    next_version: Signal<Option<u64>>,
    on_load_more: EventHandler<()>,
    loading: Signal<bool>,
    error: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_rollback: EventHandler<TemplateVersionDto>,
    on_confirm_rollback: EventHandler<TemplateVersionDto>,
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
                    class: "node-dialog-panel w-full max-w-2xl rounded-lg bg-surface p-6 shadow-xl",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-foreground", {t(l, "tpl.versions_title")} }

                    if *props.loading.read() {
                        div { class: "mt-4 flex items-center justify-center py-8",
                            div { class: "h-6 w-6 animate-spin rounded-full border-2 border-line border-t-accent" }
                        }
                    } else if !props.error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.error}" }
                    } else {
                        div { class: "mt-4 max-h-96 overflow-y-auto",
                            if props.versions.read().is_empty() {
                                p { class: "text-sm text-muted", {t(l, "tpl.no_versions")} }
                            } else {
                                table { class: "w-full text-sm",
                                    thead {
                                        tr { class: "border-b border-line-soft",
                                            th { class: "px-3 py-2 text-left font-medium text-muted", {t(l, "tpl.version")} }
                                            th { class: "px-3 py-2 text-left font-medium text-muted", {t(l, "tpl.created_at")} }
                                            th { class: "px-3 py-2 text-left font-medium text-muted", {t(l, "nodes.status")} }
                                            th { class: "px-3 py-2 text-right font-medium text-muted", "" }
                                        }
                                    }
                                    tbody {
                                        for ver in props.versions.read().iter() {
                                            {
                                                let v = ver.clone();
                                                let is_active = ver.is_active;
                                                rsx! {
                                                    tr { class: "border-b border-line-soft",
                                                        td { class: "px-3 py-2 font-mono text-xs", "v{ver.version}" }
                                                        td { class: "px-3 py-2 text-xs text-muted", "{ver.created_at}" }
                                                        td { class: "px-3 py-2",
                                                            if is_active {
                                                                span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "tpl.active")} }
                                                            } else {
                                                                span { class: "inline-flex rounded-full bg-surface-muted px-2 py-0.5 text-xs font-medium text-muted", {t(l, "tpl.inactive")} }
                                                            }
                                                        }
                                                        td { class: "px-3 py-2 text-right",
                                                            if !is_active {
                                                                button {
                                                                    class: "rounded-md border border-line px-2 py-1 text-xs text-muted hover:bg-surface-muted",
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

                    if props.next_version.read().is_some() {
                        button { class: "node-control mt-3", disabled: *props.loading.read(), onclick: move |_| props.on_load_more.call(()), {t(l, "nodes.load_more")} }
                    }
                    div { class: "mt-6 flex justify-end",
                        button { class: "rounded-md border border-line px-4 py-2 text-sm text-muted hover:bg-surface-muted", onclick: move |_| props.on_close.call(()), {t(l, "common.close")} }
                    }
                }
            }
        }

        if is_rollback_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "node-dialog-panel w-full max-w-md rounded-lg bg-surface p-6 shadow-xl",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-foreground", {t(l, "tpl.rollback_title")} }
                    p { class: "mt-3 text-sm text-muted",
                        {t(l, "tpl.rollback_confirm")}
                        if let Some(v) = &rollback_version {
                            span { class: "font-mono", " v{v.version}" }
                        }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-line px-4 py-2 text-sm text-muted hover:bg-surface-muted", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50",
                            disabled: *props.saving.read(),
                            onclick: move |_| {
                                if let Some(v) = &rollback_version {
                                    props.on_confirm_rollback.call(v.clone());
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

/// Shared history request state; the dialog revision also guards loading/errors.
#[derive(Clone, Copy)]
pub struct HistoryRequest {
    pub revision: Signal<u64>,
    pub versions: Signal<Vec<TemplateVersionDto>>,
    pub next: Signal<Option<u64>>,
    pub loading: Signal<bool>,
    pub error: Signal<String>,
}

impl HistoryRequest {
    pub fn load(mut self, tid: String, before: Option<u64>) {
        let revision = *self.revision.read();
        self.loading.set(true);
        self.error.set(String::new());
        spawn(async move {
            let path = match before {
                Some(v) => format!("/templates/{tid}/versions?before_version={v}"),
                None => format!("/templates/{tid}/versions"),
            };
            let result = crate::api::get::<deve_sub_contract::ListVersionsResponse>(&path).await;
            if revision != *self.revision.read() {
                return;
            }
            match result {
                Ok(resp) => {
                    self.next.set(resp.next_before_version);
                    self.versions.write().extend(resp.versions);
                }
                Err(e) => self.error.set(e.message),
            }
            self.loading.set(false);
        });
    }
}
