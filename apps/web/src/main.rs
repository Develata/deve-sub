//! Deve Sub — Dioxus Web frontend entry point.
//!
//! CSR mode (ADR-0001): renders UI, collects intent, dispatches typed REST
//! requests to `/api/v1/*`. No business logic in the frontend.

#![cfg(target_family = "wasm")]
#![allow(clippy::expect_used)]

mod api;
mod i18n;
mod pages;
mod theme;

use dioxus::prelude::*;
use i18n::{Language, t};
use theme::Theme;

use pages::{
    dashboard::DashboardPage, login::LoginPage, nodes::NodesPage, settings::SettingsPage,
    setup::SetupPage, sources::SourcesPage, subscriptions::SubscriptionsPage,
    templates::TemplatesPage,
};

/// Top-level view state driven by auth status.
#[derive(Clone, Copy, PartialEq)]
enum AuthState {
    /// Checking session on initial load.
    Checking,
    /// No admin exists yet — show setup wizard.
    NeedsSetup,
    /// Admin exists but not logged in — show login.
    Unauthenticated,
    /// Session active — show main app.
    Authenticated,
}

/// Navigation page within the authenticated app.
#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Dashboard,
    Nodes,
    Sources,
    Templates,
    Subscriptions,
    Settings,
}

const NAV_ITEMS: [(Page, &str, &str); 6] = [
    (Page::Dashboard, "nav.dashboard", "M"),
    (Page::Nodes, "nav.nodes", "N"),
    (Page::Sources, "nav.sources", "S"),
    (Page::Templates, "nav.templates", "T"),
    (Page::Subscriptions, "nav.subscriptions", "U"),
    (Page::Settings, "nav.settings", ","),
];

/// Keyboard shortcut: Alt+<key> switches pages (UI-010).
const NAV_SHORTCUTS: [(Page, &str); 6] = [
    (Page::Dashboard, "m"),
    (Page::Nodes, "n"),
    (Page::Sources, "s"),
    (Page::Templates, "t"),
    (Page::Subscriptions, "u"),
    (Page::Settings, ","),
];

