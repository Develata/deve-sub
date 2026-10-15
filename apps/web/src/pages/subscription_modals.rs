//! Subscription form, credential and consequence confirmations.
#![cfg(target_family = "wasm")]
use crate::i18n::{Language, t};
use crate::pages::modal_dialog::ModalDialog;
use crate::pages::subscription_types::{Modal, PROFILES};
use crate::pages::template_types::TemplateDto;
use crate::pages::util::copy_to_clipboard;
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct SubscriptionModalsProps {
    lang: Signal<Language>,
    modal: Signal<Modal>,
    templates: Signal<Vec<TemplateDto>>,
    f_name: Signal<String>,
    f_slug: Signal<String>,
    f_template: Signal<String>,
    f_profile: Signal<String>,
    f_traffic: Signal<String>,
    f_expires: Signal<Option<String>>,
    f_enabled: Signal<bool>,
    f_temp_expiry: Signal<String>,
    form_error: Signal<String>,
    saving: Signal<bool>,
    on_close: EventHandler<()>,
    on_submit: EventHandler<()>,
}

pub fn SubscriptionModals(mut props: SubscriptionModalsProps) -> Element {
    let l = *props.lang.read();
    let state = props.modal.read().clone();
    let is_edit = matches!(state, Modal::Edit(_));
    let title = match &state {
        Modal::None => return rsx! {},
        Modal::Create => t(l, "subs.add"),
        Modal::Edit(_) => t(l, "subs.edit_title"),
        Modal::Delete(_) => t(l, "common.delete"),
        Modal::Rotate(_) => t(l, "subs.rotate_token"),
        Modal::Regenerate(_) => t(l, "subs.regenerate_short_code"),
        Modal::TempLink(_) => t(l, "subs.temp_link"),
        Modal::TokenDisplay(_) => t(l, "subs.delivery_link"),
    };
    rsx! {
        ModalDialog { title, busy: *props.saving.read(), on_close: props.on_close,
            if matches!(state, Modal::Create | Modal::Edit(_)) {
                form { onsubmit: move |e| { e.prevent_default(); props.on_submit.call(()); },
                    fieldset { disabled: *props.saving.read(), class: "mt-4 min-w-0 space-y-4",
                        div {
                            label { r#for: "subscription-name", class: "block text-sm font-medium", {t(l, "subs.name")} }
                            input { id: "subscription-name", class: "source-input", r#type: "text", required: true, autofocus: true,
                                value: "{props.f_name}", oninput: move |e| props.f_name.set(e.value()) }
                        }
                        div {
                            label { r#for: "subscription-slug", class: "block text-sm font-medium", {t(l, "subs.slug")} }
                            input { id: "subscription-slug", class: "source-input", r#type: "text", required: true,
                                value: "{props.f_slug}", oninput: move |e| props.f_slug.set(e.value()) }
                        }
                        if !is_edit {
                            div {
                                label { r#for: "subscription-template", class: "block text-sm font-medium", {t(l, "subs.template")} }
                                if props.templates.read().is_empty() { p { class: "mt-1 text-sm text-red-700 dark:text-red-300", {t(l, "subs.no_template")} } }
                                else { select { id: "subscription-template", class: "source-input", value: "{props.f_template}", onchange: move |e| props.f_template.set(e.value()),
                                    for tmpl in props.templates.read().iter() { option { value: "{tmpl.id}", "{tmpl.name}" } }
                                } }
                            }
                        }
                        div {
                            label { r#for: "subscription-profile", class: "block text-sm font-medium", {t(l, "subs.profile")} }
                            select { id: "subscription-profile", class: "source-input", value: "{props.f_profile}", onchange: move |e| props.f_profile.set(e.value()),
                                for p in PROFILES.iter().copied() { option { value: "{p}", "{p}" } }
                            }
                        }
                        div {
                            label { r#for: "subscription-traffic", class: "block text-sm font-medium", {t(l, "subs.traffic_limit")} }
                            input { id: "subscription-traffic", class: "source-input", r#type: "text", inputmode: "numeric",
                                value: "{props.f_traffic}", oninput: move |e| props.f_traffic.set(e.value()) }
                        }
                        div {
                            label { r#for: "subscription-expiry", class: "block text-sm font-medium", {t(l, "subs.expires_at")} }
                            input { id: "subscription-expiry", class: "source-input", r#type: "text", placeholder: "2027-12-31T23:59:59Z",
                                value: "{props.f_expires.read().clone().unwrap_or_default()}", oninput: move |e| {
                                    let value = e.value(); props.f_expires.set(if value.is_empty() { None } else { Some(value) });
                                } }
                        }
                        if is_edit { label { class: "flex min-h-11 items-center gap-2 text-sm",
                            input { r#type: "checkbox", checked: *props.f_enabled.read(), onchange: move |e| props.f_enabled.set(e.checked()) } {t(l, "nodes.enabled")} }
                        }
                    }
                    ModalFeedback { lang: l, busy: *props.saving.read(), error: props.form_error.read().clone() }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { r#type: "button", class: "ui-button", disabled: *props.saving.read(), onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { r#type: "submit", class: "ui-button ui-button-accent", disabled: *props.saving.read(), {t(l, "common.save")} }
                    }
                }
            } else if let Modal::TokenDisplay(url) = &state {
                TokenLink { lang: l, url: url.clone(), on_close: props.on_close }
            } else if matches!(state, Modal::TempLink(_)) {
                form { onsubmit: move |e| { e.prevent_default(); props.on_submit.call(()); },
                    div { class: "mt-4",
                        label { r#for: "subscription-temp-expiry", class: "block text-sm font-medium", {t(l, "subs.temp_link_expiry")} }
                        input { id: "subscription-temp-expiry", class: "source-input", r#type: "text", required: true, autofocus: true,
                            disabled: *props.saving.read(), placeholder: "2027-12-31T23:59:59Z", value: "{props.f_temp_expiry}",
                            oninput: move |e| props.f_temp_expiry.set(e.value()) }
                    }
                    ModalFeedback { lang: l, busy: *props.saving.read(), error: props.form_error.read().clone() }
                    div { class: "mt-6 flex justify-end gap-2",
                        button { r#type: "button", class: "ui-button", disabled: *props.saving.read(), onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                        button { r#type: "submit", class: "ui-button ui-button-accent", disabled: *props.saving.read(), {t(l, "common.save")} }
                    }
                }
            } else {
                if let Modal::Delete(sub) | Modal::Rotate(sub) | Modal::Regenerate(sub) = &state {
                    p { class: "mt-4 break-words font-medium", "{sub.name}" }
                }
                p { class: "mt-3 text-sm text-stone-600 dark:text-stone-300",
                    {t(l, match state { Modal::Rotate(_) => "subs.rotate_confirm", Modal::Regenerate(_) => "subs.regenerate_confirm", _ => "subs.delete_confirm" })}
                }
                ModalFeedback { lang: l, busy: *props.saving.read(), error: props.form_error.read().clone() }
                div { class: "mt-6 flex flex-wrap justify-end gap-2",
                    button { class: "ui-button", autofocus: true, disabled: *props.saving.read(), onclick: move |_| props.on_close.call(()), {t(l, "common.cancel")} }
                    button { class: "ui-button ui-button-danger", disabled: *props.saving.read(), onclick: move |_| props.on_submit.call(()), "{title}" }
                }
            }
        }
    }
}

#[component]
fn ModalFeedback(lang: Language, busy: bool, error: String) -> Element {
    rsx! {
        if busy {
            // WHY: disabling the submitting control otherwise releases focus to the page.
            p { class: "mt-4 text-sm", role: "status", tabindex: "-1",
                onmounted: move |e| async move { let _ = e.data().set_focus(true).await; }, {t(lang, "common.loading")} }
        }
        if !error.is_empty() {
            // WHY: the focused pending status is removed when a request fails.
            p { class: "ui-error mt-4 text-sm", role: "alert", tabindex: "-1",
                onmounted: move |e| async move { let _ = e.data().set_focus(true).await; }, "{error}" }
        }
    }
}

#[component]
fn TokenLink(lang: Language, url: String, on_close: EventHandler<()>) -> Element {
    let mut copied = use_signal(|| false);
    let mut failed = use_signal(|| false);
    let copy_url = url.clone();
    rsx! {
        p { class: "mt-3 text-sm font-medium text-red-700 dark:text-red-300", {t(lang, "subs.token_warning")} }
        label { r#for: "subscription-url", class: "sr-only", {t(lang, "subs.delivery_link")} }
        // WHY: this replaces pending content inside an already-open dialog;
        // autofocus alone only handles the dialog's initial opening.
        input { id: "subscription-url", class: "source-input mt-4 font-mono", r#type: "text", readonly: true, autofocus: true, value: "{url}",
            onmounted: move |e| async move { let _ = e.data().set_focus(true).await; } }
        if *failed.read() { p { class: "ui-error mt-3 text-sm", role: "alert", {t(lang, "subs.copy_failed")} } }
        if *copied.read() { p { role: "status", class: "ui-success mt-3 text-sm", {t(lang, "subs.copied")} } }
        div { class: "mt-6 flex justify-end gap-2",
            button { class: "ui-button", onclick: move |_| {
                let url = copy_url.clone();
                spawn(async move { let result = copy_to_clipboard(&url).await; copied.set(result.is_ok()); failed.set(result.is_err()); });
            }, {t(lang, "common.copy")} }
            button { class: "ui-button ui-button-accent", onclick: move |_| on_close.call(()), "OK" }
        }
    }
}
