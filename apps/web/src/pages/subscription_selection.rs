//! Subscription selection intent; resolution and compatibility stay on the server.
#![cfg(target_family = "wasm")]

use deve_sub_contract::{ListNodesResponse, NodeDto};
use dioxus::prelude::*;
use serde_json::{Value, json};

use crate::i18n::{Language, t};

#[component]
pub fn SubscriptionSelection(lang: Language, mut selection: Signal<Value>) -> Element {
    let mode = selection.read()["mode"].as_str().unwrap_or("inherit_template").to_owned();
    let filters = selection.read()["filters"].as_array().cloned().unwrap_or_default();
    rsx! {
        fieldset { class: "min-w-0 rounded-md border border-line-soft p-3",
            legend { class: "px-1 text-sm font-medium", {t(lang, "selection.title")} }
            label { r#for: "subscription-selection", class: "sr-only", {t(lang, "selection.title")} }
            select { id: "subscription-selection", class: "source-input", value: "{mode}",
                onchange: move |event| selection.set(json!({"mode": event.value(), "filters": [], "nodeIds": []})),
                option { value: "inherit_template", {t(lang, "selection.inherit")} }
                option { value: "dynamic", {t(lang, "selection.dynamic")} }
                option { value: "fixed", {t(lang, "selection.fixed")} }
            }
            p { class: "mt-2 text-sm text-muted",
                {t(lang, match mode.as_str() {
                    "dynamic" => "selection.dynamic_help",
                    "fixed" => "selection.fixed_help",
                    _ => "selection.inherit_help",
                })}
            }
            if mode == "dynamic" {
                for (index, filter) in filters.iter().enumerate() {
                    div { key: "{index}", class: "mt-2 flex flex-wrap items-end gap-2",
                        label { class: "min-w-24 flex-1 text-sm",
                            {t(lang, "selection.field")}
                            select { class: "source-input", value: filter["field"].as_str().unwrap_or("protocol"),
                                onchange: move |e| selection.write()["filters"][index]["field"] = json!(e.value()),
                                for field in ["protocol", "region", "tag", "source"] {
                                    option { value: field, {t(lang, &format!("selection.{field}")).to_owned()} }
                                }
                            }
                        }
                        label { class: "min-w-32 flex-1 text-sm",
                            {t(lang, "selection.value")}
                            input { class: "source-input", value: filter["value"].as_str().unwrap_or(""), required: true,
                                oninput: move |e| selection.write()["filters"][index]["value"] = json!(e.value()) }
                        }
                        button { r#type: "button", class: "ui-button", aria_label: t(lang, "selection.remove_filter"),
                            onclick: move |_| {
                                if let Some(filters) = selection.write()["filters"].as_array_mut() {
                                    filters.remove(index);
                                }
                            }, {t(lang, "common.delete")}
                        }
                    }
                }
                button { r#type: "button", class: "ui-button mt-3",
                    onclick: move |_| {
                        let mut value = selection.write();
                        if !value["filters"].is_array() { value["filters"] = json!([]); }
                        if let Some(filters) = value["filters"].as_array_mut() {
                            filters.push(json!({"field":"protocol","value":""}));
                        }
                    }, {t(lang, "selection.add_filter")}
                }
            }
            if mode == "fixed" { FixedNodes { lang, selection } }
        }
    }
}

#[component]
fn FixedNodes(lang: Language, mut selection: Signal<Value>) -> Element {
    let mut nodes = use_signal(Vec::<NodeDto>::new);
    let mut cursor = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut load = move || {
        if *busy.peek() { return; }
        busy.set(true);
        error.set(String::new());
        let path = cursor.peek().as_ref()
            .map_or_else(|| "/nodes?limit=100".to_owned(), |cursor| format!("/nodes?limit=100&cursor={cursor}"));
        spawn(async move {
            match crate::api::get::<ListNodesResponse>(&path).await {
                Ok(response) => { nodes.write().extend(response.nodes); cursor.set(response.next_cursor); }
                Err(err) => error.set(err.message),
            }
            busy.set(false);
        });
    };
    use_future(move || async move { load(); });
    let selected = selection.read()["nodeIds"].as_array().cloned().unwrap_or_default();
    rsx! {
        p { class: "mt-3 text-sm", "{selected.len()} " {t(lang, "selection.selected")} }
        div { class: "mt-2 max-h-64 overflow-y-auto",
            for node in nodes.read().iter() {
                label { key: "{node.id}", class: "flex min-h-11 items-center gap-2 border-b border-line-soft py-2 text-sm",
                    input { r#type: "checkbox", checked: selected.contains(&json!(node.id)),
                        disabled: !node.is_active || node.missing_from_source,
                        onchange: {
                            let id = node.id.clone();
                            move |event: Event<FormData>| {
                                let mut value = selection.write();
                                if !value["nodeIds"].is_array() { value["nodeIds"] = json!([]); }
                                if let Some(ids) = value["nodeIds"].as_array_mut() {
                                    ids.retain(|entry| entry != &json!(id));
                                    if event.checked() { ids.push(json!(id)); }
                                }
                            }
                        }
                    }
                    span { class: "min-w-0 break-all",
                        "{node.display_name}"
                        span { class: "block text-xs text-muted", "{node.protocol} · {node.host}:{node.port}" }
                    }
                }
            }
        }
        if *busy.read() { p { role: "status", class: "mt-2 text-sm", {t(lang, "common.loading")} } }
        if !error.read().is_empty() { p { role: "alert", class: "ui-error mt-2 text-sm", "{error}" } }
        if cursor.read().is_some() || !error.read().is_empty() {
            button { r#type: "button", class: "ui-button mt-2", disabled: *busy.read(),
                onclick: move |_| load(), {t(lang, "nodes.load_more")} }
        }
        if !selected.is_empty() {
            button { r#type: "button", class: "ui-button mt-2",
                onclick: move |_| selection.write()["nodeIds"] = json!([]), {t(lang, "selection.clear")} }
        }
    }
}
