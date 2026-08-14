//! Nodes page — virtual scroll list, multi-select, filtering (UI-008).
//!
//! Supports 10,000+ nodes via manual virtual scrolling: only visible items
//! (viewport / item_height + buffer) are rendered. Search by name, filter
//! by protocol.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, format_t, t};

const ITEM_HEIGHT: f64 = 48.0;
const VIEWPORT_HEIGHT: f64 = 600.0;
const BUFFER: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDto {
    pub id: String,
    pub display_name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub region: Option<String>,
    pub source_label: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNodesResponse {
    pub nodes: Vec<NodeDto>,
    pub next_cursor: Option<String>,
}

#[derive(Props, Clone, PartialEq)]
pub struct NodesProps {
    lang: Signal<Language>,
}

pub fn NodesPage(props: NodesProps) -> Element {
    let l = *props.lang.read();
    let mut nodes = use_signal(Vec::<NodeDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| String::new());
    let mut search = use_signal(String::new);
    let mut protocol_filter = use_signal(String::new);
    let mut selected = use_signal(std::collections::HashSet::<String>::new);
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut cursor = use_signal(|| Option::<String>::None);
    let mut loading_more = use_signal(|| false);

    // Fetch all nodes. Virtual scroll ensures only visible rows are rendered,
    // so holding 10k nodes in memory is cheap; client-side filter then searches
    // the full set instead of just the first page.
    use_future(move || async move {
        match crate::api::get::<ListNodesResponse>("/nodes?limit=10000").await {
            Ok(resp) => {
                nodes.set(resp.nodes);
                cursor.set(resp.next_cursor);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    let load_more = move |_| {
        if *loading_more.read() {
            return;
        }
        let c = cursor.read().clone();
        let Some(c) = c else { return };
        loading_more.set(true);
        spawn(async move {
            let path = format!("/nodes?limit=100&cursor={c}");
            match crate::api::get::<ListNodesResponse>(&path).await {
                Ok(resp) => {
                    let mut current = nodes.read().clone();
                    current.extend(resp.nodes);
                    nodes.set(current);
                    cursor.set(resp.next_cursor);
                    loading_more.set(false);
                }
                Err(e) => {
                    error.set(e.message);
                    loading_more.set(false);
                }
            }
        });
    };

    // Filter nodes by search and protocol.
    let filtered: Vec<NodeDto> = {
        let all = nodes.read();
        let s = search.read().to_lowercase();
        let p = protocol_filter.read().clone();
        all.iter()
            .filter(|n| s.is_empty() || n.display_name.to_lowercase().contains(&s))
            .filter(|n| p.is_empty() || n.protocol == p)
            .cloned()
            .collect()
    };

    // Collect unique protocols for filter dropdown.
    let protocols: Vec<String> = {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in nodes.read().iter() {
            set.insert(n.protocol.clone());
        }
        set.into_iter().collect()
    };

    // Virtual scroll calculations.
    let total = filtered.len();
    let total_height = total as f64 * ITEM_HEIGHT;
    let current_scroll = *scroll_top.read();
    let start_idx = ((current_scroll / ITEM_HEIGHT) as usize).saturating_sub(BUFFER);
    let visible_count = ((VIEWPORT_HEIGHT / ITEM_HEIGHT) as usize) + 2 * BUFFER;
    let end_idx = (start_idx + visible_count).min(total);
    let visible_items: Vec<&NodeDto> = filtered
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
        .collect();
    let offset_y = start_idx as f64 * ITEM_HEIGHT;

    let selected_count = selected.read().len();

    rsx! {
        div { class: "space-y-4",
            // Header.
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nodes.title")} }
                if selected_count > 0 {
                    span { class: "text-sm text-amber-600 dark:text-amber-500", {format_t(l, "nodes.selected", selected_count)} }
                }
            }

            // Filters.
            div { class: "flex flex-wrap items-center gap-3",
                input {
                    class: "flex-1 rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                    r#type: "search",
                    placeholder: {t(l, "nodes.search")},
                    value: "{search}",
                    oninput: move |e| search.set(e.value()),
                }
                select {
                    class: "rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                    value: "{protocol_filter}",
                    onchange: move |e| protocol_filter.set(e.value()),
                    option { value: "", {t(l, "nodes.all_protocols")} }
                    for p in &protocols {
                        option { value: "{p}", "{p}" }
                    }
                }
                button {
                    class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                    onclick: move |_| {
                        selected.write().clear();
                    },
                    if selected_count > 0 { "清除选择" } else { "" }
                }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else {
                // Virtual scroll container.
                div {
                    class: "overflow-y-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    id: "nodes-scroll",
                    style: "height: {VIEWPORT_HEIGHT}px;",
                    onscroll: move |_| {
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.get_element_by_id("nodes-scroll"))
                        {
                            scroll_top.set(el.scroll_top() as f64);
                        }
                    },
                    div { style: "height: {total_height}px; position: relative;",
                        div { style: "position: absolute; top: {offset_y}px; left: 0; right: 0;",
                            // Column header.
                            div { class: "flex items-center border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                div { class: "w-10 px-3 py-3" }
                                div { class: "flex-1 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.protocol")} }
                                div { class: "w-32 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.region")} }
                                div { class: "w-24 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                            }
                            for (i, node) in visible_items.iter().enumerate() {
                                {
                                    let node_id = node.id.clone();
                                    let onclick_id = node_id.clone();
                                    let onchange_id = node_id.clone();
                                    let is_selected = selected.read().contains(&node_id);
                                    let row_class = if is_selected {
                                        "flex items-center border-b border-stone-100 bg-amber-50 dark:border-stone-800 dark:bg-amber-900/10"
                                    } else {
                                        "flex items-center border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50"
                                    };
                                    let protocol_class = match node.protocol.as_str() {
                                        "Vless" => "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300",
                                        "Trojan" => "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300",
                                        "Shadowsocks" => "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-300",
                                        "Hysteria2" => "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-300",
                                        "Tuic" => "bg-pink-100 text-pink-700 dark:bg-pink-900/30 dark:text-pink-300",
                                        _ => "bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300",
                                    };
                                    rsx! {
                                        div {
                                            key: "{node_id}",
                                            class: "{row_class}",
                                            style: "height: {ITEM_HEIGHT}px;",
                                            onclick: move |_| {
                                                let mut s = selected.write();
                                                if s.contains(&onclick_id) {
                                                    s.remove(&onclick_id);
                                                } else {
                                                    s.insert(onclick_id.clone());
                                                }
                                            },
                                            div { class: "w-10 px-3",
                                                input {
                                                    r#type: "checkbox",
                                                    class: "rounded border-stone-300 text-amber-600 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800",
                                                    checked: is_selected,
                                                    onchange: move |e| {
                                                        let mut s = selected.write();
                                                        if e.checked() {
                                                            s.insert(onchange_id.clone());
                                                        } else {
                                                            s.remove(&onchange_id);
                                                        }
                                                    },
                                                }
                                            }
                                            div { class: "flex-1 px-3 py-2 text-sm text-stone-900 dark:text-stone-100",
                                                span { class: "font-medium", "{node.display_name}" }
                                                span { class: "ml-2 text-xs text-stone-400 dark:text-stone-500", "{node.host}:{node.port}" }
                                            }
                                            div { class: "w-32 px-3 py-2",
                                                span { class: "inline-flex rounded-full px-2 py-0.5 text-xs font-medium {protocol_class}", "{node.protocol}" }
                                            }
                                            div { class: "w-24 px-3 py-2 text-sm text-stone-600 dark:text-stone-400",
                                                {node.region.clone().unwrap_or_else(|| "—".to_string())}
                                            }
                                            div { class: "w-24 px-3 py-2",
                                                if node.is_active {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Load more button.
                if cursor.read().is_some() {
                    div { class: "flex justify-center pt-4",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *loading_more.read(),
                            onclick: load_more,
                            if *loading_more.read() { {t(l, "common.loading")} } else { "加载更多" }
                        }
                    }
                }
            }
        }
    }
}
