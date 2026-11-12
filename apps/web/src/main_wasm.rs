// Deve Sub — Dioxus Web frontend WASM entry point.
//
// CSR mode (ADR-0001): renders UI, collects intent, dispatches typed REST
// requests to `/api/v1/*`. No business logic in the frontend.
//
// This file is text-included into `main.rs` only under
// `target_family = "wasm"`. On native targets the bin target exits with a
// clear error instead — the frontend must be built separately via
// `dx build --release --package deve-sub-web` for `wasm32-unknown-unknown`
// and served from `apps/web/dist/`.

mod api;
mod branding;
mod i18n;
mod pages;
mod theme;

use dioxus::prelude::*;
use i18n::{Language, t};
use theme::Theme;

use pages::{
    audit::AuditPage, dashboard::DashboardPage, login::LoginPage, nodes::NodesPage,
    settings::SettingsPage, setup::SetupPage, sources::SourcesPage,
    subscriptions::SubscriptionsPage, templates::TemplatesPage, users::UsersPage,
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
    Users,
    Audit,
    Settings,
}

const NAV_ITEMS: [(Page, &str); 8] = [
    (Page::Dashboard, "nav.dashboard"),
    (Page::Nodes, "nav.nodes"),
    (Page::Sources, "nav.sources"),
    (Page::Templates, "nav.templates"),
    (Page::Subscriptions, "nav.subscriptions"),
    (Page::Users, "nav.users"),
    (Page::Audit, "nav.audit"),
    (Page::Settings, "nav.settings"),
];

fn app() -> Element {
    let branding = branding::use_branding();
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
                // Distinguish "no admin yet" from "need login" via the
                // side-effect-free GET /auth/status endpoint (DS-AUD-002).
                match api::auth::status().await {
                    Ok(s) if s.initialized => auth_state.set(AuthState::Unauthenticated),
                    Ok(_) => auth_state.set(AuthState::NeedsSetup),
                    Err(_) => auth_state.set(AuthState::Unauthenticated),
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

    let l = *lang.read();

    let view = match *auth_state.read() {
        AuthState::Checking => rsx! {
            div { class: "flex min-h-screen items-center justify-center bg-canvas",
                div { class: "flex flex-col items-center gap-3",
                    div { class: "h-8 w-8 animate-spin rounded-full border-2 border-line border-t-accent" }
                    p { class: "text-sm text-muted", {t(l, "common.loading")} }
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
                "fixed z-40 flex w-56 flex-col border-r border-line-soft bg-surface  "
            } else {
                "hidden md:flex w-56 flex-col border-r border-line-soft bg-surface  "
            };

            rsx! {
                div { class: "flex min-h-screen bg-canvas text-foreground",
                    // Mobile overlay.
                    if *mobile_menu.read() {
                        div {
                            class: "fixed inset-0 z-30 bg-black/30 md:hidden",
                            onclick: move |_| mobile_menu.set(false),
                        }
                    }

                    // Sidebar.
                    aside { class: "{sidebar_class}", style: "flex-shrink: 0;",
                        div { class: "flex h-16 items-center px-6",
                            span { class: "truncate text-lg font-bold text-accent-text", "{branding.read().display_name()}" }
                        }
                        nav { class: "flex-1 space-y-1 p-4",
                            for (page, label_key) in NAV_ITEMS {
                                {
                                    let is_active = *current_page.read() == page;
                                    let class = if is_active {
                                        "flex w-full items-center gap-3 rounded-md bg-accent-soft px-3 py-2 text-sm font-medium text-accent-text  "
                                    } else {
                                        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted hover:bg-surface-muted  "
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
                                            span { {t(l, label_key)} }
                                        }
                                    }
                                }
                            }
                        }
                        // Logout button.
                        div { class: "border-t border-line-soft p-4",
                            button {
                                class: "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted hover:bg-surface-muted",
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
                    div { class: "flex min-w-0 flex-1 flex-col",
                        header {
                            class: "flex h-16 items-center justify-between border-b border-line-soft bg-surface px-4",
                            button {
                                class: "rounded-md p-2 text-muted hover:bg-surface-muted md:hidden",
                                aria_label: "Menu",
                                onclick: move |_| {
                                    let v = *mobile_menu.read();
                                    mobile_menu.set(!v);
                                },
                                "☰"
                            }
                            h1 { class: "min-w-0 truncate text-sm font-semibold text-muted", "{branding.read().display_name()}" }
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
                                Page::Users => rsx! { UsersPage { lang: lang } },
                                Page::Audit => rsx! { AuditPage { lang: lang } },
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
    };
    rsx! {
        document::Title { "{branding.read().display_name()}" }
        // WHY: asset! registers the stylesheet with dx so production dist
        // includes the file; legacy resource links alone did not copy it.
        document::Stylesheet { href: asset!("/src/styles/tailwind.css") }
        {view}
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
                        "rounded-md px-2 py-1 text-xs font-medium text-accent-text bg-accent-soft  "
                    } else {
                        "rounded-md px-2 py-1 text-xs font-medium text-muted hover:bg-surface-muted  "
                    },
                    onclick: move |_| props.on_change.call(Language::Zh),
                    "中文"
                }
                button {
                    class: if l == Language::En {
                        "rounded-md px-2 py-1 text-xs font-medium text-accent-text bg-accent-soft  "
                    } else {
                        "rounded-md px-2 py-1 text-xs font-medium text-muted hover:bg-surface-muted  "
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
