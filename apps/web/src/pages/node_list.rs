//! Bounded virtual rows with visible tag membership and scoped selection.
#![cfg(target_family = "wasm")]
use super::{node_selection::NodeSelection, node_types::{NodeDto, NodeModal}};
use crate::i18n::{Language, t};
use dioxus::prelude::*;

/// Visible node rows and the parent-owned selection and scroll state.
#[derive(Props, Clone, PartialEq)]
pub struct NodeListProps {
    lang: Signal<Language>,
    nodes: Vec<NodeDto>,
    selected: Signal<NodeSelection>,
    modal: Signal<NodeModal>,
    total: usize,
    total_height: f64,
    offset_y: f64,
    scroll_top: Signal<f64>,
    item_height: f64,
    viewport_height: f64,
}

/// Render the bounded node table and dispatch node editing intent.
pub fn NodeList(mut props: NodeListProps) -> Element {
    let l = *props.lang.read();
    rsx! {
        div { class: "node-table overflow-auto rounded-lg border border-stone-200 dark:border-stone-800",
            id: "nodes-scroll", "data-total-rows": "{props.total}", style: "height: {props.viewport_height}px;",
            onscroll: move |_| {
                if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("nodes-scroll")) {
                    props.scroll_top.set(el.scroll_top() as f64);
                }
            },
            div { class: "node-grid node-table-header bg-stone-50 dark:bg-stone-900", "data-node-header": "true",
                span {} span { {t(l, "nodes.override_name")} } span { {t(l, "nodes.protocol")} }
                span { {t(l, "nodes.region")} } span { {t(l, "nodes.status")} } span { {t(l, "nodes.actions")} }
            }
            div { style: "height: {props.total_height}px; position: relative; min-width: 760px;",
                div { style: "position: absolute; top: {props.offset_y}px; left: 0; right: 0;",
                    for node in &props.nodes {
                        {
                            let id = node.id.clone(); let checkbox_id = id.clone(); let row_id = id.clone();
                            let tags_id = id.clone(); let override_id = id.clone(); let region_id = id.clone();
                            let chain_id = id.clone(); let chain = node.chain.clone();
                            let checked = props.selected.read().ids().contains(&id);
                            rsx! {
                                div { key: "{id}", "data-node-row": "{id}",
                                    class: if checked { "node-grid node-row node-selected" } else { "node-grid node-row" },
                                    style: "height: {props.item_height}px;",
                                    onclick: move |_| props.selected.write().toggle(row_id.clone()),
                                    input { r#type: "checkbox", checked, aria_label: format!("{} {}", t(l, "nodes.select"), node.display_name),
                                        onclick: move |e| e.stop_propagation(),
                                        onchange: move |e| props.selected.write().set(checkbox_id.clone(), e.checked()),
                                    }
                                    div { class: "min-w-0",
                                        div { class: "truncate text-sm font-medium", title: "{node.display_name}", "{node.display_name}" }
                                        div { class: "flex items-center gap-1 overflow-hidden text-xs text-stone-500",
                                            span { class: "truncate", "{node.host}:{node.port}" }
                                            for tag in &node.tags { span { class: "node-tag", title: "{tag.name}", span { class: "node-tag-dot", style: format!("background-color: {}", tag.color.as_deref().unwrap_or("#b45309")), aria_hidden: "true" } "{tag.name}" } }
                                        }
                                    }
                                    span { class: "text-xs font-medium", "{node.protocol}" }
                                    span { class: "text-sm", {node.region.as_deref().unwrap_or("—")} }
                                    span { class: if node.is_active { "text-xs text-green-700 dark:text-green-400" } else { "text-xs text-stone-500" },
                                        if node.is_active { {t(l, "nodes.enabled")} } else { {t(l, "nodes.disabled")} }
                                    }
                                    div { class: "flex items-center", onclick: move |e| e.stop_propagation(),
                                        button { class: "node-row-action", onclick: move |_| props.modal.set(NodeModal::Override(override_id.clone())), {t(l, "nodes.row_override")} }
                                        button { class: "node-row-action", onclick: move |_| props.modal.set(NodeModal::SetRegion(region_id.clone())), {t(l, "nodes.row_region")} }
                                        button { class: "node-row-action", onclick: move |_| props.modal.set(NodeModal::Tags(vec![tags_id.clone()])), {t(l, "nodes.row_tags")} }
                                        button { class: "node-row-action", onclick: move |_| props.modal.set(NodeModal::Chain(chain_id.clone(), chain.clone())), {t(l, "nodes.row_chain")} }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if props.total == 0 { p { class: "py-4 text-center text-sm text-stone-500", {t(l, "nodes.no_matches")} } }
    }
}