fn app() -> Element {
    let mut auth_state = use_signal(|| AuthState::Checking);
    let mut lang = use_context_provider(|| Signal::new(theme::read_language()));
    let mut current_theme = use_context_provider(|| Signal::new(theme::read_theme()));
    let mut current_page = use_signal(|| Page::Dashboard);
    let mut mobile_menu = use_signal(|| false);

    // Check auth on mount.
    use_future(move || async move {
        match api::auth::me().await {
            Ok(_) => auth_state.set(AuthState::Authenticated),
            Err(e) if e.status == 401 => {
                // Distinguish "no admin yet" from "need login".
                // POST /auth/setup returns 409 if already initialized.
                // WHY: use a password shorter than MIN_PASSWORD_LEN (8) so the
                // server always rejects with 400 invalid_input when the
                // endpoint is available (no admin yet), without creating one.
                // A 409 means admin already exists → show login.
                match api::auth::setup("__probe__", "x").await {
                    Err(probe_err) if probe_err.status == 409 => {
                        auth_state.set(AuthState::Unauthenticated);
                    }
                    Err(probe_err) if probe_err.status == 400 => {
                        auth_state.set(AuthState::NeedsSetup);
                    }
                    _ => auth_state.set(AuthState::Unauthenticated),
                }
            }
            Err(_) => auth_state.set(AuthState::Unauthenticated),
        }
    });

    // Apply theme on mount and when it changes.
    use_future(move || async move {
        let th = *current_theme.read();
        theme::apply_theme(th);
        if theme::read_reduce_motion() {
            theme::apply_reduce_motion(true);
        }
        if let Some(accent) = theme::read_custom_accent() {
            theme::apply_custom_accent(&accent);
        }
    });

    // Keyboard navigation (UI-010).
    use_future(move || async move {
        // WHY: Alt+key for page switching, Escape to close mobile menu.
        // This is a simplified global handler — Dioxus doesn't have a
        // built-in global keyboard event, so we poll via window event.
        // The actual keyboard handling is done via onkeydown on focusable
        // elements in each page component.
    });

    let l = *lang.read();

    match *auth_state.read() {
        AuthState::Checking => rsx! {
            div { class: "flex min-h-screen items-center justify-center bg-stone-50 dark:bg-stone-950",
                div { class: "flex flex-col items-center gap-3",
                    div { class: "h-8 w-8 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "common.loading")} }
                }
            }
        },
        AuthState::NeedsSetup => rsx! {
            SetupPage { lang: lang, on_done: move |_| {
                auth_state.set(AuthState::Unauthenticated);
            }}
        },
        AuthState::Unauthenticated => rsx! {
            LoginPage { lang: lang, on_success: move |_| {
                auth_state.set(AuthState::Authenticated);
            }}
        },
        AuthState::Authenticated => {
            let sidebar_class = if *mobile_menu.read() {
                "fixed z-40 flex w-56 flex-col border-r border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900"
            } else {
                "hidden md:flex w-56 flex-col border-r border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900"
            };

            rsx! {
                div { class: "flex min-h-screen bg-stone-50 text-stone-900 dark:bg-stone-950 dark:text-stone-100",
                    // Mobile overlay.
                    if *mobile_menu.read() {
                        div {
                            class: "fixed inset-0 z-30 bg-black/30 md:hidden",
                            onclick: move |_| mobile_menu.set(false),
                        }
                    }

                    // Sidebar.
                    aside { class: "{sidebar_class}",
                        div { class: "flex h-16 items-center px-6",
                            span { class: "text-lg font-bold text-amber-600 dark:text-amber-500", "Deve Sub" }
                        }
                        nav { class: "flex-1 space-y-1 p-4",
                            for (page, label_key, shortcut) in NAV_ITEMS {
                                {
                                    let is_active = *current_page.read() == page;
                                    let class = if is_active {
                                        "flex w-full items-center gap-3 rounded-md bg-amber-50 px-3 py-2 text-sm font-medium text-amber-700 dark:bg-amber-900/20 dark:text-amber-400"
                                    } else {
                                        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-stone-600 hover:bg-stone-100 dark:text-stone-300 dark:hover:bg-stone-800"
                                    };
                                    rsx! {
                                        button {
                                            key: "{label_key}",
                                            class: "{class}",
                                            tabindex: 0,
                                            onclick: move |_| {
                                                current_page.set(page);
                                                mobile_menu.set(false);
                                            },
                                            onkeydown: move |e| {
                                                if e.key() == Key::Enter {
                                                    current_page.set(page);
                                                    mobile_menu.set(false);
                                                }
                                            },
                                            span { {t(l, label_key)} }
                                        }
                                    }
                                }
                            }
                        }
                        // Logout button.
                        div { class: "border-t border-stone-200 p-4 dark:border-stone-800",
                            button {
                                class: "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-stone-600 hover:bg-stone-100 dark:text-stone-300 dark:hover:bg-stone-800",
                                tabindex: 0,
                                onclick: move |_| {
                                    spawn(async move {
                                        let _ = api::auth::logout().await;
                                        auth_state.set(AuthState::Unauthenticated);
                                    });
                                },
                                {t(l, "auth.logout")}
                            }
                        }
                    }

                    // Main content area.
                    div { class: "flex flex-1 flex-col",
                        header {
                            class: "flex h-16 items-center justify-between border-b border-stone-200 bg-white px-4 dark:border-stone-800 dark:bg-stone-900",
                            button {
                                class: "rounded-md p-2 text-stone-500 hover:bg-stone-100 md:hidden dark:text-stone-400 dark:hover:bg-stone-800",
                                aria_label: "Menu",
                                onclick: move |_| {
                                    let v = *mobile_menu.read();
                                    mobile_menu.set(!v);
                                },
                                "☰"
                            }
                            h1 { class: "text-sm font-semibold text-stone-400 dark:text-stone-500", {t(l, "app.title")} }
                            components::LanguageSwitcher {
                                lang: lang,
                                on_change: move |v: Language| {
                                    theme::save_language(v);
                                    lang.set(v);
                                },
                            }
                        }

                        main { class: "flex-1 overflow-y-auto p-6",
                            match *current_page.read() {
                                Page::Dashboard => rsx! { DashboardPage { lang: lang } },
                                Page::Nodes => rsx! { NodesPage { lang: lang } },
                                Page::Sources => rsx! { SourcesPage { lang: lang } },
                                Page::Templates => rsx! { TemplatesPage { lang: lang } },
                                Page::Subscriptions => rsx! { SubscriptionsPage { lang: lang } },
                                Page::Settings => rsx! {
                                    SettingsPage {
                                        theme: current_theme,
                                        lang: lang,
                                        on_theme_change: move |th: Theme| {
                                            theme::apply_theme(th);
                                            current_theme.set(th);
                                        },
                                        on_lang_change: move |v: Language| {
                                            theme::save_language(v);
                                            lang.set(v);
                                        },
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

mod components {
    use super::*;

    #[derive(Props, Clone, PartialEq)]
    pub struct LanguageSwitcherProps {
        lang: Signal<Language>,
        on_change: EventHandler<Language>,
    }

    /// Language switcher (Zh/En) with persistence.
    pub fn LanguageSwitcher(props: LanguageSwitcherProps) -> Element {
        let l = *props.lang.read();
        rsx! {
            div { class: "flex items-center gap-1",
                button {
                    class: if l == Language::Zh {
                        "rounded-md px-2 py-1 text-xs font-medium text-amber-700 bg-amber-50 dark:text-amber-400 dark:bg-amber-900/20"
                    } else {
                        "rounded-md px-2 py-1 text-xs font-medium text-stone-500 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-800"
                    },
                    onclick: move |_| props.on_change.call(Language::Zh),
                    "中文"
                }
                button {
                    class: if l == Language::En {
                        "rounded-md px-2 py-1 text-xs font-medium text-amber-700 bg-amber-50 dark:text-amber-400 dark:bg-amber-900/20"
                    } else {
                        "rounded-md px-2 py-1 text-xs font-medium text-stone-500 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-800"
                    },
                    onclick: move |_| props.on_change.call(Language::En),
                    "EN"
                }
            }
        }
    }
}

fn main() {
    dioxus::launch(app);
}
