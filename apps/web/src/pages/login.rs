//! Login page — session authentication.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};

#[derive(Props, Clone, PartialEq)]
pub struct LoginProps {
    lang: Signal<Language>,
    on_success: EventHandler<()>,
}

pub fn LoginPage(props: LoginProps) -> Element {
    let l = *props.lang.read();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| String::new());
    let mut loading = use_signal(|| false);

    let mut do_login = move || {
        error.set(String::new());
        loading.set(true);
        let u = username.read().clone();
        let p = password.read().clone();
        spawn(async move {
            match crate::api::auth::login(&u, &p).await {
                Ok(_) => {
                    props.on_success.call(());
                }
                Err(e) => {
                    let msg = if e.status == 401 {
                        t(l, "auth.login_failed").to_string()
                    } else if e.status == 429 {
                        t(l, "auth.rate_limited").to_string()
                    } else {
                        e.message
                    };
                    error.set(msg);
                    loading.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "flex min-h-screen items-center justify-center bg-stone-50 dark:bg-stone-950",
            div { class: "w-full max-w-md rounded-lg border border-stone-200 bg-white p-8 shadow-sm dark:border-stone-800 dark:bg-stone-900",
                h1 { class: "text-xl font-bold text-stone-900 dark:text-stone-100", {t(l, "auth.login_title")} }
                div { class: "mt-6 space-y-4",
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "auth.username")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                            r#type: "text",
                            autocomplete: "username",
                            value: "{username}",
                            oninput: move |e| username.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_login(); }
                            },
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "auth.password")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                            r#type: "password",
                            autocomplete: "current-password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_login(); }
                            },
                        }
                    }
                    if !error.read().is_empty() {
                        p { class: "text-sm text-red-600 dark:text-red-400", "{error}" }
                    }
                    button {
                        class: "w-full rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-amber-700 focus:outline-none focus:ring-2 focus:ring-amber-500 focus:ring-offset-2 disabled:opacity-50 dark:focus:ring-offset-stone-900",
                        disabled: *loading.read(),
                        onclick: move |_| do_login(),
                        if *loading.read() {
                            {t(l, "common.loading")}
                        } else {
                            {t(l, "auth.login")}
                        }
                    }
                }
            }
        }
    }
}
