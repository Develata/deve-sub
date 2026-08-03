use dioxus::prelude::*;

use crate::components::{LanguageSwitcher, ThemeToggle};
use crate::i18n::{t, Language};

#[component]
pub fn SettingsPage(
    theme: String,
    on_theme_change: EventHandler<String>,
    lang: Signal<Language>,
    on_lang_change: EventHandler<Language>,
) -> Element {
    rsx! {
        div { class: "mx-auto max-w-2xl space-y-6",
            div {
                class: "rounded-lg border border-stone-200 bg-white p-6 dark:border-stone-800 dark:bg-stone-900",
                h2 { class: "mb-4 text-lg font-semibold", {t(*lang.read(), "settings.theme")} }
                ThemeToggle { theme: theme.clone(), on_change: move |v| on_theme_change.call(v) }
            }

            div {
                class: "rounded-lg border border-stone-200 bg-white p-6 dark:border-stone-800 dark:bg-stone-900",
                h2 { class: "mb-4 text-lg font-semibold", {t(*lang.read(), "settings.language")} }
                LanguageSwitcher { lang: lang, on_change: move |v| on_lang_change.call(v) }
            }
        }
    }
}
