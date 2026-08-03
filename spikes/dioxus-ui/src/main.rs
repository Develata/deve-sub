mod chart;
mod components;
mod i18n;
mod mock;
mod pages;

use dioxus::prelude::*;

use i18n::{t, Language};
use pages::{dashboard::DashboardPage, groups::GroupsPage, nodes::NodesPage, settings::SettingsPage};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Dashboard,
    Nodes,
    Groups,
    Settings,
}

const NAV_ITEMS: [(Page, &str, &str); 4] = [
    (Page::Dashboard, "nav.dashboard", "📊"),
    (Page::Nodes, "nav.nodes", "📦"),
    (Page::Groups, "nav.groups", "🔗"),
    (Page::Settings, "nav.settings", "⚙️"),
];

fn read_theme() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item("theme").ok())
        .flatten()
        .unwrap_or_else(|| "light".to_string())
}

fn apply_theme(theme: &str) {
    if let Some(html) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let classes = html.class_list();
        let _ = classes.remove_1("dark");
        let _ = classes.remove_1("theme-amber");
        match theme {
            "dark" => {
                let _ = classes.add_1("dark");
            }
            "amber" => {
                let _ = classes.add_1("theme-amber");
            }
            _ => {}
        }
    }
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.set_item("theme", theme);
    }
}

fn app() -> Element {
    let mut lang = use_context_provider(|| Signal::new(Language::Zh));
    let mut theme = use_context_provider(|| Signal::new(read_theme()));
    let mut current_page = use_signal(|| Page::Dashboard);
    let mut mobile_menu = use_signal(|| false);

    use_future(move || async move {
        let t = theme.read().clone();
        apply_theme(&t);
    });

    let l = *lang.read();
    let sidebar_class = if *mobile_menu.read() {
        "fixed z-40 flex w-56 flex-col border-r border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900"
    } else {
        "hidden md:flex w-56 flex-col border-r border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900"
    };

    rsx! {
        div { class: "flex min-h-screen bg-stone-50 text-stone-900 dark:bg-stone-950 dark:text-stone-100",
            if *mobile_menu.read() {
                div {
                    class: "fixed inset-0 z-30 bg-black/30 md:hidden",
                    onclick: move |_| mobile_menu.set(false),
                }
            }

            aside { class: "{sidebar_class}",
                div { class: "flex h-16 items-center px-6",
                    span { class: "text-lg font-bold text-amber-600", "Deve Sub" }
                }
                nav { class: "flex-1 space-y-1 p-4",
                    for (page, label_key, icon) in NAV_ITEMS {
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
                                    onclick: move |_| {
                                        current_page.set(page);
                                        mobile_menu.set(false);
                                    },
                                    span { class: "text-base", "{icon}" }
                                    span { {t(l, label_key)} }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "flex flex-1 flex-col",
                header {
                    class: "flex h-16 items-center justify-between border-b border-stone-200 bg-white px-4 dark:border-stone-800 dark:bg-stone-900",
                    button {
                        class: "rounded-md p-2 text-stone-500 hover:bg-stone-100 md:hidden dark:text-stone-400 dark:hover:bg-stone-800",
                        onclick: move |_| {
                            let v = *mobile_menu.read();
                            mobile_menu.set(!v);
                        },
                        "☰"
                    }
                    h1 { class: "text-sm font-semibold text-stone-400", {t(l, "app.title")} }
                    components::LanguageSwitcher {
                        lang: lang,
                        on_change: move |v| lang.set(v),
                    }
                }

                main { class: "flex-1 overflow-y-auto p-6",
                    match *current_page.read() {
                        Page::Dashboard => rsx! { DashboardPage { lang: lang } },
                        Page::Nodes => rsx! { NodesPage { lang: lang } },
                        Page::Groups => rsx! { GroupsPage { lang: lang } },
                        Page::Settings => rsx! {
                            SettingsPage {
                                theme: theme.read().clone(),
                                on_theme_change: move |v: String| {
                                    apply_theme(&v);
                                    theme.set(v);
                                },
                                lang: lang,
                                on_lang_change: move |v: Language| {
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

fn main() {
    dioxus::launch(app);
}
