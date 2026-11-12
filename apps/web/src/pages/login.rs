//! Login page — session authentication with optional 2FA challenge.
//!
//! DS-AUD-003: when the backend returns `requires_2fa: true`, the login
//! flow transitions to a `TwoFactorChallenge` stage instead of calling
//! `on_success` immediately. The challenge token is held locally and
//! exchanged for a session via `POST /auth/login/2fa` when the user
//! submits a TOTP/recovery code.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};

#[derive(Props, Clone, PartialEq)]
pub struct LoginProps {
    lang: Signal<Language>,
    on_success: EventHandler<()>,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Credentials,
    TwoFactorChallenge,
}

pub fn LoginPage(props: LoginProps) -> Element {
    let branding = use_context::<Signal<crate::branding::Branding>>();
    let l = *props.lang.read();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut error = use_signal(|| String::new());
    let mut loading = use_signal(|| false);
    let mut stage = use_signal(|| Stage::Credentials);
    let mut challenge_token = use_signal(String::new);

    let mut do_login = move || {
        if *loading.read() { return; }
        error.set(String::new());
        loading.set(true);
        let u = username.read().clone();
        let p = password.read().clone();
        spawn(async move {
            match crate::api::auth::login(&u, &p).await {
                Ok(resp) if resp.requires_2fa => {
                    if let Some(token) = resp.challenge_token {
                        password.set(String::new());
                        challenge_token.set(token);
                        stage.set(Stage::TwoFactorChallenge);
                        code.set(String::new());
                        error.set(String::new());
                    } else {
                        error.set(t(l, "auth.2fa_invalid").to_string());
                    }
                    loading.set(false);
                }
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

    let mut do_2fa = move || {
        if *loading.read() { return; }
        error.set(String::new());
        loading.set(true);
        let token = challenge_token.read().clone();
        let c = code.read().clone();
        spawn(async move {
            match crate::api::auth::login_2fa(&token, &c).await {
                Ok(_) => {
                    props.on_success.call(());
                }
                Err(e) => {
                    let msg = if e.status == 401 {
                        t(l, "auth.2fa_invalid").to_string()
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

    let mut back_to_credentials = move || {
        if *loading.read() { return; }
        stage.set(Stage::Credentials);
        challenge_token.set(String::new());
        code.set(String::new());
        error.set(String::new());
        loading.set(false);
    };

    rsx! {
        div { class: "flex min-h-screen items-center justify-center bg-canvas",
            div { class: "w-full max-w-md rounded-lg border border-line-soft bg-surface p-8 shadow-sm",
                match *stage.read() {
                    Stage::Credentials => rsx! {
                h1 { class: "text-xl font-bold text-foreground", {format!("{} {}", t(l, "auth.login_title"), branding.read().display_name())} }
                        div { class: "mt-6 space-y-4",
                            div {
                                label { class: "block text-sm font-medium text-foreground", {t(l, "auth.username")} }
                                input {
                                    class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
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
                                label { class: "block text-sm font-medium text-foreground", {t(l, "auth.password")} }
                                input {
                                    class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
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
                                class: "w-full rounded-md bg-accent px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 disabled:opacity-50",
                                disabled: *loading.read(),
                                onclick: move |_| do_login(),
                                if *loading.read() {
                                    {t(l, "common.loading")}
                                } else {
                                    {t(l, "auth.login")}
                                }
                            }
                        }
                    },
                    Stage::TwoFactorChallenge => rsx! {
                        h1 { class: "text-xl font-bold text-foreground", {t(l, "auth.2fa_title")} }
                        p { class: "mt-2 text-sm text-muted", {t(l, "auth.2fa_prompt")} }
                        div { class: "mt-6 space-y-4",
                            div {
                                label { r#for: "login-second-factor", class: "block text-sm font-medium text-foreground", {t(l, "auth.2fa_login_code")} }
                                input {
                                    id: "login-second-factor",
                                    class: "mt-1 block w-full rounded-md border border-line px-3 py-2 text-sm shadow-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent",
                                    r#type: "text",
                                    autocomplete: "one-time-code",
                                    // Recovery codes contain letters and a separator;
                                    // six-digit-only controls make that valid path unusable.
                                    inputmode: "text",
                                    maxlength: "64",
                                    value: "{code}",
                                    oninput: move |e| code.set(e.value()),
                                    onkeydown: move |e| {
                                        if e.key() == Key::Enter { do_2fa(); }
                                    },
                                }
                            }
                            if !error.read().is_empty() {
                                p { class: "text-sm text-red-600 dark:text-red-400", "{error}" }
                            }
                            button {
                                class: "w-full rounded-md bg-accent px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 disabled:opacity-50",
                                disabled: *loading.read(),
                                onclick: move |_| do_2fa(),
                                if *loading.read() {
                                    {t(l, "common.loading")}
                                } else {
                                    {t(l, "auth.2fa_submit")}
                                }
                            }
                            button {
                                class: "w-full rounded-md px-4 py-2 text-sm font-medium text-muted hover:bg-surface-muted",
                                onclick: move |_| back_to_credentials(),
                                {t(l, "auth.2fa_back")}
                            }
                        }
                    },
                }
            }
        }
    }
}
