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
    let show = matches!(*props.modal.read(), Modal::Generate(_) | Modal::Preview(_));
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
                    class: "node-dialog-panel w-full max-w-3xl rounded-lg bg-surface p-6 shadow-xl",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-foreground", "{title}" }

                    div { class: "mt-4 flex flex-wrap gap-4",
                        div {
                            label { class: "block text-sm font-medium text-foreground", {t(l, "tpl.profile")} }
                            select {
                                class: "mt-1 block rounded-md border border-line px-3 py-2 text-sm",
                                value: "{props.gen_profile}",
                                disabled: *props.gen_loading.read(),
                                onchange: move |e| { props.gen_profile.set(e.value()); props.gen_result.set(None); props.gen_error.set(String::new()); },
                                for p in PROFILES.iter().copied() {
                                    option { value: "{p}", "{p}" }
                                }
                            }
                        }
                        div {
                            label { class: "block text-sm font-medium text-foreground", {t(l, "tpl.mode")} }
                            select {
                                class: "mt-1 block rounded-md border border-line px-3 py-2 text-sm",
                                value: "{props.gen_mode}",
                                disabled: *props.gen_loading.read(),
                                onchange: move |e| { props.gen_mode.set(e.value()); props.gen_result.set(None); props.gen_error.set(String::new()); },
                                option { value: "lenient", "lenient" }
                                option { value: "strict", "strict" }
                            }
                        }
                        div { class: "flex items-end",
                            button {
                                class: "rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50",
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
                                div { class: "rounded-md bg-accent-soft p-3",
                                    p { class: "text-sm font-medium text-accent-text", {t(l, "tpl.warnings")} }
                                    ul { class: "mt-1 list-disc pl-5",
                                        for w in result.warnings.iter() {
                                            li { class: "text-xs text-accent-text", "{w}" }
                                        }
                                    }
                                }
                            }
                            div { class: "flex flex-wrap gap-4 text-xs",
                                span { "{result.profile}" }
                                span { class: "text-muted",
                                    "{t(l, \"tpl.included\")}: {result.included_node_ids.len()}"
                                }
                                span { class: "text-muted",
                                    "{t(l, \"tpl.excluded\")}: {result.excluded.len()}"
                                }
                            }
                            if !result.excluded.is_empty() {
                                details { class: "rounded-md border border-line-soft",
                                    summary { class: "cursor-pointer px-3 py-2 text-sm text-muted", {t(l, "tpl.excluded_nodes")} }
                                    ul { class: "px-3 pb-2 list-disc pl-5",
                                        for ex in result.excluded.iter() {
                                            li { class: "text-xs text-muted", "{ex.display_name} ({ex.node_id}): {ex.reason}" }
                                        }
                                    }
                                }
                            }
                            div {
                                div { class: "flex items-center justify-between",
                                    label { class: "text-sm font-medium text-foreground", {t(l, "tpl.output")} }
                                    button {
                                        class: "rounded-md border border-line px-2 py-1 text-xs text-muted hover:bg-surface-muted",
                                        onclick: move |_| {
                                            let c = props.gen_result.read().as_ref().map(|r| r.content.clone()).unwrap_or_default();
                                            spawn(async move { let _ = copy_to_clipboard(&c).await; });
                                        },
                                        {t(l, "common.copy")}
                                    }
                                }
                                textarea {
                                    class: "mt-1 block w-full rounded-md border border-line px-3 py-2 font-mono text-xs",
                                    rows: "20",
                                    readonly: true,
                                    "{result.content}",
                                }
                            }
                        }
                    }

                    div { class: "mt-6 flex justify-end",
                        button { class: "rounded-md border border-line px-4 py-2 text-sm text-muted hover:bg-surface-muted", onclick: move |_| props.on_close.call(()), {t(l, "common.close")} }
                    }
                }
            }
        }
    }
}
