//! Preview / generate modal for the templates page.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::template_types::{GenerationResultDto, Modal, PROFILES};
use crate::pages::util::copy_to_clipboard;

#[derive(Props, Clone, PartialEq)]
pub struct TemplateGenModalProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    gen_profile: Signal<String>,
    gen_mode: Signal<String>,
    gen_result: Signal<Option<GenerationResultDto>>,
    gen_loading: Signal<bool>,
    gen_error: Signal<String>,
    is_generate: bool,
    on_close: EventHandler<()>,
    on_run: EventHandler<()>,
}

pub fn TemplateGenModal(mut props: TemplateGenModalProps) -> Element {
    let l = *props.lang.read();
    let show = matches!(
        *props.modal.read(),
        Modal::Generate(_) | Modal::Preview(_)
    );
    let title = if props.is_generate {
        t(l, "tpl.generate_title")
    } else {
        t(l, "tpl.preview_title")
    };
    let run_label = if props.is_generate {
        t(l, "tpl.generate")
    } else {
        t(l, "tpl.preview")
    };

    rsx! {
        if show {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-3xl rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", "{title}" }

                    div { class: "mt-4 flex flex-wrap gap-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.profile")} }
                            select {
                                class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{props.gen_profile}",
                                onchange: move |e| props.gen_profile.set(e.value()),
                                for p in PROFILES.iter().copied() {
                                    option { value: "{p}", "{p}" }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.mode")} }
                            select {
                                class: "mt-1 block rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{props.gen_mode}",
                                onchange: move |e| props.gen_mode.set(e.value()),
                                option { value: "lenient", "lenient" }
                                option { value: "strict", "strict" }
                            }
                        }
                        div { class: "flex items-end",
                            button {
                                class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                                disabled: *props.gen_loading.read(),
                                onclick: move |_| props.on_run.call(()),
                                if *props.gen_loading.read() { {t(l, "common.loading")} } else { "{run_label}" }
                            }
                        }
                    }

                    if !props.gen_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.gen_error}" }
                    }

                    if let Some(result) = props.gen_result.read().as_ref() {
                        div { class: "mt-4 space-y-3",
                            if !result.warnings.is_empty() {
                                div { class: "rounded-md bg-amber-50 p-3 dark:bg-amber-900/20",
                                    p { class: "text-sm font-medium text-amber-700 dark:text-amber-400", {t(l, "tpl.warnings")} }
                                    ul { class: "mt-1 list-disc pl-5",
                                        for w in result.warnings.iter() {
                                            li { class: "text-xs text-amber-700 dark:text-amber-400", "{w}" }
                                        }
                                    }
                                }
                            }
                            div { class: "flex flex-wrap gap-4 text-xs",
                                span { class: "text-stone-500 dark:text-stone-400",
                                    "{t(l, \"tpl.included\")}: {result.included_node_ids.len()}"
                                }
                                span { class: "text-stone-500 dark:text-stone-400",
                                    "{t(l, \"tpl.excluded\")}: {result.excluded.len()}"
                                }
                            }
                            if !result.excluded.is_empty() {
                                details { class: "rounded-md border border-stone-200 dark:border-stone-800",
                                    summary { class: "cursor-pointer px-3 py-2 text-sm text-stone-600 dark:text-stone-400", {t(l, "tpl.excluded_nodes")} }
                                    ul { class: "px-3 pb-2 list-disc pl-5",
                                        for ex in result.excluded.iter() {
                                            li { class: "text-xs text-stone-500 dark:text-stone-400", "{ex.display_name} ({ex.node_id}): {ex.reason}" }
                                        }
                                    }
                                }
                            }
                            div {
                                div { class: "flex items-center justify-between",
                                    label { class: "text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "tpl.output")} }
                                    button {
                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                        onclick: move |_| {
                                            let c = props.gen_result.read().as_ref().map(|r| r.content.clone()).unwrap_or_default();
                                            spawn(async move { let _ = copy_to_clipboard(&c).await; });
                                        },
                                        {t(l, "common.copy")}
                                    }
                                }
                                textarea {
                                    class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 font-mono text-xs dark:border-stone-700 dark:bg-stone-800",
                                    rows: "20",
                                    readonly: true,
                                    "{result.content}",
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
    }
}
