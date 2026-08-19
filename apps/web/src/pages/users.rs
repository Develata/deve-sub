//! Users page — list, create, disable, force-logout (AUTH-007, AUTH-010).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::user_modals::{UserModals, UserModalsProps};
use crate::pages::user_types::{
    CreateUserRequest, CreateUserResponse, ListUsersResponse, Modal, UserDto,
};

#[derive(Props, Clone, PartialEq)]
pub struct UsersProps {
    lang: Signal<Language>,
}

pub fn UsersPage(props: UsersProps) -> Element {
    let l = *props.lang.read();
    let mut users = use_signal(Vec::<UserDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new);
    let mut saving = use_signal(|| false);
    let mut info_msg = use_signal(|| String::new());

    let mut f_username = use_signal(String::new);
    let mut f_password = use_signal(String::new);
    let mut f_role = use_signal(|| "user".to_string());

    let fetch_users = move || {
        spawn(async move {
            loading.set(true);
            match crate::api::get::<ListUsersResponse>("/users?limit=100").await {
                Ok(resp) => {
                    users.set(resp.users);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    use_future(move || async move {
        fetch_users();
    });

    let open_create = move |_| {
        f_username.set(String::new());
        f_password.set(String::new());
        f_role.set("user".to_string());
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let open_disable = move |u: UserDto| {
        form_error.set(String::new());
        modal.set(Modal::Disable(u));
    };

    let open_force_logout = move |u: UserDto| {
        form_error.set(String::new());
        modal.set(Modal::ForceLogout(u));
    };

    let close_modal = move |_| {
        modal.set(Modal::None);
    };

    let do_submit = move |_| {
        let state = (*modal.read()).clone();
        match state {
            Modal::Create => {
                let username = f_username.read().clone();
                let password = f_password.read().clone();
                if username.is_empty() || password.is_empty() {
                    form_error.set("Username and password are required".to_string());
                    return;
                }
                let req = CreateUserRequest {
                    username,
                    password,
                    role: f_role.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    match crate::api::send::<CreateUserResponse, CreateUserRequest>(
                        "POST", "/users", Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_users();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Disable(u) => {
                let id = u.id.clone();
                saving.set(true);
                spawn(async move {
                    let path = format!("/users/{id}/disable");
                    match crate::api::send::<(), serde_json::Value>("POST", &path, None).await {
                        Ok(_) => {
                            info_msg.set(format!("{}: {}", t(l, "users.disabled"), u.username.clone()));
                            modal.set(Modal::None);
                            fetch_users();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::ForceLogout(u) => {
                let id = u.id.clone();
                saving.set(true);
                spawn(async move {
                    let path = format!("/users/{id}/force-logout");
                    match crate::api::send::<(), serde_json::Value>("POST", &path, None).await {
                        Ok(_) => {
                            info_msg.set(format!("{}: {}", t(l, "users.force_logout_done"), u.username.clone()));
                            modal.set(Modal::None);
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::None => {}
        }
    };

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.users")} }
                button {
                    class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: open_create,
                    {t(l, "users.add")}
                }
            }

            if !info_msg.read().is_empty() {
                div { class: "rounded-md bg-green-50 p-3 text-sm text-green-700 dark:bg-green-900/20 dark:text-green-400", "{info_msg}" }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if users.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "users.empty")} }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "users.username")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "users.role")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "users.2fa")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "users.last_login")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for u in users.read().iter() {
                                {
                                    let id = u.id.clone();
                                    let dis_u = u.clone();
                                    let flo_u = u.clone();
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{u.username}" }
                                            td { class: "px-4 py-3",
                                                span {
                                                    class: if u.role == "admin" {
                                                        "inline-flex rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-700 dark:bg-amber-900/30 dark:text-amber-300"
                                                    } else {
                                                        "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400"
                                                    },
                                                    "{u.role}"
                                                }
                                            }
                                            td { class: "px-4 py-3",
                                                if u.two_factor_enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "users.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "users.disabled")} }
                                                }
                                            }
                                            td { class: "px-4 py-3",
                                                if u.enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400",
                                                if let Some(ts) = &u.last_login_at {
                                                    "{ts}"
                                                } else {
                                                    "—"
                                                }
                                            }
                                            td { class: "px-4 py-3 text-right",
                                                div { class: "flex flex-wrap justify-end gap-1",
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_force_logout(flo_u.clone()),
                                                        {t(l, "users.force_logout")}
                                                    }
                                                    if u.enabled {
                                                        button {
                                                            class: "rounded-md border border-red-300 px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                                                            onclick: move |_| open_disable(dis_u.clone()),
                                                            {t(l, "users.disable")}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        UserModals {
            lang: props.lang,
            modal,
            f_username,
            f_password,
            f_role,
            form_error,
            saving,
            on_close: close_modal,
            on_submit: do_submit,
        }
    }
}
