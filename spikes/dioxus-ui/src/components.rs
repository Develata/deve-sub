use dioxus::prelude::*;

use crate::i18n::{t, Language};

#[component]
pub fn Button(
    text: String,
    onclick: EventHandler<()>,
    variant: Option<String>,
) -> Element {
    let base = "inline-flex items-center justify-center rounded-md px-4 py-2 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500";
    let variant_class = match variant.as_deref().unwrap_or("primary") {
        "primary" => "bg-amber-600 text-white hover:bg-amber-500 dark:bg-amber-600 dark:hover:bg-amber-500",
        "secondary" => "bg-stone-200 text-stone-900 hover:bg-stone-300 dark:bg-stone-800 dark:text-stone-100 dark:hover:bg-stone-700",
        "ghost" => "text-stone-600 hover:bg-stone-100 dark:text-stone-300 dark:hover:bg-stone-800",
        _ => "bg-amber-600 text-white hover:bg-amber-500",
    };
    rsx! {
        button {
            class: "{base} {variant_class}",
            onclick: move |_| onclick.call(()),
            "{text}"
        }
    }
}

#[component]
pub fn Card(title: Option<String>, children: Element) -> Element {
    rsx! {
        div {
            class: "rounded-lg border border-stone-200 bg-white p-5 shadow-sm dark:border-stone-800 dark:bg-stone-900",
            if let Some(t) = title {
                h3 { class: "mb-4 text-sm font-semibold text-stone-500 dark:text-stone-400 uppercase tracking-wide", "{t}" }
            }
            {children}
        }
    }
}

#[component]
pub fn Badge(text: String, color: Option<String>) -> Element {
    let class = match color.as_deref().unwrap_or("gray") {
        "green" => "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
        "red" => "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400",
        "amber" => "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400",
        _ => "bg-stone-100 text-stone-600 dark:bg-stone-800 dark:text-stone-400",
    };
    rsx! {
        span { class: "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {class}", "{text}" }
    }
}

#[component]
pub fn Modal(open: bool, title: String, on_close: EventHandler<()>, children: Element) -> Element {
    if !open {
        return rsx! {};
    }
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            onclick: move |_| on_close.call(()),
            div {
                class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                onclick: move |evt| evt.stop_propagation(),
                div {
                    class: "mb-4 flex items-center justify-between",
                    h2 { class: "text-lg font-semibold", "{title}" }
                    button {
                        class: "text-stone-400 hover:text-stone-600 dark:hover:text-stone-200",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                {children}
            }
        }
    }
}

#[component]
pub fn SearchInput(value: String, placeholder: String, on_input: EventHandler<String>) -> Element {
    rsx! {
        input {
            r#type: "text",
            class: "w-full rounded-md border border-stone-300 bg-white px-3 py-2 text-sm placeholder-stone-400 focus:border-amber-500 focus:outline-none dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
            placeholder: "{placeholder}",
            value: "{value}",
            oninput: move |evt| on_input.call(evt.value()),
        }
    }
}

#[component]
pub fn Select(value: String, options: Vec<(String, String)>, on_change: EventHandler<String>) -> Element {
    rsx! {
        select {
            class: "rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-amber-500 focus:outline-none dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
            value: "{value}",
            onchange: move |evt| on_change.call(evt.value()),
            for (val, label) in options {
                option { value: "{val}", selected: val == value, "{label}" }
            }
        }
    }
}

#[component]
pub fn ProgressBar(value: f64) -> Element {
    let pct = value.clamp(0.0, 100.0);
    let width = format!("{pct:.0}%");
    rsx! {
        div {
            class: "h-3 w-full overflow-hidden rounded-full bg-stone-200 dark:bg-stone-800",
            div {
                class: "h-full rounded-full bg-amber-500 transition-all duration-150 ease-out",
                style: "width: {width}",
            }
        }
    }
}

#[component]
pub fn StatCard(label: String, value: String, sub: Option<String>, emphasis: bool) -> Element {
    let size_class = if emphasis {
        "text-4xl font-bold text-stone-900 dark:text-stone-50"
    } else {
        "text-2xl font-semibold text-stone-600 dark:text-stone-300"
    };
    rsx! {
        div {
            class: "rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900",
            p { class: "text-xs font-medium uppercase tracking-wide text-stone-400", "{label}" }
            p { class: "mt-2 {size_class}", "{value}" }
            if let Some(s) = sub {
                p { class: "mt-1 text-sm text-stone-400", "{s}" }
            }
        }
    }
}

#[component]
pub fn ThemeToggle(theme: String, on_change: EventHandler<String>) -> Element {
    let lang = use_context::<Signal<Language>>();
    let label = t(*lang.read(), "settings.theme");
    rsx! {
        div { class: "flex items-center gap-3",
            span { class: "text-sm text-stone-500", "{label}" }
            for (key, name_key) in [("light", "settings.light"), ("dark", "settings.dark"), ("amber", "settings.custom")] {
                button {
                    class: if key == theme {
                        "rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-white"
                    } else {
                        "rounded-md bg-stone-100 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-stone-200 dark:bg-stone-800 dark:text-stone-300 dark:hover:bg-stone-700"
                    },
                    onclick: move |_| on_change.call(key.to_string()),
                    {t(*lang.read(), name_key)}
                }
            }
        }
    }
}

#[component]
pub fn LanguageSwitcher(lang: Signal<Language>, on_change: EventHandler<Language>) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            button {
                class: if *lang.read() == Language::Zh {
                    "rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-white"
                } else {
                    "rounded-md bg-stone-100 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-stone-200 dark:bg-stone-800 dark:text-stone-300 dark:hover:bg-stone-700"
                },
                onclick: move |_| on_change.call(Language::Zh),
                "中文"
            }
            button {
                class: if *lang.read() == Language::En {
                    "rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-white"
                } else {
                    "rounded-md bg-stone-100 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-stone-200 dark:bg-stone-800 dark:text-stone-300 dark:hover:bg-stone-700"
                },
                onclick: move |_| on_change.call(Language::En),
                "EN"
            }
        }
    }
}
