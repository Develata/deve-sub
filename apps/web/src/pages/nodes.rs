//! Nodes page — virtual scroll list, multi-select, filtering, and
//! management actions (import, batch enable/disable, batch tags, per-node
//! override/region/tags/chain).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::{get, send};
use crate::i18n::{Language, format_t, t};
use crate::pages::node_chain_modal::ChainModal;
use crate::pages::node_import_modal::ImportModal;
use crate::pages::node_override_modal::{OverrideModal, RegionModal};
use crate::pages::node_tag_modal::TagModal;
use crate::pages::node_types::{
    BatchEnabledRequest, BatchResultDto, ListNodesResponse, NodeDto, NodeModal,
};

const ITEM_HEIGHT: f64 = 48.0;
const VIEWPORT_HEIGHT: f64 = 600.0;
const BUFFER: usize = 5;

#[derive(Props, Clone, PartialEq)]
pub struct NodesProps {
    lang: Signal<Language>,
}

pub fn NodesPage(props: NodesProps) -> Element {
    let l = *props.lang.read();
    let mut nodes = use_signal(Vec::<NodeDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(String::new);
    let mut search = use_signal(String::new);
    let mut protocol_filter = use_signal(String::new);
    let mut selected = use_signal(std::collections::HashSet::<String>::new);
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut cursor = use_signal(|| Option::<String>::None);
    let mut loading_more = use_signal(|| false);
    let mut modal = use_signal(|| NodeModal::None);
    let mut batch_msg = use_signal(String::new);

    let fetch_nodes = move || {
        spawn(async move {
            match get::<ListNodesResponse>("/nodes?limit=10000").await {
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
    };

    use_future(move || async move {
        match get::<ListNodesResponse>("/nodes?limit=10000").await {
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
            match get::<ListNodesResponse>(&path).await {
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

    let batch_set_enabled = move |enabled: bool| {
        let ids: Vec<String> = selected.read().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        batch_msg.set(String::new());
        spawn(async move {
            let req = BatchEnabledRequest {
                node_ids: ids,
                enabled,
            };
            match send::<BatchResultDto, _>("POST", "/nodes/batch-enabled", Some(&req)).await {
                Ok(r) => {
                    batch_msg.set(format_t(
                        l,
                        if enabled { "nodes.batch_enabled_ok" } else { "nodes.batch_disabled_ok" },
                        r.updated as usize,
                    ));
                    fetch_nodes();
                    selected.write().clear();
                }
                Err(e) => {
                    batch_msg.set(e.message);
                }
            }
        });
    };

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

    let protocols: Vec<String> = {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in nodes.read().iter() {
            set.insert(n.protocol.clone());
        }
        set.into_iter().collect()
    };

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
    let has_selection = selected_count > 0;

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nodes.title")} }
                if selected_count > 0 {
                    span { class: "text-sm text-amber-600 dark:text-amber-500", {format_t(l, "nodes.selected", selected_count)} }
                }
            }

            // Toolbar.
            div { class: "flex flex-wrap items-center gap-2",
                button {
                    class: "rounded-md bg-amber-600 px-3 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: move |_| modal.set(NodeModal::Import),
                    {t(l, "nodes.import_btn")}
                }
                if has_selection {
                    button {
                        class: "rounded-md border border-green-300 px-3 py-2 text-sm text-green-700 hover:bg-green-50 dark:border-green-700 dark:text-green-400 dark:hover:bg-green-900/20",
                        onclick: move |_| batch_set_enabled(true),
                        {t(l, "nodes.batch_enable")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| batch_set_enabled(false),
                        {t(l, "nodes.batch_disable")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| {
                            let ids: Vec<String> = selected.read().iter().cloned().collect();
                            modal.set(NodeModal::Tags(ids));
                        },
                        {t(l, "nodes.batch_tags")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-500 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800",
                        onclick: move |_| selected.write().clear(),
                        {t(l, "nodes.clear_selection")}
                    }
                }
            }

            if !batch_msg.read().is_empty() {
                div { class: "rounded-md bg-blue-50 p-3 text-sm text-blue-700 dark:bg-blue-900/20 dark:text-blue-400", "{batch_msg}" }
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
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else {
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
                            div { class: "flex items-center border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                div { class: "w-10 px-3 py-3" }
                                div { class: "flex-1 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.protocol")} }
                                div { class: "w-28 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.region")} }
                                div { class: "w-20 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.status")} }
                                div { class: "w-44 px-3 py-2 text-xs font-medium text-stone-500 dark:text-stone-400", {t(l, "nodes.actions")} }
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
                                    let ov_id = node_id.clone();
                                    let reg_id = node_id.clone();
                                    let tag_id = node_id.clone();
                                    let chain_id = node_id.clone();
                                    let chain_initial = node.chain.clone();
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
                                            div { class: "w-28 px-3 py-2",
                                                span { class: "inline-flex rounded-full px-2 py-0.5 text-xs font-medium {protocol_class}", "{node.protocol}" }
                                            }
                                            div { class: "w-20 px-3 py-2 text-sm text-stone-600 dark:text-stone-400",
                                                {node.region.clone().unwrap_or_else(|| "—".to_string())}
                                            }
                                            div { class: "w-20 px-3 py-2",
                                                if node.is_active {
                                                    span { class: "inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300", {t(l, "nodes.enabled")} }
                                                } else {
                                                    span { class: "inline-flex rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-500 dark:bg-stone-800 dark:text-stone-400", {t(l, "nodes.disabled")} }
                                                }
                                            }
                                            div { class: "w-44 px-3 py-2 flex gap-1",
                                                button {
                                                    class: "text-xs text-amber-600 hover:text-amber-800 dark:text-amber-500",
                                                    title: {t(l, "nodes.override_title")},
                                                    onclick: move |e| {
                                                        e.stop_propagation();
                                                        modal.set(NodeModal::Override(ov_id.clone()));
                                                    },
                                                    {t(l, "nodes.row_override")}
                                                }
                                                button {
                                                    class: "text-xs text-stone-500 hover:text-stone-700 dark:text-stone-400",
                                                    title: {t(l, "nodes.region_title")},
                                                    onclick: move |e| {
                                                        e.stop_propagation();
                                                        modal.set(NodeModal::SetRegion(reg_id.clone()));
                                                    },
                                                    {t(l, "nodes.row_region")}
                                                }
                                                button {
                                                    class: "text-xs text-stone-500 hover:text-stone-700 dark:text-stone-400",
                                                    title: {t(l, "nodes.tag_title")},
                                                    onclick: move |e| {
                                                        e.stop_propagation();
                                                        modal.set(NodeModal::Tags(vec![tag_id.clone()]));
                                                    },
                                                    {t(l, "nodes.row_tags")}
                                                }
                                                button {
                                                    class: "text-xs text-stone-500 hover:text-stone-700 dark:text-stone-400",
                                                    title: {t(l, "nodes.chain_title")},
                                                    onclick: move |e| {
                                                        e.stop_propagation();
                                                        modal.set(NodeModal::Chain(chain_id.clone(), chain_initial.clone()));
                                                    },
                                                    {t(l, "nodes.row_chain")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if cursor.read().is_some() {
                    div { class: "flex justify-center pt-4",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *loading_more.read(),
                            onclick: load_more,
                            if *loading_more.read() { {t(l, "common.loading")} } else { {t(l, "nodes.load_more")} }
                        }
                    }
                }
            }
        }

        // Modals.
        match &*modal.read() {
            NodeModal::None => {}
            NodeModal::Import => {
                ImportModal {
                    lang: props.lang,
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            }
            NodeModal::Tags(ids) => {
                TagModal {
                    lang: props.lang,
                    node_ids: ids.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            }
            NodeModal::SetRegion(id) => {
                RegionModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            }
            NodeModal::Override(id) => {
                OverrideModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            }
            NodeModal::Chain(id, chain) => {
                ChainModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    initial_chain: chain.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            }
        }
    }
}
