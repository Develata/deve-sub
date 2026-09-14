//! Source editor and deletion confirmation; server owns update semantics.
#![cfg(target_family = "wasm")]
use super::source_types::{SourceDto, SourceTypeDto, SourceTypePresentation};
use crate::i18n::{Language, t};
use dioxus::prelude::*;

/// Active source-management dialog and its server-owned record.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Edit(SourceDto),
    Delete(SourceDto),
}

/// Parent-owned source form fields and command callbacks.
#[derive(Props, Clone, PartialEq)]
pub struct SourceModalsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    f_name: Signal<String>,
    f_url: Signal<String>,
    f_type: Signal<SourceTypeDto>,
    f_auto: Signal<bool>,
    f_interval: Signal<u64>,
    f_keep: Signal<bool>,
    f_enabled: Signal<bool>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<MouseEvent>,
    on_submit: EventHandler<MouseEvent>,
}

/// Render source editing and deletion without owning persistence.
pub fn SourceModals(props: SourceModalsProps) -> Element {
    let SourceModalsProps {
        lang,
        modal,
        mut f_name,
        mut f_url,
        mut f_type,
        mut f_auto,
        mut f_interval,
        mut f_keep,
        mut f_enabled,
        form_error,
        saving,
        on_close: close_modal,
        on_submit: do_submit,
    } = props;
    let l = *lang.read();
    let is_form_modal = matches!(*modal.read(), Modal::Create | Modal::Edit(_));
    let is_delete_modal = matches!(*modal.read(), Modal::Delete(_));
    let is_edit = matches!(*modal.read(), Modal::Edit(_));

    rsx! {
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
