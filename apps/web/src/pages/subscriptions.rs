//! Subscriptions page — list, create, edit, delete, token rotation, short
//! code, temp links (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::subscription_modals::{SubscriptionModals, SubscriptionModalsProps};
use crate::pages::subscription_types::{
    CreateSubscriptionRequest, CreateTempLinkRequest, CreateTempLinkResponse, GetSubscriptionResponse,
    ListSubscriptionsResponse, Modal, PROFILES, RotateTokenRequest, ShortCodeResponse, SubscriptionDto,
    SubscriptionResponse, TokenRotationResponse, UpdateSubscriptionRequest,
};
use crate::pages::template_types::{ListTemplatesResponse, TemplateDto};
use crate::pages::util::copy_to_clipboard;

#[derive(Props, Clone, PartialEq)]
pub struct SubscriptionsProps {
    lang: Signal<Language>,
}

pub fn SubscriptionsPage(props: SubscriptionsProps) -> Element {
    let l = *props.lang.read();
    let mut subs = use_signal(Vec::<SubscriptionDto>::new);
    let mut templates = use_signal(Vec::<TemplateDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut copied_id = use_signal(|| String::new());
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new());
    let mut saving = use_signal(|| false);
    let mut info_msg = use_signal(|| String::new());

    let mut f_name = use_signal(String::new);
    let mut f_slug = use_signal(String::new);
    let mut f_template = use_signal(String::new);
    let mut f_profile = use_signal(|| "mihomo".to_string());
    let mut f_traffic = use_signal(|| Option::<u64>::None);
    let mut f_expires = use_signal(|| Option::<String>::None);
    let mut f_enabled = use_signal(|| true);
    let mut f_node_sel = use_signal(|| serde_json::json!({"mode": "all"}));
    let mut f_temp_expiry = use_signal(String::new);

    let fetch_subs = move || {
        spawn(async move {
            loading.set(true);
            match crate::api::get::<ListSubscriptionsResponse>("/subscriptions").await {
                Ok(resp) => {
                    subs.set(resp.subscriptions);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    use_future(move || async move {
        fetch_subs();
        let _ = crate::api::get::<ListTemplatesResponse>("/templates").await.map(|r| templates.set(r.templates));
    });

    let copy_link = move |id: String, slug: String| {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default();
        let link = format!("{origin}/sub/{slug}");
        spawn(async move {
            let _ = copy_to_clipboard(&link).await;
            copied_id.set(id);
        });
    };

    let open_create = move |_| {
        f_name.set(String::new());
        f_slug.set(String::new());
        f_template.set(templates.read().first().map(|t| t.id.clone()).unwrap_or_default());
        f_profile.set("mihomo".to_string());
        f_traffic.set(None);
        f_expires.set(None);
        f_enabled.set(true);
        f_node_sel.set(serde_json::json!({"mode": "all"}));
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let mut open_edit = move |s: SubscriptionDto| {
        f_name.set(s.name.clone());
        f_slug.set(s.slug.clone());
        f_template.set(s.template_id.clone());
        f_profile.set(s.profile.clone());
        f_traffic.set(s.traffic_limit);
        f_expires.set(s.expires_at.clone());
        f_enabled.set(s.enabled);
        f_node_sel.set(s.node_selection.clone());
        form_error.set(String::new());
        modal.set(Modal::Edit(s));
    };

    let mut open_delete = move |s: SubscriptionDto| {
        form_error.set(String::new());
        modal.set(Modal::Delete(s));
    };

    let mut open_temp_link = move |s: SubscriptionDto| {
        f_temp_expiry.set(String::new());
        form_error.set(String::new());
        modal.set(Modal::TempLink(s));
    };

    let close_modal = move |_: Event<MouseData>| {
        modal.set(Modal::None);
    };

    let mut do_rotate = move |id: String| {
        info_msg.set(String::new());
        spawn(async move {
            let path = format!("/subscriptions/{id}/rotate-token");
            match crate::api::send::<TokenRotationResponse, RotateTokenRequest>(
                "POST", &path, Some(&RotateTokenRequest { grace_seconds: None }),
            )
            .await
            {
                Ok(resp) => modal.set(Modal::TokenDisplay(resp.token_plaintext)),
                Err(e) => error.set(e.message),
            }
        });
    };

    let mut do_regen_short_code = move |id: String| {
        info_msg.set(String::new());
        spawn(async move {
            let path = format!("/subscriptions/{id}/regenerate-short-code");
            match crate::api::send::<ShortCodeResponse, serde_json::Value>("POST", &path, None)
                .await
            {
                Ok(resp) => {
                    info_msg.set(format!("{}: {}", t(l, "subs.short_code"), resp.code));
                    fetch_subs();
                }
                Err(e) => error.set(e.message),
            }
        });
    };

    let do_submit = move |_| {
        let state = (*modal.read()).clone();
        match state {
            Modal::Create => {
                let name = f_name.read().clone();
                let slug = f_slug.read().clone();
                let template_id = f_template.read().clone();
                if name.is_empty() || slug.is_empty() || template_id.is_empty() {
                    form_error.set("Name, slug, and template are required".to_string());
                    return;
                }
                let req = CreateSubscriptionRequest {
                    name,
                    slug,
                    template_id,
                    profile: f_profile.read().clone(),
                    node_selection: f_node_sel.read().clone(),
                    traffic_limit: *f_traffic.read(),
                    expires_at: f_expires.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    match crate::api::send::<SubscriptionResponse, CreateSubscriptionRequest>(
                        "POST", "/subscriptions", Some(&req),
                    )
                    .await
                    {
                        Ok(resp) => {
                            modal.set(Modal::TokenDisplay(resp.token_plaintext));
                            fetch_subs();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Edit(s) => {
                let id = s.id.clone();
                let name = f_name.read().clone();
                let slug = f_slug.read().clone();
                if name.is_empty() || slug.is_empty() {
                    form_error.set("Name and slug are required".to_string());
                    return;
                }
                let req = UpdateSubscriptionRequest {
                    name,
                    slug,
                    template_version_pin: s.template_version_pin,
                    profile: f_profile.read().clone(),
                    node_selection: f_node_sel.read().clone(),
                    traffic_limit: *f_traffic.read(),
                    expires_at: f_expires.read().clone(),
                    enabled: Some(*f_enabled.read()),
                };
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{id}");
                    match crate::api::send::<GetSubscriptionResponse, UpdateSubscriptionRequest>(
                        "PUT", &path, Some(&req),
                    )
                    .await
                    {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_subs();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Delete(s) => {
                let id = s.id.clone();
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{id}");
                    match crate::api::delete(&path).await {
                        Ok(_) => {
                            modal.set(Modal::None);
                            fetch_subs();
                        }
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::TempLink(s) => {
                let id = s.id.clone();
                let expiry = f_temp_expiry.read().clone();
                if expiry.is_empty() {
                    form_error.set("Expiry is required".to_string());
                    return;
                }
                let req = CreateTempLinkRequest { expires_at: expiry };
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{id}/temp-links");
                    match crate::api::send::<CreateTempLinkResponse, CreateTempLinkRequest>(
                        "POST", &path, Some(&req),
                    )
                    .await
                    {
                        Ok(resp) => modal.set(Modal::TokenDisplay(resp.token_plaintext)),
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::None | Modal::TokenDisplay(_) => {}
        }
    };

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nav.subscriptions")} }
                button {
                    class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: open_create,
                    {t(l, "subs.add")}
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
            } else if subs.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", "暂无订阅" }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "名称" }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "Slug" }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "Profile" }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "subs.short_code")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", "" }
                            }
                        }
                        tbody {
                            for sub in subs.read().iter() {
                                {
                                    let id = sub.id.clone();
                                    let slug = sub.slug.clone();
                                    let is_copied = *copied_id.read() == id;
                                    let edit_sub = sub.clone();
                                    let del_sub = sub.clone();
                                    let temp_sub = sub.clone();
                                    let rot_id = id.clone();
                                    let regen_id = id.clone();
                                    rsx! {
                                        tr {
                                            key: "{id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 font-medium text-stone-900 dark:text-stone-100", "{sub.name}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400", "{sub.slug}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400", "{sub.profile}" }
                                            td { class: "px-4 py-3 text-stone-500 dark:text-stone-400",
                                                if let Some(code) = &sub.short_code {
                                                    span { class: "font-mono text-xs", "{code}" }
                                                } else {
                                                    span { class: "text-stone-400", "—" }
                                                }
                                            }
                                            td { class: "px-4 py-3",
                                                if sub.enabled {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            td { class: "px-4 py-3 text-right",
                                                div { class: "flex flex-wrap justify-end gap-1",
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| copy_link(id.clone(), slug.clone()),
                                                        if is_copied { "✓" } else { {t(l, "common.copy")} }
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_edit(edit_sub.clone()),
                                                        {t(l, "common.edit")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| do_rotate(rot_id.clone()),
                                                        {t(l, "subs.rotate_token")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| do_regen_short_code(regen_id.clone()),
                                                        {t(l, "subs.regenerate_short_code")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-stone-300 px-2 py-1 text-xs text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                                                        onclick: move |_| open_temp_link(temp_sub.clone()),
                                                        {t(l, "subs.temp_link")}
                                                    }
                                                    button {
                                                        class: "rounded-md border border-red-300 px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                                                        onclick: move |_| open_delete(del_sub.clone()),
                                                        {t(l, "common.delete")}
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

        SubscriptionModals {
            lang: props.lang,
            modal,
            templates,
            f_name,
            f_slug,
            f_template,
            f_profile,
            f_traffic,
            f_expires,
            f_enabled,
            f_temp_expiry,
            form_error,
            saving,
            on_close: move |_| modal.set(Modal::None),
            on_submit: do_submit,
        }
    }
}
