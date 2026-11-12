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
    let branding = use_context::<Signal<crate::branding::Branding>>();
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
        div { class: "flex min-h-screen items-center justify-center bg-canvas",
            div { class: "w-full max-w-md rounded-lg border border-line-soft bg-surface p-8 shadow-sm",
                h1 { class: "text-xl font-bold text-foreground", {t(l, "setup.title")} }
                p { class: "mt-2 text-sm text-muted", {format!("{} {}", t(l, "setup.description"), branding.read().display_name())} }

                div { class: "mt-6 space-y-4",
                    div {
                        label { class: "block text-sm font-medium text-foreground", {t(l, "setup.username")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
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
                        label { class: "block text-sm font-medium text-foreground", {t(l, "setup.password")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
                            r#type: "password",
                            autocomplete: "new-password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter { do_setup(); }
                            },
                        }
                        p { class: "mt-1 text-xs text-muted", {t(l, "setup.password_weak")} }
                    }
                    div {
                        label { class: "block text-sm font-medium text-foreground", {t(l, "setup.confirm")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
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
                        class: "w-full rounded-md bg-accent px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 disabled:opacity-50",
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
