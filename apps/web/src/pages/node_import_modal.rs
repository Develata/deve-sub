//! Import nodes modal — manual paste of subscription content (NODE-001/002).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::send;
use crate::i18n::{Language, format_t, t};
use crate::pages::node_types::{ImportNodesRequest, ImportNodesResponse, SourceTypeDto};

#[derive(Props, Clone, PartialEq)]
pub struct ImportModalProps {
    lang: Signal<Language>,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

pub fn ImportModal(props: ImportModalProps) -> Element {
    let l = *props.lang.read();
    let mut content = use_signal(String::new);
    let mut source_type = use_signal(|| SourceTypeDto::Auto);
    let mut saving = use_signal(|| false);
    let mut result = use_signal(|| Option::<ImportNodesResponse>::None);
    let mut error = use_signal(String::new);

    let do_import = move |_| {
        let c = content.read().clone();
        if c.trim().is_empty() {
            error.set(t(l, "nodes.import_empty").to_string());
            return;
        }
        saving.set(true);
        error.set(String::new());
        let st = *source_type.read();
        spawn(async move {
            let req = ImportNodesRequest {
                content: c,
                source_type: st,
            };
            match send::<ImportNodesResponse, _>("POST", "/nodes/import", Some(&req)).await {
                Ok(resp) => {
                    result.set(Some(resp));
                    saving.set(false);
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    let has_result = result.read().is_some();

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            onclick: move |_| props.on_close.call(()),
            div {
                class: "w-full max-w-2xl rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                onclick: move |e| e.stop_propagation(),
                h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                    {t(l, "nodes.import_title")}
                }

                if let Some(r) = result.read().as_ref() {
                    // Results view.
                    div { class: "mt-4 space-y-3",
                        div { class: "rounded-md bg-green-50 p-3 text-sm text-green-700 dark:bg-green-900/20 dark:text-green-400",
                            {format_t(l, "nodes.import_new", r.new_nodes as usize)}
                        }
                        div { class: "rounded-md bg-amber-50 p-3 text-sm text-amber-700 dark:bg-amber-900/20 dark:text-amber-400",
                            {format_t(l, "nodes.import_dup", r.duplicate_nodes as usize)}
                        }
                        if r.failed > 0 {
                            div { class: "rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400",
                                {format_t(l, "nodes.import_failed", r.failed as usize)}
                            }
                        }
                        div { class: "mt-4 flex justify-end",
                            button {
                                class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                                onclick: move |_| {
                                    props.on_success.call(());
                                    props.on_close.call(());
                                },
                                {t(l, "common.done")}
                            }
                        }
                    }
                } else {
                    // Import form.
                    div { class: "mt-4 space-y-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                                {t(l, "nodes.import_format")}
                            }
                            select {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{source_type.read().as_str()}",
                                onchange: move |e| {
                                    let v = e.value();
                                    for st in SourceTypeDto::all() {
                                        if st.as_str() == v {
                                            source_type.set(*st);
                                            break;
                                        }
                                    }
                                },
                                for st in SourceTypeDto::all() {
                                    option { value: "{st.as_str()}", "{st.as_str()}" }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                                {t(l, "nodes.import_content")}
                            }
                            textarea {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 font-mono text-sm dark:border-stone-700 dark:bg-stone-800",
                                rows: "12",
                                placeholder: "vless://...\ntrojan://...\nss://...",
                                value: "{content}",
                                oninput: move |e| content.set(e.value()),
                            }
                        }
                        if !error.read().is_empty() {
                            div { class: "rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400",
                                "{error}"
                            }
                        }
                        div { class: "flex justify-end gap-2",
                            button {
                                class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                onclick: move |_| props.on_close.call(()),
                                {t(l, "common.cancel")}
                            }
                            button {
                                class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                                disabled: *saving.read(),
                                onclick: do_import,
                                if *saving.read() { {t(l, "common.loading")} } else { {t(l, "nodes.import_btn")} }
                            }
                        }
                    }
                }

            }
        }
    }
}
