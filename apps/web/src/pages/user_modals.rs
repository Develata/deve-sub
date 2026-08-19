//! Modal dialogs for the users page (create, disable, force-logout).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::user_types::{Modal, ROLES};

#[derive(Props, Clone)]
pub struct UserModalsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    f_username: Signal<String>,
    f_password: Signal<String>,
    f_role: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_submit: EventHandler<()>,
}

pub fn UserModals(props: UserModalsProps) -> Element {
    let l = *props.lang.read();
    let is_create_modal = matches!(*props.modal.read(), Modal::Create);
    let is_disable_modal = matches!(*props.modal.read(), Modal::Disable(_));
    let is_force_logout_modal = matches!(*props.modal.read(), Modal::ForceLogout(_));

    let target_name = match &*props.modal.read() {
        Modal::Disable(u) | Modal::ForceLogout(u) => u.username.clone(),
        _ => String::new(),
    };

    rsx! {
        if is_create_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "users.add")} }
                    div { class: "mt-4 space-y-4",
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "users.username")} }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "text", value: "{props.f_username}", oninput: move |e| props.f_username.set(e.value()) }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "users.password")} }
                            input { class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800", r#type: "password", value: "{props.f_password}", oninput: move |e| props.f_password.set(e.value()) }
                        }
                        div {
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "users.role")} }
                            select {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "{props.f_role}",
                                onchange: move |e| props.f_role.set(e.value()),
                                for r in ROLES.iter().copied() {
                                    option { value: "{r}", "{r}" }
                                }
                            }
                        }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }

        if is_disable_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "users.disable_title")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400",
                        {t(l, "users.disable_confirm")}
                        span { class: "font-medium", " {target_name}" }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "users.disable")} }
                        }
                    }
                }
            }
        }

        if is_force_logout_modal {
            div {
                class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                onclick: move |_| props.on_close.call(()),
                div {
                    class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                    onclick: move |e| e.stop_propagation(),
                    h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "users.force_logout_title")} }
                    p { class: "mt-3 text-sm text-stone-600 dark:text-stone-400",
                        {t(l, "users.force_logout_confirm")}
                        span { class: "font-medium", " {target_name}" }
                    }
                    if !props.form_error.read().is_empty() {
                        div { class: "mt-4 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{props.form_error}" }
                    }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800", onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()),
                            if *props.saving.read() { {t(l, "common.loading")} } else { {t(l, "users.force_logout")} }
                        }
                    }
                }
            }
        }
    }
}
