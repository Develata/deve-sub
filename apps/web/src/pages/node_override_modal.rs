//! Node override modal (NODE-010) and region modal (NODE-006).
//!
//! Override form: display_name, region, enabled, sni, skip_cert_verify,
//! fingerprint, sort_order. Boolean fields use a tri-state select
//! (inherit / true / false) to express `None` (clear the override).
//! Region modal: single text field, empty = clear manual region.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::send;
use crate::i18n::{Language, t};
use crate::pages::node_types::{SetRegionRequest, UpdateOverrideRequest};

/// Tri-state for optional booleans in the override form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriBool {
    Inherit,
    True,
    False,
}

impl TriBool {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::True => "true",
            Self::False => "false",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "true" => Self::True,
            "false" => Self::False,
            _ => Self::Inherit,
        }
    }

    const fn to_option_bool(self) -> Option<bool> {
        match self {
            Self::Inherit => None,
            Self::True => Some(true),
            Self::False => Some(false),
        }
    }
}

#[derive(Props, Clone)]
pub struct OverrideModalProps {
    lang: Signal<Language>,
    node_id: String,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

pub fn OverrideModal(props: OverrideModalProps) -> Element {
    let l = *props.lang.read();
    let mut f_name = use_signal(String::new);
    let mut f_region = use_signal(String::new);
    let mut f_enabled = use_signal(|| TriBool::Inherit);
    let mut f_sni = use_signal(String::new);
    let mut f_skip_verify = use_signal(|| TriBool::Inherit);
    let mut f_fingerprint = use_signal(String::new);
    let mut f_sort = use_signal(|| 0_i64);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(String::new);

    let submit = move |_| {
        saving.set(true);
        error.set(String::new());
        let req = UpdateOverrideRequest {
            display_name: opt_str(&f_name.read()),
            region: opt_str(&f_region.read()),
            enabled: f_enabled.read().to_option_bool(),
            sni: opt_str(&f_sni.read()),
            skip_cert_verify: f_skip_verify.read().to_option_bool(),
            fingerprint: opt_str(&f_fingerprint.read()),
            sort_order: *f_sort.read(),
        };
        let nid = props.node_id.clone();
        spawn(async move {
            let path = format!("/nodes/{nid}/override");
            match send::<serde_json::Value, _>("PATCH", &path, Some(&req)).await {
                Ok(_) => {
                    saving.set(false);
                    props.on_success.call(());
                    props.on_close.call(());
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    let delete_override = move |_| {
        saving.set(true);
        error.set(String::new());
        let nid = props.node_id.clone();
        spawn(async move {
            let path = format!("/nodes/{nid}/override");
            match crate::api::delete(&path).await {
                Ok(()) => {
                    saving.set(false);
                    props.on_success.call(());
                    props.on_close.call(());
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            onclick: move |_| props.on_close.call(()),
            div {
                class: "w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                onclick: move |e| e.stop_propagation(),
                h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                    {t(l, "nodes.override_title")}
                }
                div { class: "mt-4 space-y-3",
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_name")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            value: "{f_name}",
                            oninput: move |e| f_name.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_region")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            value: "{f_region}",
                            oninput: move |e| f_region.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_enabled")} }
                        select {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            value: "{f_enabled.read().as_str()}",
                            onchange: move |e| f_enabled.set(TriBool::from_str(&e.value())),
                            option { value: "inherit", {t(l, "nodes.override_inherit")} }
                            option { value: "true", {t(l, "nodes.override_true")} }
                            option { value: "false", {t(l, "nodes.override_false")} }
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_sni")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            value: "{f_sni}",
                            oninput: move |e| f_sni.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_skip_verify")} }
                        select {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            value: "{f_skip_verify.read().as_str()}",
                            onchange: move |e| f_skip_verify.set(TriBool::from_str(&e.value())),
                            option { value: "inherit", {t(l, "nodes.override_inherit")} }
                            option { value: "true", {t(l, "nodes.override_true")} }
                            option { value: "false", {t(l, "nodes.override_false")} }
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_fingerprint")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            value: "{f_fingerprint}",
                            oninput: move |e| f_fingerprint.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.override_sort")} }
                        input {
                            class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "number",
                            value: "{f_sort}",
                            oninput: move |e| { f_sort.set(e.value().parse::<i64>().unwrap_or(0)); },
                        }
                    }
                }
                if !error.read().is_empty() {
                    div { class: "mt-3 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
                }
                div { class: "mt-6 flex justify-between",
                    button {
                        class: "rounded-md border border-red-300 px-4 py-2 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20",
                        disabled: *saving.read(),
                        onclick: delete_override,
                        {t(l, "nodes.override_delete")}
                    }
                    div { class: "flex gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: move |_| props.on_close.call(()),
                            {t(l, "common.cancel")}
                        }
                        button {
                            class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                            disabled: *saving.read(),
                            onclick: submit,
                            if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone)]
pub struct RegionModalProps {
    lang: Signal<Language>,
    node_id: String,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

pub fn RegionModal(props: RegionModalProps) -> Element {
    let l = *props.lang.read();
    let mut region = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(String::new);

    let submit = move |_| {
        saving.set(true);
        error.set(String::new());
        let req = SetRegionRequest {
            region: opt_str(&region.read()),
        };
        let nid = props.node_id.clone();
        spawn(async move {
            let path = format!("/nodes/{nid}/region");
            match send::<serde_json::Value, _>("PATCH", &path, Some(&req)).await {
                Ok(_) => {
                    saving.set(false);
                    props.on_success.call(());
                    props.on_close.call(());
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            onclick: move |_| props.on_close.call(()),
            div {
                class: "w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                onclick: move |e| e.stop_propagation(),
                h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                    {t(l, "nodes.region_title")}
                }
                p { class: "mt-2 text-sm text-stone-500 dark:text-stone-400",
                    {t(l, "nodes.region_hint")}
                }
                div { class: "mt-4",
                    label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300", {t(l, "nodes.region_label")} }
                    input {
                        class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                        r#type: "text",
                        placeholder: "US",
                        value: "{region}",
                        oninput: move |e| region.set(e.value()),
                    }
                }
                if !error.read().is_empty() {
                    div { class: "mt-3 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
                }
                div { class: "mt-6 flex justify-end gap-2",
                    button {
                        class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| props.on_close.call(()),
                        {t(l, "common.cancel")}
                    }
                    button {
                        class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                        disabled: *saving.read(),
                        onclick: submit,
                        if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                    }
                }
            }
        }
    }
}

/// Convert empty string to None, non-empty to Some.
fn opt_str(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
