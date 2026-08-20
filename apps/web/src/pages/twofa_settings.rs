//! 2FA management component for the settings page.
//!
//! Covers setup (generate TOTP secret), verify (enable 2FA + show recovery
//! codes), disable (re-auth with password), and regenerate recovery codes.
//! AUTH-005, AUTH-006.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, t};
use crate::pages::util::copy_to_clipboard;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwoFactorSetupResponse {
    secret: String,
    otpauth_uri: String,
}

#[derive(Debug, Clone, Serialize)]
struct TwoFactorVerifyRequest {
    code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwoFactorVerifyResponse {
    recovery_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TwoFactorDisableRequest {
    password: String,
}

#[derive(Debug, Clone, Serialize)]
struct RegenerateRecoveryCodesRequest {
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegenerateRecoveryCodesResponse {
    recovery_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurrentUserResponse {
    user: CurrentUserDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurrentUserDto {
    two_factor_enabled: bool,
}

#[derive(Clone, PartialEq)]
enum Phase {
    Idle,
    AwaitingVerify { secret: String, otpauth_uri: String },
    RecoveryCodes(Vec<String>),
    DisablePrompt,
    RegeneratePrompt,
}

#[derive(Props, Clone, PartialEq)]
pub struct TwoFactorSettingsProps {
    lang: Signal<Language>,
}

pub fn TwoFactorSettings(props: TwoFactorSettingsProps) -> Element {
    let l = *props.lang.read();
    let mut enabled = use_signal(|| false);
    let mut phase = use_signal(|| Phase::Idle);
    let mut error = use_signal(|| String::new());
    let mut loading = use_signal(|| false);
    let mut verify_code = use_signal(String::new);
    let mut password = use_signal(String::new);

    let refresh_status = move || {
        spawn(async move {
            if let Ok(resp) = crate::api::get::<CurrentUserResponse>("/auth/me").await {
                enabled.set(resp.user.two_factor_enabled);
            }
        });
    };

    use_future(move || async move {
        refresh_status();
    });

    let start_setup = move |_| {
        error.set(String::new());
        loading.set(true);
        spawn(async move {
            match crate::api::send::<TwoFactorSetupResponse, serde_json::Value>(
                "POST", "/auth/2fa/setup", None,
            )
            .await
            {
                Ok(resp) => {
                    verify_code.set(String::new());
                    phase.set(Phase::AwaitingVerify {
                        secret: resp.secret,
                        otpauth_uri: resp.otpauth_uri,
                    });
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let mut do_verify = move || {
        let code = verify_code.read().clone();
        if code.is_empty() {
            return;
        }
        error.set(String::new());
        loading.set(true);
        let req = TwoFactorVerifyRequest { code };
        spawn(async move {
            match crate::api::send::<TwoFactorVerifyResponse, TwoFactorVerifyRequest>(
                "POST", "/auth/2fa/verify", Some(&req),
            )
            .await
            {
                Ok(resp) => {
                    enabled.set(true);
                    phase.set(Phase::RecoveryCodes(resp.recovery_codes));
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let cancel_setup = move |_| {
        phase.set(Phase::Idle);
        verify_code.set(String::new());
        error.set(String::new());
    };

    let mut do_disable = move || {
        let pwd = password.read().clone();
        if pwd.is_empty() {
            return;
        }
        error.set(String::new());
        loading.set(true);
        let req = TwoFactorDisableRequest { password: pwd };
        spawn(async move {
            match crate::api::send::<serde_json::Value, TwoFactorDisableRequest>(
                "POST", "/auth/2fa/disable", Some(&req),
            )
            .await
            {
                Ok(_) => {
                    enabled.set(false);
                    phase.set(Phase::Idle);
                    password.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let mut do_regenerate = move || {
        let pwd = password.read().clone();
        if pwd.is_empty() {
            return;
        }
        error.set(String::new());
        loading.set(true);
        let req = RegenerateRecoveryCodesRequest { password: pwd };
        spawn(async move {
            match crate::api::send::<RegenerateRecoveryCodesResponse, RegenerateRecoveryCodesRequest>(
                "POST", "/auth/2fa/recovery-codes", Some(&req),
            )
            .await
            {
                Ok(resp) => {
                    phase.set(Phase::RecoveryCodes(resp.recovery_codes));
                    password.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let (secret, otpauth_uri) = match &*phase.read() {
        Phase::AwaitingVerify { secret, otpauth_uri } => (secret.clone(), otpauth_uri.clone()),
        _ => (String::new(), String::new()),
    };

    let recovery = match &*phase.read() {
        Phase::RecoveryCodes(codes) => codes.clone(),
        _ => Vec::new(),
    };

    let is_awaiting_verify = matches!(*phase.read(), Phase::AwaitingVerify { .. });
    let is_recovery = matches!(*phase.read(), Phase::RecoveryCodes(_));
    let is_disable_prompt = matches!(*phase.read(), Phase::DisablePrompt);
    let is_regen_prompt = matches!(*phase.read(), Phase::RegeneratePrompt);

    rsx! {
        section {
            h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "settings.2fa")} }

            if *enabled.read() && !is_recovery && !is_disable_prompt && !is_regen_prompt {
                div { class: "mt-4 space-y-3",
                    p { class: "text-sm text-green-600 dark:text-green-400", {t(l, "2fa.enabled_status")} }
                    div { class: "flex gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: move |_| { phase.set(Phase::RegeneratePrompt); password.set(String::new()); error.set(String::new()); },
                            {t(l, "2fa.regenerate")}
                        }
                        button {
                            class: "rounded-md border border-red-300 px-4 py-2 text-sm text-red-600 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                            onclick: move |_| { phase.set(Phase::DisablePrompt); password.set(String::new()); error.set(String::new()); },
                            {t(l, "2fa.disable")}
                        }
                    }
                }
            }

            if !*enabled.read() && !is_awaiting_verify && !is_recovery {
                div { class: "mt-4 space-y-3",
                    p { class: "text-sm text-stone-600 dark:text-stone-400", {t(l, "2fa.not_enabled")} }
                    button {
                        class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                        disabled: *loading.read(),
                        onclick: start_setup,
                        if *loading.read() { {t(l, "common.loading")} } else { {t(l, "2fa.setup")} }
                    }
                }
            }

            if is_awaiting_verify {
                div { class: "mt-4 space-y-4",
                    p { class: "text-sm text-stone-600 dark:text-stone-400", {t(l, "2fa.setup_instructions")} }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "2fa.secret")} }
                        div { class: "mt-1 flex items-center gap-2",
                            code { class: "block w-full rounded-md border border-stone-300 bg-stone-50 px-3 py-2 font-mono text-sm dark:border-stone-700 dark:bg-stone-800", "{secret}" }
                            button {
                                class: "rounded-md border border-stone-300 px-3 py-2 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                onclick: move |_| {
                                    let s = secret.clone();
                                    spawn(async move { let _ = copy_to_clipboard(&s).await; });
                                },
                                {t(l, "common.copy")}
                            }
                        }
                    }
                    details { class: "rounded-md border border-stone-200 dark:border-stone-800",
                        summary { class: "cursor-pointer px-3 py-2 text-sm text-stone-600 dark:text-stone-400", {t(l, "2fa.otpauth_uri")} }
                        code { class: "block px-3 pb-2 break-all font-mono text-xs text-stone-500 dark:text-stone-400", "{otpauth_uri}" }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "auth.2fa_code")} }
                        input {
                            class: "mt-1 block w-full max-w-xs rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            inputmode: "numeric",
                            maxlength: "6",
                            value: "{verify_code}",
                            oninput: move |e| verify_code.set(e.value()),
                            onkeydown: move |e| { if e.key() == Key::Enter { do_verify(); } },
                        }
                    }
                    div { class: "flex gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: cancel_setup,
                            {t(l, "common.cancel")}
                        }
                        button {
                            class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                            disabled: *loading.read(),
                            onclick: move |_| do_verify(),
                            if *loading.read() { {t(l, "common.loading")} } else { {t(l, "auth.2fa_submit")} }
                        }
                    }
                }
            }

            if is_recovery {
                div { class: "mt-4 space-y-3",
                    p { class: "text-sm font-medium text-red-600 dark:text-red-400", {t(l, "2fa.recovery_warning")} }
                    div { class: "rounded-md border border-stone-200 bg-stone-50 p-4 dark:border-stone-800 dark:bg-stone-900",
                        ul { class: "grid grid-cols-2 gap-1 font-mono text-sm",
                            for code in recovery.iter() {
                                li { class: "text-stone-700 dark:text-stone-300", "{code}" }
                            }
                        }
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| { phase.set(Phase::Idle); },
                        {t(l, "common.close")}
                    }
                }
            }

            if is_disable_prompt || is_regen_prompt {
                div { class: "mt-4 space-y-3",
                    p { class: "text-sm text-stone-600 dark:text-stone-400", {t(l, "2fa.password_required")} }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "users.password")} }
                        input {
                            class: "mt-1 block w-full max-w-xs rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter {
                                    if is_disable_prompt { do_disable(); }
                                    else { do_regenerate(); }
                                }
                            },
                        }
                    }
                    div { class: "flex gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: move |_| { phase.set(Phase::Idle); password.set(String::new()); error.set(String::new()); },
                            {t(l, "common.cancel")}
                        }
                        if is_disable_prompt {
                            button {
                                class: "rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50",
                                disabled: *loading.read(),
                                onclick: move |_| do_disable(),
                                if *loading.read() { {t(l, "common.loading")} } else { {t(l, "2fa.disable")} }
                            }
                        } else {
                            button {
                                class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                                disabled: *loading.read(),
                                onclick: move |_| do_regenerate(),
                                if *loading.read() { {t(l, "common.loading")} } else { {t(l, "2fa.regenerate")} }
                            }
                        }
                    }
                }
            }

            if !error.read().is_empty() {
                div { class: "mt-3 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            }
        }
    }
}
