//! Setup page — first-run admin initialization (UI-001).
//!
//! Shown when no admin user exists. Enforces strong password (≥12 chars)
//! and password confirmation.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};

/// Props for the setup page.
#[derive(Props, Clone, PartialEq)]
pub struct SetupProps {
    lang: Signal<Language>,
    on_done: EventHandler<()>,
}

/// Admin setup wizard.
pub fn SetupPage(props: SetupProps) -> Element {
    let l = *props.lang.read();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut error = use_signal(|| String::new());
    let mut loading = use_signal(|| false);

    let mut validate = move || {
        let pw = password.read();
        let cf = confirm.read();
        if pw.len() < 12 {
            return Err(t(l, "setup.password_weak").to_string());
        }
        if *pw != *cf {
            return Err(t(l, "setup.password_mismatch").to_string());
        }
        Ok(())
    };

    let mut do_setup = move || {
        let err = match validate() {
            Err(e) => e,
            Ok(()) => String::new(),
        };
        if !err.is_empty() {
            error.set(err);
            return;
        }
        error.set(String::new());
        loading.set(true);

        let username_val = username.read().clone();
        let password_val = password.read().clone();
        spawn(async move {
            match crate::api::auth::setup(&username_val, &password_val).await {
                Ok(_) => {
                    props.on_done.call(());
                }
                Err(e) => {
                    error.set(e.message);
                    loading.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "flex min-h-screen items-center justify-center bg-stone-50 dark:bg-stone-950",
            div { class: "w-full max-w-md rounded-lg border border-stone-200 bg-white p-8 shadow-sm dark:border-stone-800 dark:bg-stone-900",
                h1 { class: "text-xl font-bold text-stone-900 dark:text-stone-100", {t(l, "setup.title")} }
                p { class: "mt-2 text-sm text-stone-500 dark:text-stone-400", {t(l, "setup.description")} }

                div { class: "mt-6 space-y-4",
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "setup.username")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                            r#type: "text",
                            autocomplete: "username",
                            value: "{username}",
                            oninput: move |e| username.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_setup(); }
                            },
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "setup.password")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                            r#type: "password",
                            autocomplete: "new-password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_setup(); }
                            },
                        }
                        p { class: "mt-1 text-xs text-stone-400 dark:text-stone-500", {t(l, "setup.password_weak")} }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "setup.confirm")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                            r#type: "password",
                            autocomplete: "new-password",
                            value: "{confirm}",
                            oninput: move |e| confirm.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_setup(); }
                            },
                        }
                    }
                    if !error.read().is_empty() {
                        p { class: "text-sm text-red-600 dark:text-red-400", "{error}" }
                    }
                    button {
                        class: "w-full rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-amber-700 focus:outline-none focus:ring-2 focus:ring-amber-500 focus:ring-offset-2 disabled:opacity-50 dark:focus:ring-offset-stone-900",
                        r#type: "submit",
                        disabled: *loading.read(),
                        onclick: move |_| do_setup(),
                        if *loading.read() {
                            {t(l, "common.loading")}
                        } else {
                            {t(l, "setup.submit")}
                        }
                    }
                }
            }
        }
    }
}
