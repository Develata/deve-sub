//! Source editor and deletion confirmation; server owns update semantics.
#![cfg(target_family = "wasm")]
use super::source_dialog::SourceDialog;
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
    f_interval: Signal<String>,
    f_keep: Signal<bool>,
    f_enabled: Signal<bool>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_submit: EventHandler<()>,
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

    let title = if is_form_modal {
        if is_edit {
            t(l, "sources.edit_title")
        } else {
            t(l, "sources.add")
        }
    } else {
        t(l, "common.delete")
    };

    rsx! {
        if is_form_modal || is_delete_modal {
            SourceDialog { title, busy: *saving.read(), on_close: close_modal,
                if is_form_modal {
                    form {
                        onsubmit: move |event| { event.prevent_default(); do_submit.call(()); },
                        fieldset { disabled: *saving.read(), class: "mt-4 min-w-0 space-y-4",
                            div {
                                label { r#for: "source-name", class: "block text-sm font-medium", {t(l, "sources.name")} }
                                input { id: "source-name", class: "source-input", r#type: "text", required: true, autofocus: true,
                                    value: "{f_name}", oninput: move |e| f_name.set(e.value()), }
                            }
                            div {
                                label { r#for: "source-url", class: "block text-sm font-medium", {t(l, "sources.url")} }
                                input { id: "source-url", class: "source-input", r#type: "url", required: !is_edit,
                                    aria_describedby: if is_edit { Some("source-url-hint") } else { None },
                                    placeholder: if is_edit { t(l, "sources.url_keep") } else { "https://" },
                                    autocomplete: "off", spellcheck: false,
                                    value: "{f_url}", oninput: move |e| f_url.set(e.value()), }
                                if let Modal::Edit(source) = &*modal.read() {
                                    p { id: "source-url-hint", class: "mt-2 break-words text-xs text-stone-500 dark:text-stone-400",
                                        {t(l, "sources.url_keep")} " · {source.url}"
                                    }
                                }
                            }
                            div {
                                label { r#for: "source-type", class: "block text-sm font-medium", {t(l, "sources.source_type")} }
                                select { id: "source-type", class: "source-input", value: "{f_type.read().as_str()}",
                                    onchange: move |e| f_type.set(SourceTypeDto::from_str(&e.value())),
                                    for st in SourceTypeDto::ALL.iter().copied() {
                                        option { value: "{st.as_str()}", {st.label(l)} }
                                    }
                                }
                            }
                            div {
                                label { r#for: "source-interval", class: "block text-sm font-medium", {t(l, "sources.update_interval")} }
                                input { id: "source-interval", class: "source-input", r#type: "number", required: true, min: "1", step: "1",
                                    value: "{f_interval}", oninput: move |e| f_interval.set(e.value()), }
                            }
                            div { class: "flex flex-wrap items-center gap-x-4 gap-y-2",
                                label { class: "flex min-h-11 items-center gap-2 text-sm",
                                    input { r#type: "checkbox", checked: *f_auto.read(), onchange: move |e| f_auto.set(e.checked()), }
                                    {t(l, "sources.auto_update")}
                                }
                                label { class: "flex min-h-11 items-center gap-2 text-sm",
                                    input { r#type: "checkbox", checked: *f_keep.read(), onchange: move |e| f_keep.set(e.checked()), }
                                    {t(l, "sources.keep_on_fail")}
                                }
                            }
                            if is_edit {
                                label { class: "flex min-h-11 items-center gap-2 text-sm",
                                    input { r#type: "checkbox", checked: *f_enabled.read(), onchange: move |e| f_enabled.set(e.checked()), }
                                    {t(l, "nodes.enabled")}
                                }
                            }
                            if !form_error.read().is_empty() {
                                p { role: "alert", class: "rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{form_error}" }
                            }
                            div { class: "flex justify-end gap-2 pt-2",
                                button { r#type: "button", class: "node-control", onclick: move |_| close_modal.call(()), {t(l, "common.cancel")} }
                                button { r#type: "submit", class: "min-h-11 rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                                    if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                                }
                            }
                        }
                    }
                } else {
                    if let Modal::Delete(source) = &*modal.read() {
                        p { class: "mt-4 break-words font-medium", "{source.name}" }
                    }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400", {t(l, "sources.delete_confirm")} }
                    if !form_error.read().is_empty() {
                        p { role: "alert", class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { r#type: "button", class: "node-control", autofocus: true, disabled: *saving.read(),
                            onclick: move |_| close_modal.call(()), {t(l, "common.cancel")} }
                        button { r#type: "button", class: "min-h-11 rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50",
                            disabled: *saving.read(), onclick: move |_| do_submit.call(()),
                            if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.delete")} }
                        }
                    }
                }
            }
        }
    }
}
