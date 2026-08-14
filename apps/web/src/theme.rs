//! Theme management: light, dark, warm, violet, custom accent, reduce-motion.
//!
//! UI-003 (dark mode), UI-004 (Minimal Warm), UI-005 (Fantasy Violet),
//! UI-006 (custom theme persistence), UI-007 (reduce motion).

#![cfg(target_family = "wasm")]

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, js_sys, window};

/// Theme kind selectable from settings.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    Warm,
    Violet,
}

impl Theme {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Warm => "warm",
            Self::Violet => "violet",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Self {
        match s {
            "dark" => Self::Dark,
            "warm" => Self::Warm,
            "violet" => Self::Violet,
            _ => Self::Light,
        }
    }
}

/// Read the persisted theme from localStorage; falls back to light or
/// system preference on first visit.
#[must_use]
pub fn read_theme() -> Theme {
    let storage = match window().and_then(|w| w.local_storage().ok()).flatten() {
        Some(s) => s,
        None => return Theme::Light,
    };
    match storage.get_item("theme").ok().flatten() {
        Some(s) => Theme::from_str(&s),
        None => {
            // First visit: respect prefers-color-scheme.
            if window()
                .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok())
                .flatten()
                .is_some_and(|m| m.matches())
            {
                Theme::Dark
            } else {
                Theme::Light
            }
        }
    }
}

/// Apply the theme class to `<html>` and persist to localStorage.
pub fn apply_theme(theme: Theme) {
    let html = match window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        Some(el) => el,
        None => return,
    };
    let classes = html.class_list();
    let _ = classes.remove_1("dark");
    let _ = classes.remove_1("theme-warm");
    let _ = classes.remove_1("theme-violet");
    match theme {
        Theme::Dark => {
            let _ = classes.add_1("dark");
        }
        Theme::Warm => {
            let _ = classes.add_1("theme-warm");
        }
        Theme::Violet => {
            let _ = classes.add_1("theme-violet");
        }
        Theme::Light => {}
    }
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("theme", theme.as_str());
    }
}

/// Read the reduce-motion preference from localStorage.
#[must_use]
pub fn read_reduce_motion() -> bool {
    window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item("reduce-motion").ok())
        .flatten()
        .is_some_and(|v| v == "true")
}

/// Toggle the `reduce-motion` class on `<html>` and persist the preference.
pub fn apply_reduce_motion(enabled: bool) {
    if let Some(html) = window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let classes = html.class_list();
        if enabled {
            let _ = classes.add_1("reduce-motion");
        } else {
            let _ = classes.remove_1("reduce-motion");
        }
    }
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("reduce-motion", if enabled { "true" } else { "false" });
    }
}

/// Read a custom accent color from localStorage; returns `None` if unset.
#[must_use]
pub fn read_custom_accent() -> Option<String> {
    window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item("custom-accent").ok())
        .flatten()
        .filter(|s| !s.is_empty())
}

/// Persist a custom accent color and inject it as a CSS variable on `:root`.
///
/// WHY: The accent color drives button, link, and focus-ring colors via
/// `--color-accent` / `--color-accent-hover`. Overriding these on `:root`
/// cascades to all Tailwind `accent-*` utilities without per-component logic.
pub fn apply_custom_accent(color: &str) {
    if let Some(el) = window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        if let Some(html) = el.dyn_into::<HtmlElement>().ok() {
            let style = html.style();
            let _ = style.set_property("--color-accent", color);
            let hover = format!("color-mix(in srgb, {color} 80%, white)");
            let _ = style.set_property("--color-accent-hover", &hover);
            let soft = format!("color-mix(in srgb, {color} 15%, white)");
            let _ = style.set_property("--color-accent-soft", &soft);
        }
    }
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("custom-accent", color);
    }
}

/// Clear the custom accent, reverting to the theme default.
pub fn clear_custom_accent() {
    if let Some(el) = window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        if let Some(html) = el.dyn_into::<HtmlElement>().ok() {
            let style = html.style();
            let _ = style.remove_property("--color-accent");
            let _ = style.remove_property("--color-accent-hover");
            let _ = style.remove_property("--color-accent-soft");
        }
    }
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.remove_item("custom-accent");
    }
}

/// Read a custom product name from localStorage; returns `None` if unset.
#[must_use]
pub fn read_custom_product_name() -> Option<String> {
    window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item("custom-product-name").ok())
        .flatten()
        .filter(|s| !s.is_empty())
}

/// Persist a custom product name (UI-006: logo/name persistence).
pub fn save_custom_product_name(name: &str) {
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("custom-product-name", name);
    }
}

/// Read the persisted language preference.
#[must_use]
pub fn read_language() -> crate::i18n::Language {
    let storage = match window().and_then(|w| w.local_storage().ok()).flatten() {
        Some(s) => s,
        None => return crate::i18n::Language::Zh,
    };
    match storage.get_item("lang").ok().flatten() {
        Some(s) => crate::i18n::Language::from_str(&s),
        None => crate::i18n::Language::Zh,
    }
}

/// Persist the language preference.
pub fn save_language(lang: crate::i18n::Language) {
    if let Some(storage) = window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("lang", lang.as_str());
    }
}

// Suppress unused-import warning when js_sys is not directly referenced.
#[allow(unused_imports)]
use js_sys as _;
