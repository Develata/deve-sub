//! Import nodes modal — manual paste of subscription content (NODE-001/002).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::send;
use crate::i18n::{Language, format_t, t};
use crate::pages::modal_dialog::ModalDialog;
use crate::pages::node_types::{ImportNodesRequest, ImportNodesResponse, SourceTypeDto};
use crate::pages::source_types::SourceTypePresentation;

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

    let close = move |_| {
        if !*saving.read() {
            props.on_close.call(());
        }
    };
    let do_import = move |event: FormEvent| {
        event.prevent_default();
        if *saving.read() || result.read().is_some() {
            return;
        }
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
                    // WHY: refresh on completion, so every exit from the result
                    // view (including Escape) returns to the committed pool.
                    props.on_success.call(());
                }
                Err(e) => error.set(e.message),
            }
            saving.set(false);
        });
    };

    rsx! {
        ModalDialog { title: t(l, "nodes.import_title").to_string(), busy: *saving.read(), wide: true, on_close: close,
            if let Some(r) = result.read().as_ref() {
                div { class: "mt-4 space-y-3", role: "status",
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
                        button { r#type: "button",
                            onmounted: move |event| async move { let _ = event.data().set_focus(true).await; },
                            class: "min-h-11 rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                            onclick: move |_| close(()), {t(l, "common.done")}
                        }
                    }
                }
            } else {
                form { onsubmit: do_import,
                    fieldset { disabled: *saving.read(), class: "mt-4 min-w-0 space-y-4",
                        div {
                            label { r#for: "node-import-format", class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                                {t(l, "nodes.import_format")}
                            }
                            select { id: "node-import-format", autofocus: true,
                                class: "source-input",
                                value: "{source_type.read().as_str()}",
                                onchange: move |e| {
                                    let v = e.value();
                                    for st in &SourceTypeDto::ALL {
                                        if st.as_str() == v {
                                            source_type.set(*st);
                                            break;
                                        }
                                    }
                                },
                                for st in &SourceTypeDto::ALL {
                                    option { value: "{st.as_str()}", "{st.as_str()}" }
                                }
                            }
                        }
                        div {
                            label { r#for: "node-import-content", class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                                {t(l, "nodes.import_content")}
                            }
                            textarea { id: "node-import-content", spellcheck: false,
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 font-mono text-sm dark:border-stone-700 dark:bg-stone-800",
                                rows: "12", placeholder: "vless://...\ntrojan://...\nss://...",
                                value: "{content}", oninput: move |e| content.set(e.value()),
                            }
                        }
                        if !error.read().is_empty() {
                            // WHY: disabling the pending form releases focus;
                            // restore it after the error is mounted and announced.
                            div { role: "alert", tabindex: "-1",
                                onmounted: move |event| async move { let _ = event.data().set_focus(true).await; },
                                class: "rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
                        }
                        div { class: "flex justify-end gap-2",
                            button { r#type: "button", class: "node-control", onclick: move |_| close(()), {t(l, "common.cancel")} }
                            button { r#type: "submit",
                                class: "min-h-11 rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                                if *saving.read() { {t(l, "common.loading")} } else { {t(l, "nodes.import_btn")} }
                            }
                        }
                    }
                }
            }
        }
    }
}
