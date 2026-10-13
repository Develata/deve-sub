//! Subscriptions page — list, create, edit, delete, token rotation, short
//! code, temp links (UI-009).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::subscription_modals::SubscriptionModals;
use crate::pages::subscription_row::SubscriptionRow;
use crate::pages::subscription_types::{
    CreateSubscriptionRequest, CreateTempLinkRequest, CreateTempLinkResponse,
    GetSubscriptionResponse, ListSubscriptionsResponse, Modal, RotateTokenRequest,
    ShortCodeResponse, SubscriptionDto, SubscriptionResponse, TokenRotationResponse,
    UpdateSubscriptionRequest,
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
    let mut next_cursor = use_signal(|| None::<String>);
    let mut page_busy = use_signal(|| false);
    let mut list_revision = use_signal(|| 0_u64);
    let mut page_error = use_signal(String::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut copied_id = use_signal(|| String::new());
    let mut action_error = use_signal(String::new);
    let mut modal = use_signal(|| Modal::None);
    let mut form_error = use_signal(|| String::new());
    let mut saving = use_signal(|| false);
    let mut info_msg = use_signal(|| String::new());

    let mut f_name = use_signal(String::new);
    let mut f_slug = use_signal(String::new);
    let mut f_template = use_signal(String::new);
    let mut f_profile = use_signal(|| "mihomo".to_string());
    let mut f_traffic = use_signal(String::new);
    let mut f_expires = use_signal(|| Option::<String>::None);
    let mut f_enabled = use_signal(|| true);
    let mut f_node_sel = use_signal(|| serde_json::json!({"mode": "dynamic"}));
    let mut f_temp_expiry = use_signal(String::new);

    let mut fetch_subs = move || {
        *list_revision.write() += 1;
        let revision = *list_revision.read();
        spawn(async move {
            loading.set(true);
            let result = crate::api::get::<ListSubscriptionsResponse>("/subscriptions").await;
            if revision != *list_revision.read() {
                return;
            }
            match result {
                Ok(resp) => {
                    next_cursor.set(resp.next_cursor);
                    subs.set(resp.subscriptions);
                    error.set(String::new());
                }
                Err(e) => error.set(e.message),
            }
            loading.set(false);
        });
    };

    let load_more = move |_| {
        if *page_busy.read() || *loading.read() {
            return;
        }
        let Some(cursor) = next_cursor.read().clone() else {
            return;
        };
        let revision = *list_revision.read();
        page_busy.set(true);
        page_error.set(String::new());
        spawn(async move {
            match crate::api::get::<ListSubscriptionsResponse>(&format!(
                "/subscriptions?cursor={cursor}"
            ))
            .await
            {
                Ok(resp) if revision == *list_revision.read() => {
                    next_cursor.set(resp.next_cursor);
                    subs.write().extend(resp.subscriptions);
                }
                Ok(_) => {}
                Err(e) if revision == *list_revision.read() => page_error.set(e.message),
                Err(_) => {}
            }
            page_busy.set(false);
        });
    };

    use_future(move || async move {
        fetch_subs();
        let mut path = "/templates?limit=100".to_string();
        let deadline = js_sys::Date::now() + 30_000.0;
        let mut seen = std::collections::HashSet::new();
        loop {
            if js_sys::Date::now() >= deadline || !seen.insert(path.clone()) {
                action_error.set(t(l, "subs.templates_incomplete").into());
                break;
            }
            match crate::api::get::<ListTemplatesResponse>(&path).await {
                Ok(resp) => {
                    templates.write().extend(resp.templates);
                    match resp.next_cursor {
                        Some(cursor) => path = format!("/templates?limit=100&cursor={cursor}"),
                        None => break,
                    }
                }
                Err(e) => {
                    action_error.set(e.message);
                    break;
                }
            }
        }
    });

    let mut copy_link = move |id: String| {
        action_error.set(String::new());
        copied_id.set(String::new());
        spawn(async move {
            let path = format!("/subscriptions/{id}");
            match crate::api::get::<GetSubscriptionResponse>(&path).await {
                Ok(resp) => match resp.subscription.short_code {
                    Some(code) => {
                        let link = crate::pages::subscription_types::delivery_url(
                            "s",
                            &code,
                            &resp.subscription.profile,
                        );
                        match copy_to_clipboard(&link).await {
                            Ok(()) => copied_id.set(id),
                            Err(_) => action_error.set(t(l, "subs.copy_failed").to_string()),
                        }
                    }
                    None => action_error.set(t(l, "subs.generate_code_first").to_string()),
                },
                Err(e) => action_error.set(e.message),
            }
        });
    };

    let open_create = move |_| {
        if *saving.read() {
            return;
        }
        f_name.set(String::new());
        f_slug.set(String::new());
        f_template.set(
            templates
                .read()
                .first()
                .map(|t| t.id.clone())
                .unwrap_or_default(),
        );
        f_profile.set("mihomo".to_string());
        f_traffic.set(String::new());
        f_expires.set(None);
        f_enabled.set(true);
        f_node_sel.set(serde_json::json!({"mode": "dynamic"}));
        form_error.set(String::new());
        modal.set(Modal::Create);
    };

    let mut open_edit = move |s: SubscriptionDto| {
        if *saving.read() {
            return;
        }
        f_name.set(s.name.clone());
        f_slug.set(s.slug.clone());
        f_template.set(s.template_id.clone());
        f_profile.set(s.profile.clone());
        f_traffic.set(
            s.traffic_limit
                .map(|limit| limit.to_string())
                .unwrap_or_default(),
        );
        f_expires.set(s.expires_at.clone());
        f_enabled.set(s.enabled);
        f_node_sel.set(s.node_selection.clone());
        form_error.set(String::new());
        modal.set(Modal::Edit(s));
    };

    let mut open_delete = move |s: SubscriptionDto| {
        if *saving.read() {
            return;
        }
        form_error.set(String::new());
        modal.set(Modal::Delete(s));
    };

    let mut open_temp_link = move |s: SubscriptionDto| {
        if *saving.read() {
            return;
        }
        f_temp_expiry.set(String::new());
        form_error.set(String::new());
        modal.set(Modal::TempLink(s));
    };

    let close_modal = move |_| {
        if *saving.read() {
            return;
        }
        modal.set(Modal::None);
    };

    let mut do_rotate = move |sub: SubscriptionDto| {
        if *saving.read() {
            return;
        }
        form_error.set(String::new());
        modal.set(Modal::Rotate(sub));
    };

    let mut open_regenerate = move |sub: SubscriptionDto| {
        if *saving.read() {
            return;
        }
        form_error.set(String::new());
        modal.set(Modal::Regenerate(sub));
    };

    let do_submit = move |_| {
        if *saving.read() {
            return;
        }
        let state = (*modal.read()).clone();
        let traffic_limit = if matches!(state, Modal::Create | Modal::Edit(_)) {
            match crate::pages::subscription_types::parse_traffic_limit(&f_traffic.read()) {
                Ok(value) => value,
                Err(_) => {
                    form_error.set(t(l, "subs.invalid_traffic_limit").to_string());
                    return;
                }
            }
        } else {
            None
        };
        form_error.set(String::new());
        match state {
            Modal::Create => {
                let name = f_name.read().clone();
                let slug = f_slug.read().clone();
                let template_id = f_template.read().clone();
                if name.is_empty() || slug.is_empty() || template_id.is_empty() {
                    form_error.set(t(l, "subs.required_create").to_string());
                    return;
                }
                let req = CreateSubscriptionRequest {
                    name,
                    slug,
                    template_id,
                    profile: f_profile.read().clone(),
                    node_selection: f_node_sel.read().clone(),
                    traffic_limit,
                    expires_at: f_expires.read().clone(),
                };
                saving.set(true);
                spawn(async move {
                    match crate::api::send::<SubscriptionResponse, CreateSubscriptionRequest>(
                        "POST",
                        "/subscriptions",
                        Some(&req),
                    )
                    .await
                    {
                        Ok(resp) => {
                            modal.set(Modal::TokenDisplay(
                                crate::pages::subscription_types::delivery_url(
                                    "sub",
                                    &resp.token_plaintext,
                                    &resp.subscription.profile,
                                ),
                            ));
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
                    form_error.set(t(l, "subs.required_edit").to_string());
                    return;
                }
                let req = UpdateSubscriptionRequest {
                    name,
                    slug,
                    template_version_pin: s.template_version_pin,
                    profile: f_profile.read().clone(),
                    node_selection: f_node_sel.read().clone(),
                    traffic_limit,
                    expires_at: f_expires.read().clone(),
                    enabled: Some(*f_enabled.read()),
                };
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{id}");
                    match crate::api::send::<GetSubscriptionResponse, UpdateSubscriptionRequest>(
                        "PUT",
                        &path,
                        Some(&req),
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
            Modal::Rotate(s) => {
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{}/rotate-token", s.id);
                    match crate::api::send::<TokenRotationResponse, RotateTokenRequest>(
                        "POST",
                        &path,
                        Some(&RotateTokenRequest {
                            grace_seconds: Some(0),
                        }),
                    )
                    .await
                    {
                        Ok(resp) => modal.set(Modal::TokenDisplay(
                            crate::pages::subscription_types::delivery_url(
                                "sub",
                                &resp.token_plaintext,
                                &s.profile,
                            ),
                        )),
                        Err(e) => form_error.set(e.message),
                    }
                    saving.set(false);
                });
            }
            Modal::Regenerate(s) => {
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{}/regenerate-short-code", s.id);
                    match crate::api::send::<ShortCodeResponse, serde_json::Value>(
                        "POST", &path, None,
                    )
                    .await
                    {
                        Ok(_) => {
                            action_error.set(String::new());
                            info_msg.set(t(l, "subs.regenerated").to_string());
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
                    form_error.set(t(l, "subs.required_expiry").to_string());
                    return;
                }
                let req = CreateTempLinkRequest { expires_at: expiry };
                saving.set(true);
                spawn(async move {
                    let path = format!("/subscriptions/{id}/temp-links");
                    match crate::api::send::<CreateTempLinkResponse, CreateTempLinkRequest>(
                        "POST",
                        &path,
                        Some(&req),
                    )
                    .await
                    {
                        Ok(resp) => modal.set(Modal::TokenDisplay(
                            crate::pages::subscription_types::delivery_url(
                                "sub",
                                &resp.token_plaintext,
                                &s.profile,
                            ),
                        )),
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
                    class: "ui-button ui-button-accent",
                    onclick: open_create,
                    {t(l, "subs.add")}
                }
            }

            if !action_error.read().is_empty() {
                div { class: "ui-error text-sm", role: "alert", "{action_error}" }
            }
            if !info_msg.read().is_empty() {
                div { class: "ui-success text-sm", role: "status", "{info_msg}" }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else if subs.read().is_empty() {
                div { class: "rounded-md border border-stone-200 p-8 text-center dark:border-stone-800",
                    p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "subs.empty")} }
                }
            } else {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "subscription-table text-sm", role: "table", aria_label: t(l, "nav.subscriptions"),
                        thead { role: "rowgroup",
                            tr { role: "row", class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "subs.name")} }
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", "Slug" }
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "subs.profile")} }
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "subs.short_code")} }
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                th { scope: "col", role: "columnheader", class: "px-4 py-3 text-right font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.actions")} }
                            }
                        }
                        tbody { role: "rowgroup",
                            for sub in subs.read().iter() {
                                SubscriptionRow { key: "{sub.id}", lang: l, sub: sub.clone(), copied: *copied_id.read() == sub.id,
                                    on_copy: move |id| copy_link(id), on_edit: move |sub| open_edit(sub),
                                    on_rotate: move |sub| do_rotate(sub), on_regenerate: move |sub| open_regenerate(sub),
                                    on_temp_link: move |sub| open_temp_link(sub), on_delete: move |sub| open_delete(sub) }
                            }
                        }
                    }
                }
            }
            if !page_error.read().is_empty() { p { role: "alert", class: "text-sm text-red-600", "{page_error}" } }
            if next_cursor.read().is_some() { button { class: "node-control", disabled: *page_busy.read() || *loading.read(), onclick: load_more, {t(l, "nodes.load_more")} } }
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
            on_close: close_modal,
            on_submit: do_submit,
        }
    }
}
