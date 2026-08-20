//! Tag assignment modal — single node or batch (NODE-005).
//!
//! Fetches all tags, lets admin toggle tag membership for the target node(s),
//! and supports creating a new tag inline. For single-node edits, uses
//! `PUT /nodes/{id}/tags` (replace). For batch, uses `POST /nodes/batch-tags`.

#![cfg(target_family = "wasm")]

use std::collections::HashSet;

use dioxus::prelude::*;

use crate::api::{get, send};
use crate::i18n::{Language, t};
use crate::pages::node_types::{
    BatchTagsRequest, CreateTagRequest, ListTagsResponse, NodeTagAssignmentDto, SetNodeTagsRequest,
    TagDto, TagResponse,
};

#[derive(Props, Clone, PartialEq)]
pub struct TagModalProps {
    lang: Signal<Language>,
    /// Node IDs to assign tags to. Length 1 = single, >1 = batch.
    node_ids: Vec<String>,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

pub fn TagModal(props: TagModalProps) -> Element {
    let l = *props.lang.read();
    let mut all_tags = use_signal(Vec::<TagDto>::new);
    let mut selected = use_signal(HashSet::<String>::new);
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut new_tag_name = use_signal(String::new);

    // Load existing tags once.
    use_future(move || async move {
        match get::<ListTagsResponse>("/tags").await {
            Ok(resp) => {
                all_tags.set(resp.tags);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    let toggle = move |id: String| {
        let mut s = selected.write();
        if s.contains(&id) {
            s.remove(&id);
        } else {
            s.insert(id);
        }
    };

    let create_tag = move |_| {
        let name = new_tag_name.read().trim().to_string();
        if name.is_empty() {
            return;
        }
        saving.set(true);
        error.set(String::new());
        spawn(async move {
            let req = CreateTagRequest {
                name,
                color: None,
            };
            match send::<TagResponse, _>("POST", "/tags", Some(&req)).await {
                Ok(resp) => {
                    let mut current = all_tags.write();
                    current.push(resp.tag);
                    new_tag_name.set(String::new());
                    saving.set(false);
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    let submit = move |_| {
        let ids: Vec<String> = selected.read().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        saving.set(true);
        error.set(String::new());
        let targets = props.node_ids.clone();
        spawn(async move {
            let result = if targets.len() == 1 {
                let req = SetNodeTagsRequest { tag_ids: ids };
                let path = format!("/nodes/{}/tags", &targets[0]);
                send::<(), _>("PUT", &path, Some(&req)).await
            } else {
                let assignments: Vec<NodeTagAssignmentDto> = targets
                    .iter()
                    .map(|nid| NodeTagAssignmentDto {
                        node_id: nid.clone(),
                        tag_ids: ids.clone(),
                    })
                    .collect();
                let req = BatchTagsRequest { assignments };
                send::<(), _>("POST", "/nodes/batch-tags", Some(&req)).await
            };
            match result {
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
                    {t(l, "nodes.tag_title")}
                }

                if *loading.read() {
                    div { class: "mt-4 flex justify-center py-8",
                        div { class: "h-5 w-5 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600" }
                    }
                } else {
                    div { class: "mt-4 space-y-2 max-h-64 overflow-y-auto",
                        if all_tags.read().is_empty() {
                            p { class: "text-sm text-stone-500 dark:text-stone-400", {t(l, "nodes.tag_empty")} }
                        } else {
                            for tag in all_tags.read().iter() {
                                {
                                    let tid = tag.id.clone();
                                    let is_on = selected.read().contains(&tid);
                                    rsx! {
                                        label {
                                            class: "flex items-center gap-2 rounded-md border border-stone-200 px-3 py-2 text-sm hover:bg-stone-50 dark:border-stone-700 dark:hover:bg-stone-800",
                                            input {
                                                r#type: "checkbox",
                                                checked: is_on,
                                                onchange: move |e| {
                                                    if e.checked() {
                                                        selected.write().insert(tid.clone());
                                                    } else {
                                                        selected.write().remove(&tid);
                                                    }
                                                },
                                            }
                                            span { class: "text-stone-800 dark:text-stone-200", "{tag.name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Inline create.
                    div { class: "mt-4 flex gap-2",
                        input {
                            class: "flex-1 rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                            r#type: "text",
                            placeholder: {t(l, "nodes.tag_new_name")},
                            value: "{new_tag_name}",
                            oninput: move |e| new_tag_name.set(e.value()),
                        }
                        button {
                            class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *saving.read(),
                            onclick: create_tag,
                            {t(l, "nodes.tag_create")}
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
}
