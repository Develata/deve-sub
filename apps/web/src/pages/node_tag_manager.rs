//! Tag identity management, separate from node membership editing.
#![cfg(target_family = "wasm")]
use crate::{
    api,
    i18n::{Language, t},
};
use deve_sub_contract::{CreateTagRequest, ListTagsResponse, TagDto, TagResponse};
use dioxus::prelude::*;

/// Completion and dismissal callbacks for managing tag identities.
#[derive(Props, Clone, PartialEq)]
pub struct TagManagerProps {
    lang: Signal<Language>,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

/// Edit tag identities through the typed administration API.
pub fn TagManager(props: TagManagerProps) -> Element {
    let l = *props.lang.read();
    let mut tags = use_signal(Vec::<TagDto>::new);
    let mut name = use_signal(String::new);
    let mut color = use_signal(|| "#b45309".to_string());
    let mut editing = use_signal(|| None::<String>);
    let mut deleting = use_signal(|| None::<TagDto>);
    let mut busy = use_signal(|| false);
    let mut loaded = use_signal(|| false);
    let mut error = use_signal(String::new);
    use_future(move || async move {
        match api::get::<ListTagsResponse>("/tags").await {
            Ok(resp) => {
                tags.set(resp.tags);
                loaded.set(true);
            }
            Err(e) => error.set(e.message),
        }
    });
    let mut save = move |_| {
        if *busy.read() || !*loaded.read() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        let id = editing.read().clone();
        let body = CreateTagRequest {
            name: name.read().clone(),
            color: Some(color.read().clone()),
        };
        spawn(async move {
            let (method, path) = id.as_ref().map_or(("POST", "/tags".to_string()), |id| {
                ("PATCH", format!("/tags/{id}"))
            });
            match api::send::<TagResponse, _>(method, &path, Some(&body)).await {
                Ok(resp) => {
                    let mut rows = tags.write();
                    rows.retain(|tag| tag.id != resp.tag.id);
                    rows.push(resp.tag);
                    rows.sort_by(|a, b| a.name.cmp(&b.name));
                    editing.set(None);
                    name.set(String::new());
                    props.on_success.call(());
                }
                Err(e) => error.set(e.message),
            }
            busy.set(false);
        });
    };
    let mut remove = move |_| {
        if *busy.read() {
            return;
        }
        let Some(tag) = deleting.read().clone() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn(async move {
            match api::delete(&format!("/tags/{}", tag.id)).await {
                Ok(()) => {
                    tags.write().retain(|t| t.id != tag.id);
                    deleting.set(None);
                    if editing.read().as_ref() == Some(&tag.id) {
                        editing.set(None);
                        name.set(String::new());
                    }
                    props.on_success.call(());
                }
                Err(e) => error.set(e.message),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-3",
            onclick: move |_| { if !*busy.read() { props.on_close.call(()); } },
            div { class: "node-dialog-panel w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                role: "dialog", aria_label: t(l, "nodes.manage_tags"), aria_modal: "true",
                onkeydown: move |e| { if e.key() == Key::Escape && !*busy.read() { props.on_close.call(()); } }, onclick: move |e| e.stop_propagation(),
                h3 { class: "text-lg font-semibold", {t(l, "nodes.manage_tags")} }
                p { class: "my-2 text-sm text-stone-500", {t(l, "nodes.manage_tags_hint")} }
                if !error.read().is_empty() { p { role: "alert", class: "my-2 text-sm text-red-600", "{error}" } }
                div { class: "my-4 max-h-64 space-y-2 overflow-y-auto",
                    for tag in tags.read().iter() {
                        { let edit = tag.clone(); let remove_tag = tag.clone(); rsx! {
                            div { class: "flex items-center justify-between gap-2", "data-tag-id": "{tag.id}",
                                span { class: "node-tag truncate", span { class: "node-tag-dot", style: format!("background-color: {}", tag.color.as_deref().unwrap_or("#b45309")), aria_hidden: "true" } "{tag.name}" }
                                div { class: "flex shrink-0 gap-1",
                                    button { class: "node-control", disabled: *busy.read(), onclick: move |_| { name.set(edit.name.clone()); color.set(edit.color.clone().unwrap_or_else(|| "#b45309".into())); editing.set(Some(edit.id.clone())); }, {t(l, "common.edit")} }
                                    button { class: "node-control text-red-600", disabled: *busy.read(), onclick: move |_| deleting.set(Some(remove_tag.clone())), {t(l, "common.delete")} }
                                }
                            }
                        } }
                    }
                }
                if let Some(tag) = deleting.read().as_ref() {
                    div { class: "my-3 space-y-2", role: "alert",
                        p { "{tag.name}: " {t(l, "nodes.tag_delete_confirm")} }
                        button { class: "node-control text-red-600", disabled: *busy.read(), onclick: move |e| remove(e), {t(l, "common.delete")} }
                        button { class: "node-control", disabled: *busy.read(), onclick: move |_| deleting.set(None), {t(l, "common.cancel")} }
                    }
                }
                div { class: "flex flex-wrap gap-2",
                    input { class: "node-control min-w-0 flex-1", autofocus: true, aria_label: t(l, "nodes.tag_new_name"), placeholder: t(l, "nodes.tag_new_name"), value: "{name}", disabled: *busy.read(), oninput: move |e| name.set(e.value()) }
                    input { class: "node-control", r#type: "color", aria_label: t(l, "nodes.tag_color"), value: "{color}", disabled: *busy.read(), oninput: move |e| color.set(e.value()) }
                    button { class: "node-control", disabled: *busy.read() || !*loaded.read() || name.read().trim().is_empty(), onclick: move |e| save(e), if editing.read().is_some() { {t(l, "common.save")} } else { {t(l, "nodes.tag_create")} } }
                }
                div { class: "mt-5 flex justify-end", button { class: "node-control", disabled: *busy.read(), onclick: move |_| props.on_close.call(()), {t(l, "common.close")} } }
            }
        }
    }
}
