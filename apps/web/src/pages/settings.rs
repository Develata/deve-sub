//! Settings page — theme, language, reduce-motion, custom accent, product name.
//!
//! UI-002 (i18n), UI-003 (dark mode), UI-004 (Minimal Warm),
//! UI-005 (Fantasy Violet), UI-006 (custom theme persistence),
//! UI-007 (reduce motion).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::theme::Theme;

#[derive(Props, Clone, PartialEq)]
pub struct SettingsProps {
    theme: Signal<Theme>,
    lang: Signal<Language>,
    on_theme_change: EventHandler<Theme>,
    on_lang_change: EventHandler<Language>,
}

pub fn SettingsPage(props: SettingsProps) -> Element {
    let l = *props.lang.read();
    let current_theme = *props.theme.read();
    let mut reduce_motion = use_signal(|| crate::theme::read_reduce_motion());
    let mut custom_accent =
        use_signal(|| crate::theme::read_custom_accent().unwrap_or_else(|| "#d97706".to_string()));
    let mut use_custom_accent = use_signal(|| crate::theme::read_custom_accent().is_some());
    let mut product_name = use_signal(|| {
        crate::theme::read_custom_product_name().unwrap_or_else(|| "Deve Sub".to_string())
    });
    let mut saved_flash = use_signal(|| false);

    let theme_options: [(Theme, &str); 4] = [
        (Theme::Light, "settings.light"),
        (Theme::Dark, "settings.dark"),
        (Theme::Warm, "settings.warm"),
        (Theme::Violet, "settings.violet"),
    ];

    rsx! {
        div { class: "mx-auto max-w-2xl space-y-8",
            // Theme selection.
            section {
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.theme")} }
                div { class: "mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4",
                    for (theme, label_key) in theme_options {
                        {
                            let is_active = current_theme == theme;
                            let border = if is_active {
                                "border-amber-500 ring-2 ring-amber-500"
                            } else {
                                "border-stone-300 dark:border-stone-700"
                            };
                            let bg = match theme {
                                Theme::Light => "bg-stone-50",
                                Theme::Dark => "bg-stone-900",
                                Theme::Warm => "bg-amber-50",
                                Theme::Violet => "bg-violet-50",
                            };
                            rsx! {
                                button {
                                    key: "{label_key}",
                                    class: "rounded-lg border-2 {border} {bg} p-4 text-left transition hover:shadow-md",
                                    onclick: move |_| props.on_theme_change.call(theme),
                                    div { class: "flex h-16 flex-col justify-end",
                                        span { class: "text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, label_key)} }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Custom accent color (UI-006).
            section {
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.accent_color")} }
                div { class: "mt-4 flex items-center gap-4",
                    label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                        input {
                            r#type: "checkbox",
                            class: "rounded border-stone-300 text-amber-600 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800",
                            checked: *use_custom_accent.read(),
                            onchange: move |e| {
                                let v = e.checked();
                                use_custom_accent.set(v);
                                if v {
                                    crate::theme::apply_custom_accent(&custom_accent.read());
                                } else {
                                    crate::theme::clear_custom_accent();
                                }
                            },
                        }
                        {t(l, "settings.custom_theme")}
                    }
                    if *use_custom_accent.read() {
                        input {
                            r#type: "color",
                            class: "h-10 w-16 rounded border border-stone-300 dark:border-stone-700 dark:bg-stone-800",
                            value: "{custom_accent}",
                            onchange: move |e| {
                                let v = e.value();
                                custom_accent.set(v.clone());
                                crate::theme::apply_custom_accent(&v);
                            },
                        }
                    }
                }
            }

            // Reduce motion (UI-007).
            section {
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.reduce_motion")} }
                div { class: "mt-4",
                    label { class: "flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300",
                        input {
                            r#type: "checkbox",
                            class: "rounded border-stone-300 text-amber-600 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800",
                            checked: *reduce_motion.read(),
                            onchange: move |e| {
                                let v = e.checked();
                                reduce_motion.set(v);
                                crate::theme::apply_reduce_motion(v);
                            },
                        }
                        {t(l, "settings.reduce_motion")}
                    }
                }
            }

            // Language (UI-002).
            section {
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.language")} }
                div { class: "mt-4 flex gap-3",
                    button {
                        class: if l == Language::Zh {
                            "rounded-md bg-amber-50 px-4 py-2 text-sm font-medium text-amber-700 dark:bg-amber-900/20 dark:text-amber-400"
                        } else {
                            "rounded-md border border-stone-300 px-4 py-2 text-sm font-medium text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
                        },
                        onclick: move |_| props.on_lang_change.call(Language::Zh),
                        {t(l, "settings.chinese")}
                    }
                    button {
                        class: if l == Language::En {
                            "rounded-md bg-amber-50 px-4 py-2 text-sm font-medium text-amber-700 dark:bg-amber-900/20 dark:text-amber-400"
                        } else {
                            "rounded-md border border-stone-300 px-4 py-2 text-sm font-medium text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
                        },
                        onclick: move |_| props.on_lang_change.call(Language::En),
                        {t(l, "settings.english")}
                    }
                }
            }

            // Custom product name (UI-006).
            section {
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.product_name")} }
                div { class: "mt-4 flex items-center gap-3",
                    input {
                        class: "block w-64 rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                        r#type: "text",
                        value: "{product_name}",
                        oninput: move |e| product_name.set(e.value()),
                    }
                    button {
                        class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                        onclick: move |_| {
                            crate::theme::save_custom_product_name(&product_name.read());
                            saved_flash.set(true);
                        },
                        {t(l, "common.save")}
                    }
                    if *saved_flash.read() {
                        span { class: "text-sm text-green-600 dark:text-green-400", "✓" }
                    }
                }
            }
        }
    }
}
